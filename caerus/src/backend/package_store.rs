//! `PackageStore` — loads the full package list (repository + installed)
//! via direct `libxbps` calls and exposes it as a `gio::ListStore` the
//! UI filters/sorts/searches live. Rust translation of backend/
//! package_store.{h,c}.
//!
//! ## Concurrency model (the actual point of this rewrite)
//!
//! The original C version kept `struct xbps_handle xh` on
//! `CaerusPackageStore` itself, torn down and rebuilt in place
//! (`xbps_end` + `xbps_init`) by a background `GTask` thread on every
//! reload, while the main thread could — via the detail pane's
//! `get_deps`/`get_rdeps`/`get_files`/`get_extra_info` getters — read
//! `xh` directly at any time, including mid-reload. A `GMutex` was
//! added to close that specific race, but the crash (a `SIGSEGV` after
//! several rapid reload cycles, with the crash log showing three
//! automatic sequential reloads and *no* user row selection) persisted,
//! pointing at a second, structural problem: repeated `xbps_end`/
//! `xbps_init` cycles firing back-to-back or re-entrantly, which
//! `libxbps` was never designed to tolerate (its own docs describe one
//! init per process lifetime).
//!
//! Rather than add another lock around the same shared, mutable
//! `xbps_handle` and hope the specific interleavings that caused the
//! corruption can't recur, this rewrite removes the shared state
//! entirely:
//!
//!   * exactly one dedicated OS thread (`worker_main` below) ever
//!     touches `libxbps` or holds an `xbps_handle`, for the entire
//!     process lifetime;
//!   * every other part of the program — reload, and every detail-pane
//!     lookup — is just a message sent down an `mpsc::Sender<Cmd>` to
//!     that thread;
//!   * the worker's `recv()` loop processes exactly one `Cmd` at a
//!     time, strictly sequentially, by construction (it's a plain
//!     `while let Ok(cmd) = rx.recv()` loop, not a thread pool).
//!
//! This makes concurrent/re-entrant access to the handle a *type-level*
//! impossibility rather than something a mutex has to arbitrate at
//! runtime — there is no code path anywhere that could invoke
//! `xbps_init`/`xbps_end` (or any other `libxbps` call) from two places
//! at once, because there is only ever one place. `xbps_init` is still
//! called once at first use and (for fidelity with the original, and
//! because forcing `libxbps` to re-read pkgdb + repo indices after an
//! out-of-process `xbps-install`/`xbps-remove` needs it) `xbps_end` +
//! `xbps_init` again on each explicit reload — but never concurrently
//! or re-entrantly, which is what actually produced the corruption.
//!
//! Two request styles are used, matching the original's own split:
//!   * `load_async()` (was: `caerus_package_store_load_async`) is
//!     fire-and-forget; the result comes back via a small local-main-
//!     loop poll and is applied to the `gio::ListStore`, then
//!     registered callbacks fire — mirroring the old "load-started"/
//!     "load-finished"/"load-error" signals.
//!   * the per-package detail getters (`get_deps` etc.) block the
//!     calling thread briefly on a oneshot reply channel — mirroring
//!     the original's synchronous, mutex-guarded getters, just via
//!     message-passing instead of a shared lock. These are fast
//!     lookups (no rescan), so a brief block on the main thread is the
//!     same cost the original paid while holding its mutex.

use crate::backend::package::{Package, PackageExtraInfo, PackageObject, PkgMark, PkgState};
use gio::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// ── Messages to/from the worker thread ─────────────────────────────

enum LoadResult {
    Ok(Vec<Package>),
    Err(String),
}

enum Cmd {
    Reload,
    GetDeps(String, mpsc::Sender<Option<Vec<String>>>),
    GetRdeps(String, mpsc::Sender<Option<Vec<String>>>),
    GetFiles(String, mpsc::Sender<Option<Vec<String>>>),
    GetExtraInfo(String, mpsc::Sender<Option<PackageExtraInfo>>),
    GetMissingDeps(
        String,
        HashMap<String, PkgState>,
        mpsc::Sender<Option<Vec<String>>>,
    ),
    Shutdown,
}

/// Whether a carried-over mark still makes sense once a reload delivers
/// the package's current real state — e.g. a `Remove` mark set before
/// the reload is meaningless if the package turns out to no longer be
/// installed at all.
fn mark_is_valid_for_state(mark: PkgMark, state: PkgState) -> bool {
    match mark {
        PkgMark::None => true,
        PkgMark::Install => state == PkgState::NotInstalled,
        PkgMark::Upgrade => state == PkgState::Upgradable,
        PkgMark::Remove | PkgMark::Purge => matches!(
            state,
            PkgState::Installed | PkgState::Upgradable | PkgState::OnHold | PkgState::Broken
        ),
    }
}

// ── Public, GTK-side handle ─────────────────────────────────────────

type LoadStartedCbs = RefCell<Vec<Box<dyn Fn()>>>;
type LoadFinishedCbs = RefCell<Vec<Box<dyn Fn(u32)>>>;
type LoadErrorCbs = RefCell<Vec<Box<dyn Fn(&str)>>>;

struct Inner {
    list: gio::ListStore,
    loading: Cell<bool>,
    loaded: Cell<bool>,
    cmd_tx: mpsc::Sender<Cmd>,
    on_load_started: LoadStartedCbs,
    on_load_finished: LoadFinishedCbs,
    on_load_error: LoadErrorCbs,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Shutdown);
    }
}

/// Cheaply-`Clone`able handle (an `Rc` around the shared state),
/// mirroring how the original's `GObject`-based `CaerusPackageStore`
/// was passed around by reference-counted pointer.
#[derive(Clone)]
pub struct PackageStore {
    inner: Rc<Inner>,
}

impl PackageStore {
    pub fn new() -> Self {
        let list = gio::ListStore::new::<PackageObject>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (result_tx, result_rx) = mpsc::channel::<LoadResult>();

        thread::Builder::new()
            .name("caerus-xbps-worker".into())
            .spawn(move || worker_main(cmd_rx, result_tx))
            .expect("failed to spawn xbps worker thread");

        let inner = Rc::new(Inner {
            list,
            loading: Cell::new(false),
            loaded: Cell::new(false),
            cmd_tx,
            on_load_started: RefCell::new(Vec::new()),
            on_load_finished: RefCell::new(Vec::new()),
            on_load_error: RefCell::new(Vec::new()),
        });

        // Poll for reload results on the GTK main loop. A reload
        // happens at most a handful of times per session (launch,
        // Fetch Updates, manual Reload), so a short poll interval is
        // imperceptible and sidesteps cross-thread-GObject-safety
        // questions entirely: only plain `Send` data
        // (`Vec<Package>`/`String`) ever crosses the thread boundary,
        // and it is applied to the `gio::ListStore` exclusively from
        // this main-thread closure.
        {
            let inner_weak = Rc::downgrade(&inner);
            glib::source::timeout_add_local(Duration::from_millis(30), move || {
                let Some(inner) = inner_weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                while let Ok(result) = result_rx.try_recv() {
                    inner.loading.set(false);
                    match result {
                        LoadResult::Ok(packages) => {
                            let n = packages.len() as u32;

                            // A reload rebuilds the list from scratch, which
                            // would otherwise silently discard any pending
                            // marks the user set before triggering it (e.g.
                            // clicking Reload/Update mid-session). Carry
                            // each mark over by pkgname, but only where it
                            // still makes sense for the freshly-loaded
                            // state — e.g. drop a stale Remove mark if the
                            // package turns out to already be gone.
                            let mut old_marks: HashMap<String, PkgMark> = HashMap::new();
                            let old_n = inner.list.n_items();
                            for i in 0..old_n {
                                if let Some(obj) = inner.list.item(i) {
                                    let obj = obj.downcast_ref::<PackageObject>().unwrap();
                                    let p = obj.pkg();
                                    if p.mark != PkgMark::None {
                                        old_marks.insert(p.name.clone(), p.mark);
                                    }
                                }
                            }

                            inner.list.remove_all();
                            let objects: Vec<PackageObject> = packages
                                .into_iter()
                                .map(|mut pkg| {
                                    if let Some(&mark) = old_marks.get(&pkg.name) {
                                        if mark_is_valid_for_state(mark, pkg.state) {
                                            pkg.mark = mark;
                                        }
                                    }
                                    PackageObject::new(pkg)
                                })
                                .collect();
                            inner.list.splice(0, 0, &objects);
                            inner.loaded.set(true);
                            for cb in inner.on_load_finished.borrow().iter() {
                                cb(n);
                            }
                        }
                        LoadResult::Err(msg) => {
                            for cb in inner.on_load_error.borrow().iter() {
                                cb(&msg);
                            }
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        }

        PackageStore { inner }
    }

    pub fn list(&self) -> gio::ListStore {
        self.inner.list.clone()
    }

    pub fn connect_load_started(&self, f: impl Fn() + 'static) {
        self.inner.on_load_started.borrow_mut().push(Box::new(f));
    }
    pub fn connect_load_finished(&self, f: impl Fn(u32) + 'static) {
        self.inner.on_load_finished.borrow_mut().push(Box::new(f));
    }
    pub fn connect_load_error(&self, f: impl Fn(&str) + 'static) {
        self.inner.on_load_error.borrow_mut().push(Box::new(f));
    }

    /// Kicks off a background reload. Mirrors
    /// `caerus_package_store_load_async`'s own guard: a request that
    /// arrives while one is already in flight is dropped, since the
    /// in-flight load will deliver the most current data anyway.
    pub fn load_async(&self) {
        if self.inner.loading.get() {
            return;
        }
        self.inner.loading.set(true);
        for cb in self.inner.on_load_started.borrow().iter() {
            cb();
        }
        let _ = self.inner.cmd_tx.send(Cmd::Reload);
    }

    fn for_each<F: FnMut(&PackageObject)>(&self, mut f: F) {
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj) = self.inner.list.item(i) {
                f(obj.downcast_ref::<PackageObject>().unwrap());
            }
        }
    }

    pub fn count_installed(&self) -> u32 {
        let mut c = 0;
        self.for_each(|o| {
            if matches!(o.pkg().state, PkgState::Installed | PkgState::Upgradable) {
                c += 1;
            }
        });
        c
    }
    pub fn count_upgradable(&self) -> u32 {
        let mut c = 0;
        self.for_each(|o| {
            if o.pkg().state == PkgState::Upgradable {
                c += 1;
            }
        });
        c
    }
    /// Current (state, mark) for a single package by name, if it's in
    /// the store at all. Used to check whether a package that
    /// reverse-depends on something about to be removed is itself still
    /// going to be installed afterward.
    pub fn state_and_mark(&self, pkgname: &str) -> Option<(PkgState, PkgMark)> {
        let mut out = None;
        self.for_each(|o| {
            if out.is_none() && o.name() == pkgname {
                let p = o.pkg();
                out = Some((p.state, p.mark));
            }
        });
        out
    }

    /// Names of every package currently in `PkgState::Upgradable`,
    /// regardless of mark — used to preview a full system upgrade
    /// before running it (the actual `xbps-install -Su` computes its
    /// own set; this is the best local approximation of it).
    pub fn upgradable_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.for_each(|o| {
            if o.pkg().state == PkgState::Upgradable {
                out.push(o.name());
            }
        });
        out
    }
    pub fn count_marked(&self) -> u32 {
        let mut c = 0;
        self.for_each(|o| {
            if o.pkg().mark != PkgMark::None {
                c += 1;
            }
        });
        c
    }

    pub fn set_mark(&self, pkgname: &str, mark: PkgMark) {
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj) = self.inner.list.item(i) {
                let obj = obj.downcast_ref::<PackageObject>().unwrap();
                if obj.name() == pkgname {
                    obj.set_mark(mark);
                    self.inner.list.items_changed(i, 1, 1);
                    break;
                }
            }
        }
    }

    pub fn marked_names(&self, mark: PkgMark) -> Vec<String> {
        let mut out = Vec::new();
        self.for_each(|o| {
            if o.pkg().mark == mark {
                out.push(o.name());
            }
        });
        out
    }

    pub fn clear_all_marks(&self) {
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj) = self.inner.list.item(i) {
                let obj = obj.downcast_ref::<PackageObject>().unwrap();
                if obj.pkg().mark != PkgMark::None {
                    obj.set_mark(PkgMark::None);
                    self.inner.list.items_changed(i, 1, 1);
                }
            }
        }
    }

    // ── Synchronous per-package detail queries ──────────────────────

    pub fn get_deps(&self, pkgname: &str) -> Option<Vec<String>> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::GetDeps(pkgname.to_string(), tx))
            .ok()?;
        rx.recv().unwrap_or(None)
    }

    pub fn get_rdeps(&self, pkgname: &str) -> Option<Vec<String>> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::GetRdeps(pkgname.to_string(), tx))
            .ok()?;
        rx.recv().unwrap_or(None)
    }

    pub fn get_files(&self, pkgname: &str) -> Option<Vec<String>> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::GetFiles(pkgname.to_string(), tx))
            .ok()?;
        rx.recv().unwrap_or(None)
    }

    pub fn get_extra_info(&self, pkgname: &str) -> Option<PackageExtraInfo> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::GetExtraInfo(pkgname.to_string(), tx))
            .ok()?;
        rx.recv().unwrap_or(None)
    }

    /// Resolves `pkgname`'s full run_depends closure (transitive,
    /// cycle-safe) and returns the subset not currently installed.
    /// Builds a name -> PkgState snapshot from the live list first (so
    /// the worker thread never needs to touch GTK objects), then hands
    /// the whole recursive resolution to the worker in one message.
    pub fn get_missing_deps(&self, pkgname: &str) -> Option<Vec<String>> {
        let mut snapshot = HashMap::new();
        self.for_each(|o| {
            let p = o.pkg();
            snapshot.insert(p.name.clone(), p.state);
        });
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::GetMissingDeps(pkgname.to_string(), snapshot, tx))
            .ok()?;
        rx.recv().unwrap_or(None)
    }
}

// ── Worker thread ────────────────────────────────────────────────────
//
// Everything below this point runs exclusively on the dedicated xbps
// worker thread. `xh` never leaves this function's stack frame and is
// never wrapped in an `Arc`/`Mutex`/sent anywhere — the channel is the
// only boundary.

fn worker_main(cmd_rx: mpsc::Receiver<Cmd>, result_tx: mpsc::Sender<LoadResult>) {
    // SAFETY: zero-initializing `struct xbps_handle` mirrors the
    // original's `memset(&self->xh, 0, sizeof(self->xh))` before the
    // first `xbps_init` — a valid starting bit-pattern for a plain-data
    // C struct whose fields are pointers/integers.
    let mut xh: xbps_sys::xbps_handle = unsafe { std::mem::zeroed() };
    let mut inited = false;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Reload => {
                let result = do_reload(&mut xh, &mut inited);
                let _ = result_tx.send(result);
            }
            Cmd::GetDeps(name, reply) => {
                let _ = reply.send(get_deps(&mut xh, inited, &name));
            }
            Cmd::GetRdeps(name, reply) => {
                let _ = reply.send(get_rdeps(&mut xh, inited, &name));
            }
            Cmd::GetFiles(name, reply) => {
                let _ = reply.send(get_files(&mut xh, inited, &name));
            }
            Cmd::GetExtraInfo(name, reply) => {
                let _ = reply.send(get_extra_info(&mut xh, inited, &name));
            }
            Cmd::GetMissingDeps(name, snapshot, reply) => {
                let _ = reply.send(get_missing_deps(&mut xh, inited, &name, &snapshot));
            }
            Cmd::Shutdown => break,
        }
    }

    if inited {
        unsafe { xbps_sys::xbps_end(&mut xh) };
    }
}

fn cstr(s: &str) -> CString {
    // Package/dependency/property names never legitimately contain NUL
    // bytes; falling back to a harmless empty string rather than
    // panicking keeps a single malformed entry from taking down the
    // worker thread.
    CString::new(s).unwrap_or_default()
}

unsafe fn dict_str(d: xbps_sys::xbps_dictionary_t, key: &str) -> Option<String> {
    if d.is_null() {
        return None;
    }
    let ckey = cstr(key);
    let mut val: *const c_char = std::ptr::null();
    let _ = xbps_sys::xbps_dictionary_get_cstring_nocopy(d, ckey.as_ptr(), &mut val);
    if val.is_null() {
        return None;
    }
    let s = CStr::from_ptr(val).to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Some xbps properties (e.g. "tags") may be stored as either a single
/// string or an array of strings depending on package metadata version.
/// Mirrors `dict_str_or_array_joined` in the original package_store.c.
unsafe fn dict_str_or_array_joined(d: xbps_sys::xbps_dictionary_t, key: &str) -> Option<String> {
    if d.is_null() {
        return None;
    }
    if let Some(s) = dict_str(d, key) {
        return Some(s);
    }
    let ckey = cstr(key);
    let arr = xbps_sys::xbps_dictionary_get(d, ckey.as_ptr()) as xbps_sys::xbps_array_t;
    if arr.is_null() {
        return None;
    }
    let n = xbps_sys::xbps_array_count(arr);
    if n == 0 {
        return None;
    }
    let mut parts = Vec::new();
    for i in 0..n {
        let mut item: *const c_char = std::ptr::null();
        let _ = xbps_sys::xbps_array_get_cstring_nocopy(arr, i, &mut item);
        if item.is_null() {
            continue;
        }
        parts.push(CStr::from_ptr(item).to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Version string is pkgver with "pkgname-" prefix stripped.
fn extract_version<'a>(pkgver: &'a str, pkgname: &str) -> &'a str {
    if let Some(rest) = pkgver.strip_prefix(pkgname) {
        if let Some(ver) = rest.strip_prefix('-') {
            return ver;
        }
    }
    pkgver
}

/// Callback for `xbps_rpool_foreach`. `arg` points to a
/// `HashMap<String, Package>` living on `do_reload`'s stack for the
/// duration of the call — safe because `xbps_rpool_foreach` is fully
/// synchronous and single-threaded from our side (no other thread ever
/// touches this map).
///
/// Enumeration follows the same path the original C code settled on
/// after debugging: `xbps_dictionary_iterator()` turned out not to
/// enumerate reliably on the target libxbps build, whereas
/// `xbps_dictionary_all_keys()` (returning KEYSYM objects, read via
/// `xbps_dictionary_keysym_cstring_nocopy`, *not*
/// `xbps_string_cstring_nocopy` — a type mismatch that silently
/// returned NULL for every entry) is the mechanism confirmed to work.
unsafe extern "C" fn rpool_repo_cb(
    repo: *mut xbps_sys::xbps_repo,
    arg: *mut c_void,
    _done: *mut bool,
) -> c_int {
    if repo.is_null() {
        return 0;
    }
    let ht = &mut *(arg as *mut HashMap<String, Package>);
    let idx = (*repo).idx;
    if idx.is_null() {
        return 0;
    }
    let repo_uri = if (*repo).uri.is_null() {
        None
    } else {
        Some(CStr::from_ptr((*repo).uri).to_string_lossy().into_owned())
    };

    let keys = xbps_sys::xbps_dictionary_all_keys(idx);
    if keys.is_null() {
        return 0;
    }
    let n = xbps_sys::xbps_array_count(keys);

    for i in 0..n {
        let keyobj = xbps_sys::xbps_array_get(keys, i);
        if keyobj.is_null() {
            continue;
        }
        // NOTE: `xbps_dictionary_all_keys` returns keysym objects, not
        // plain strings — see the doc comment above.
        let pkgname_ptr = xbps_sys::xbps_dictionary_keysym_cstring_nocopy(
            keyobj as xbps_sys::xbps_dictionary_keysym_t,
        );
        if pkgname_ptr.is_null() {
            continue;
        }
        let pkgname = CStr::from_ptr(pkgname_ptr).to_string_lossy().into_owned();
        if pkgname.is_empty() || ht.contains_key(&pkgname) {
            continue;
        }

        let pkgd = xbps_sys::xbps_dictionary_get(idx, pkgname_ptr) as xbps_sys::xbps_dictionary_t;
        if pkgd.is_null() {
            continue;
        }

        let pkgver = dict_str(pkgd, "pkgver");
        let short_desc = dict_str(pkgd, "short_desc").unwrap_or_default();
        let maintainer = dict_str(pkgd, "maintainer").unwrap_or_default();
        // "categories" is not a real xbps property — the correct key
        // is "tags" (confirmed against xbps's own zsh-completion
        // property list), possibly string-or-array.
        let tags = dict_str_or_array_joined(pkgd, "tags").unwrap_or_default();

        let ver = pkgver
            .as_deref()
            .map(|pv| extract_version(pv, &pkgname).to_string());

        let mut isize_: u64 = 0;
        xbps_sys::xbps_dictionary_get_uint64(pkgd, cstr("installed_size").as_ptr(), &mut isize_);
        // "download_size" is not a real property either — the binary
        // package file's size is stored as "filename-size".
        let mut dsize: u64 = 0;
        xbps_sys::xbps_dictionary_get_uint64(pkgd, cstr("filename-size").as_ptr(), &mut dsize);

        let version_available = ver.or_else(|| pkgver.clone()).unwrap_or_default();

        ht.insert(
            pkgname.clone(),
            Package {
                name: pkgname,
                version_installed: None,
                version_available: Some(version_available),
                short_desc,
                long_desc: None,
                tags,
                maintainer,
                install_size: isize_,
                download_size: dsize,
                repository: repo_uri.clone(),
                state: PkgState::NotInstalled,
                mark: PkgMark::None,
                essential: false,
            },
        );
    }

    xbps_sys::xbps_object_release(keys as xbps_sys::xbps_object_t);
    0
}

/// Callback for `xbps_pkgdb_foreach_cb_multi`. Same single-threaded
/// safety argument as `rpool_repo_cb` above.
unsafe extern "C" fn pkgdb_cb(
    _xh: *mut xbps_sys::xbps_handle,
    obj: xbps_sys::xbps_object_t,
    _key: *const c_char,
    arg: *mut c_void,
    _done: *mut bool,
) -> c_int {
    let ht = &mut *(arg as *mut HashMap<String, Package>);
    let dict = obj as xbps_sys::xbps_dictionary_t;

    let pkgname = match dict_str(dict, "pkgname") {
        Some(s) => s,
        None => return 0,
    };
    let pkgver = dict_str(dict, "pkgver");
    let ver = pkgver
        .as_deref()
        .map(|pv| extract_version(pv, &pkgname).to_string())
        .or_else(|| pkgver.clone())
        .unwrap_or_default();

    if !ht.contains_key(&pkgname) {
        // Orphan: installed but not in any configured repo. Its pkgdb
        // entry is a copy of the repodata dict captured at install
        // time, so "tags" may still be present.
        let tags = dict_str_or_array_joined(dict, "tags").unwrap_or_default();
        let short_desc = dict_str(dict, "short_desc").unwrap_or_default();
        ht.insert(
            pkgname.clone(),
            Package {
                name: pkgname.clone(),
                short_desc,
                tags,
                maintainer: String::new(),
                mark: PkgMark::None,
                ..Default::default()
            },
        );
    }

    let p = ht.get_mut(&pkgname).unwrap();
    p.version_installed = Some(ver.clone());
    // The pkgdb entry's own "repository" property is what this
    // installation actually came from — more authoritative than
    // whichever currently-configured repo happened to also carry a
    // matching pkgver, so it wins when present.
    if let Some(repo) = dict_str(dict, "repository") {
        p.repository = Some(repo);
    }

    let hold = dict_str(dict, "hold");
    if hold.as_deref() == Some("yes") {
        p.state = PkgState::OnHold;
        return 0;
    }

    if let Some(avail) = p.version_available.clone() {
        if ver != avail {
            let cver = cstr(&ver);
            let cavail = cstr(&avail);
            let cmp = xbps_sys::xbps_cmpver(cver.as_ptr(), cavail.as_ptr());
            p.state = if cmp < 0 {
                PkgState::Upgradable
            } else {
                PkgState::Installed
            };
        } else {
            p.state = PkgState::Installed;
        }
    } else {
        p.state = PkgState::Installed;
    }

    let mut essential: bool = false;
    xbps_sys::xbps_dictionary_get_bool(dict, cstr("essential").as_ptr(), &mut essential);
    p.essential = essential;

    0
}

fn do_reload(xh: &mut xbps_sys::xbps_handle, inited: &mut bool) -> LoadResult {
    unsafe {
        if *inited {
            xbps_sys::xbps_end(xh);
        }
        *xh = std::mem::zeroed();
        let r = xbps_sys::xbps_init(xh);
        if r != 0 {
            *inited = false;
            return LoadResult::Err(format!("xbps_init failed (errno {})", r));
        }
        *inited = true;

        let mut ht: HashMap<String, Package> = HashMap::new();
        let ht_ptr = &mut ht as *mut HashMap<String, Package> as *mut c_void;

        xbps_sys::xbps_rpool_foreach(xh, Some(rpool_repo_cb), ht_ptr);
        xbps_sys::xbps_pkgdb_foreach_cb_multi(xh, Some(pkgdb_cb), ht_ptr);

        LoadResult::Ok(ht.into_values().collect())
    }
}

fn get_deps(xh: &mut xbps_sys::xbps_handle, inited: bool, pkgname: &str) -> Option<Vec<String>> {
    if !inited || pkgname.is_empty() {
        return None;
    }
    unsafe {
        let cname = cstr(pkgname);
        let mut d = xbps_sys::xbps_pkgdb_get_pkg(xh, cname.as_ptr());
        if d.is_null() {
            d = xbps_sys::xbps_rpool_get_pkg(xh, cname.as_ptr());
        }
        if d.is_null() {
            return None;
        }
        let deps = xbps_sys::xbps_dictionary_get(d, cstr("run_depends").as_ptr())
            as xbps_sys::xbps_array_t;
        if deps.is_null() {
            return None;
        }
        let n = xbps_sys::xbps_array_count(deps);
        if n == 0 {
            return None;
        }
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let mut s: *const c_char = std::ptr::null();
            xbps_sys::xbps_array_get_cstring_nocopy(deps, i, &mut s);
            out.push(if s.is_null() {
                String::new()
            } else {
                CStr::from_ptr(s).to_string_lossy().into_owned()
            });
        }
        Some(out)
    }
}

fn get_rdeps(xh: &mut xbps_sys::xbps_handle, inited: bool, pkgname: &str) -> Option<Vec<String>> {
    if !inited || pkgname.is_empty() {
        return None;
    }
    unsafe {
        let cname = cstr(pkgname);
        let rdeps = xbps_sys::xbps_pkgdb_get_pkg_revdeps(xh, cname.as_ptr());
        if rdeps.is_null() {
            return None;
        }
        let n = xbps_sys::xbps_array_count(rdeps);
        if n == 0 {
            return None;
        }
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let mut s: *const c_char = std::ptr::null();
            xbps_sys::xbps_array_get_cstring_nocopy(rdeps, i, &mut s);
            out.push(if s.is_null() {
                String::new()
            } else {
                CStr::from_ptr(s).to_string_lossy().into_owned()
            });
        }
        Some(out)
    }
}

fn get_files(xh: &mut xbps_sys::xbps_handle, inited: bool, pkgname: &str) -> Option<Vec<String>> {
    if !inited || pkgname.is_empty() {
        return None;
    }
    unsafe {
        let cname = cstr(pkgname);
        let fd = xbps_sys::xbps_pkgdb_get_pkg_files(xh, cname.as_ptr());
        if fd.is_null() {
            return None;
        }
        let mut out = Vec::new();

        let files =
            xbps_sys::xbps_dictionary_get(fd, cstr("files").as_ptr()) as xbps_sys::xbps_array_t;
        if !files.is_null() {
            let n = xbps_sys::xbps_array_count(files);
            for i in 0..n {
                let e = xbps_sys::xbps_array_get(files, i) as xbps_sys::xbps_dictionary_t;
                if let Some(f) = dict_str(e, "file") {
                    out.push(f);
                }
            }
        }
        let links =
            xbps_sys::xbps_dictionary_get(fd, cstr("links").as_ptr()) as xbps_sys::xbps_array_t;
        if !links.is_null() {
            let n = xbps_sys::xbps_array_count(links);
            for i in 0..n {
                let e = xbps_sys::xbps_array_get(links, i) as xbps_sys::xbps_dictionary_t;
                if let Some(f) = dict_str(e, "file") {
                    let t = dict_str(e, "target").unwrap_or_else(|| "?".to_string());
                    out.push(format!("{} -> {}", f, t));
                }
            }
        }
        let dirs =
            xbps_sys::xbps_dictionary_get(fd, cstr("dirs").as_ptr()) as xbps_sys::xbps_array_t;
        if !dirs.is_null() {
            let n = xbps_sys::xbps_array_count(dirs);
            for i in 0..n {
                let e = xbps_sys::xbps_array_get(dirs, i) as xbps_sys::xbps_dictionary_t;
                if let Some(f) = dict_str(e, "file") {
                    out.push(format!("{}/", f));
                }
            }
        }

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

/// Extended metadata not loaded during the bulk scan — looked up on
/// demand for the currently-selected package only. Property names
/// confirmed against xbps's own zsh-completion property list, same as
/// the original. "install-date"/"automatic-install" only exist on
/// entries from the local pkgdb (installed packages).
fn get_extra_info(
    xh: &mut xbps_sys::xbps_handle,
    inited: bool,
    pkgname: &str,
) -> Option<PackageExtraInfo> {
    if !inited || pkgname.is_empty() {
        return None;
    }
    unsafe {
        let cname = cstr(pkgname);
        let mut d = xbps_sys::xbps_pkgdb_get_pkg(xh, cname.as_ptr());
        let installed = !d.is_null();
        if d.is_null() {
            d = xbps_sys::xbps_rpool_get_pkg(xh, cname.as_ptr());
        }
        if d.is_null() {
            return None;
        }

        let homepage = dict_str(d, "homepage");
        let license = dict_str(d, "license");
        let repository = dict_str(d, "repository");

        let mut install_date = None;
        let mut has_automatic_install = false;
        let mut automatic_install = false;
        if installed {
            install_date = dict_str(d, "install-date");
            let mut v = false;
            has_automatic_install =
                xbps_sys::xbps_dictionary_get_bool(d, cstr("automatic-install").as_ptr(), &mut v);
            automatic_install = v;
        }

        let mut download_size: u64 = 0;
        xbps_sys::xbps_dictionary_get_uint64(d, cstr("filename-size").as_ptr(), &mut download_size);

        Some(PackageExtraInfo {
            homepage,
            license,
            repository,
            install_date,
            automatic_install,
            has_automatic_install,
            download_size,
        })
    }
}

/// Turns one run_depends entry (an xbps "pkgpattern" like "foo>=1.2_1",
/// or occasionally just a bare "foo") into the plain package name.
fn bare_pkgname_from_dep(dep: &str) -> String {
    unsafe {
        let cdep = cstr(dep);
        let mut buf = [0 as c_char; 256];
        // NOTE: `size_t` conventionally binds to Rust `usize` via
        // bindgen; adjust this cast if the generated signature in your
        // `bindings.rs` uses a fixed-width integer instead.
        let ok = xbps_sys::xbps_pkgpattern_name(buf.as_mut_ptr(), buf.len(), cdep.as_ptr());
        if ok {
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        } else {
            dep.to_string()
        }
    }
}

/// Fetches pkgname's own run_depends and, for each dependency not
/// already satisfied (per `by_name`), adds it to `missing` and
/// recurses into that dependency's own deps too. Mirrors
/// `process_deps_of` in the original.
fn process_deps_of(
    xh: &mut xbps_sys::xbps_handle,
    inited: bool,
    by_name: &HashMap<String, PkgState>,
    pkgname: &str,
    visited: &mut HashSet<String>,
    missing: &mut Vec<String>,
) {
    let Some(deps) = get_deps(xh, inited, pkgname) else {
        return;
    };
    for dep in deps {
        let dep_name = bare_pkgname_from_dep(&dep);
        if visited.contains(&dep_name) {
            continue;
        }
        let already_installed = matches!(
            by_name.get(&dep_name),
            Some(PkgState::Installed) | Some(PkgState::Upgradable)
        );
        visited.insert(dep_name.clone());
        if !already_installed {
            missing.push(dep_name.clone());
            process_deps_of(xh, inited, by_name, &dep_name, visited, missing);
        }
    }
}

fn get_missing_deps(
    xh: &mut xbps_sys::xbps_handle,
    inited: bool,
    pkgname: &str,
    by_name: &HashMap<String, PkgState>,
) -> Option<Vec<String>> {
    if pkgname.is_empty() {
        return None;
    }
    let mut visited = HashSet::new();
    visited.insert(pkgname.to_string()); // never report itself, even via a cycle
    let mut missing = Vec::new();
    process_deps_of(xh, inited, by_name, pkgname, &mut visited, &mut missing);
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}
