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
use crate::backend::transaction_preview::{
    PreviewOp, TransAction, TransactionError, TransactionPreview, TransactionPreviewItem,
};
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
    GetRdepsTransitive(String, mpsc::Sender<Option<Vec<(String, String)>>>),
    PreviewTransaction(
        Vec<PreviewOp>,
        mpsc::Sender<Result<TransactionPreview, TransactionError>>,
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

    /// Counts every *installed* package, in any of its installed states
    /// (`Installed`/`Upgradable`/`OnHold`/`Broken` — anything but
    /// `NotInstalled`) — matches `PackageList::visible_counts`'s
    /// definition exactly, so the status bar's "N installed" figure
    /// doesn't jump around purely from switching between the
    /// whole-database (this) and currently-visible (that) rendering
    /// path depending on whether a search is active.
    pub fn count_installed(&self) -> u32 {
        let mut c = 0;
        self.for_each(|o| {
            if o.pkg().state != PkgState::NotInstalled {
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

    /// No-ops (silently, previously) if `pkgname` doesn't match any
    /// entry currently in the store — which can genuinely happen for a
    /// dependency name resolved from a *virtual*/`provides`-based
    /// `run_depends` pattern (e.g. depending on "awk", satisfied by
    /// whichever package's `provides` lists it — "awk" itself is never a
    /// real, independently listed package). `deps_confirm` has no way to
    /// tell such a name apart from a normal one before calling this, so
    /// rather than leaving that case as an invisible no-op (which looks
    /// exactly like "accepted the dialog but nothing got marked"), log
    /// it — actionable enough to explain the gap without needing another
    /// libxbps round trip just to pre-filter virtual names out.
    pub fn set_mark(&self, pkgname: &str, mark: PkgMark) {
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj) = self.inner.list.item(i) {
                let obj = obj.downcast_ref::<PackageObject>().unwrap();
                if obj.name() == pkgname {
                    // Mutating `obj` in place and manually firing
                    // `items_changed(i, 1, 1)` is *not* enough to get a
                    // visible refresh here: `GtkColumnView` (through the
                    // `FilterListModel`/`SortListModel`/`MultiSelection`
                    // chain in front of it) compares the `item(i)`
                    // GObject pointer before deciding whether to rebind a
                    // row, and skips the rebind when it's the same
                    // pointer — which it always is when we just mutate
                    // the existing object. Splicing in a genuinely new
                    // `PackageObject` forces that identity check to see a
                    // change, so the checkbox/status-icon/bold-name
                    // bindings (all read `pkg.mark` at bind time) actually
                    // update. Confirmed via temporary instrumentation:
                    // the in-place-mutate + items_changed approach never
                    // re-fired the checkbox column's `bind` callback.
                    let mut pkg = obj.pkg().clone();
                    pkg.mark = mark;
                    self.inner.list.splice(i, 1, &[PackageObject::new(pkg)]);
                    return;
                }
            }
        }
        eprintln!(
            "caerus: set_mark({pkgname:?}, {mark:?}) found no matching package — likely a \
             virtual/provides-based dependency name rather than a real package"
        );
    }

    /// Same effect as calling `set_mark` once per name in `pkgnames`, but
    /// a single O(n) pass over the list instead of one O(n) linear scan
    /// per name — matters for a large multi-select bulk mark against the
    /// full package list (`apply_bulk_mark` in `ui/package_list.rs`).
    pub fn set_marks(&self, pkgnames: &std::collections::HashSet<String>, mark: PkgMark) {
        if pkgnames.is_empty() {
            return;
        }
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj) = self.inner.list.item(i) {
                let obj = obj.downcast_ref::<PackageObject>().unwrap();
                if pkgnames.contains(&obj.name()) {
                    // See the comment in `set_mark` for why this splices
                    // in a new object rather than mutating in place.
                    let mut pkg = obj.pkg().clone();
                    pkg.mark = mark;
                    self.inner.list.splice(i, 1, &[PackageObject::new(pkg)]);
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
                    // See the comment in `set_mark` — splice in a new
                    // object rather than mutate in place, so the row
                    // actually gets rebound.
                    let mut pkg = obj.pkg().clone();
                    pkg.mark = PkgMark::None;
                    self.inner.list.splice(i, 1, &[PackageObject::new(pkg)]);
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

    /// Full transitive closure of `pkgname`'s reverse dependencies —
    /// every currently-installed package that would break if `pkgname`
    /// were removed, directly or through a chain of other removals.
    /// Each entry is `(affected_pkgname, direct_parent_that_pulled_it_in)`
    /// so the UI can show *why* a transitively-reached package is
    /// affected, not just that it is.
    pub fn get_rdeps_transitive(&self, pkgname: &str) -> Option<Vec<(String, String)>> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::GetRdepsTransitive(pkgname.to_string(), tx))
            .ok()?;
        rx.recv().unwrap_or(None)
    }

    /// Runs a real `libxbps` dry-run: `xbps_transaction_*` calls for each
    /// `op` followed by `xbps_transaction_prepare()`, reading back real
    /// sizes/ordering/conflicts from `xh.transd` — but never calling
    /// `xbps_transaction_commit()`, so nothing on disk changes. Blocks
    /// the calling thread briefly, same cost class as the other
    /// synchronous detail queries above.
    pub fn preview_transaction(
        &self,
        ops: Vec<PreviewOp>,
    ) -> Option<Result<TransactionPreview, TransactionError>> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .cmd_tx
            .send(Cmd::PreviewTransaction(ops, tx))
            .ok()?;
        rx.recv().ok()
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
            Cmd::GetRdepsTransitive(name, reply) => {
                let _ = reply.send(get_rdeps_transitive(&mut xh, inited, &name));
            }
            Cmd::PreviewTransaction(ops, reply) => {
                let _ = reply.send(preview_transaction(&ops));
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
        let arch = dict_str(pkgd, "architecture");

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
                arch,
                is_orphan: false,
                is_repolocked: false,
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

    // Read before the hold early-return below so it's still picked up
    // for a package that's simultaneously on hold and repo-locked.
    p.is_repolocked = dict_str(dict, "repolock").as_deref() == Some("yes");

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

    // Same precedence rule as "repository" above: the pkgdb's own
    // recorded architecture (what's actually installed) wins over
    // whatever the currently-configured repo scan happened to set.
    if let Some(arch) = dict_str(dict, "architecture") {
        p.arch = Some(arch);
    }

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

        // Both return 0 on success; our own callbacks always return 0
        // themselves, so a non-zero result here can only mean libxbps
        // hit an internal problem (e.g. `xbps_rpool_foreach`'s own docs:
        // it drops repos that failed to open from the pool and reports
        // that as an error, but keeps going with the rest) — not
        // necessarily fatal to the whole reload, so `ht` is still used
        // below rather than discarded, but it's worth knowing about
        // rather than silently ending up with a shorter list than
        // expected for no visible reason.
        let rpool_rc = xbps_sys::xbps_rpool_foreach(xh, Some(rpool_repo_cb), ht_ptr);
        if rpool_rc != 0 {
            eprintln!(
                "caerus: xbps_rpool_foreach returned {rpool_rc} — one or more \
                 repositories may have failed to load, package list may be incomplete"
            );
        }
        let pkgdb_rc = xbps_sys::xbps_pkgdb_foreach_cb_multi(xh, Some(pkgdb_cb), ht_ptr);
        if pkgdb_rc != 0 {
            eprintln!(
                "caerus: xbps_pkgdb_foreach_cb_multi returned {pkgdb_rc} — installed-package \
                 data may be incomplete"
            );
        }

        // Single cheap pass over the already-loaded pkgdb — same
        // orphan set `xbps-remove -o` (the helper's own ORPHANS command)
        // would act on. `orphans` param left null: we only want the
        // orphans of the system as it stands right now, not "as if these
        // other packages were already removed too".
        let orphans = xbps_sys::xbps_find_pkg_orphans(xh, std::ptr::null_mut());
        if !orphans.is_null() {
            let n = xbps_sys::xbps_array_count(orphans);
            for i in 0..n {
                let d = xbps_sys::xbps_array_get(orphans, i) as xbps_sys::xbps_dictionary_t;
                if let Some(name) = dict_str(d, "pkgname") {
                    if let Some(p) = ht.get_mut(&name) {
                        p.is_orphan = true;
                    }
                }
            }
        }

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

/// Walks `pkgname`'s reverse-dependency closure breadth-first, recording
/// which direct parent pulled each newly-discovered name in. Mirrors the
/// shape of `process_deps_of`/`get_missing_deps` below, just walking
/// `get_rdeps` (reverse deps) instead of `get_deps` (forward deps).
fn get_rdeps_transitive(
    xh: &mut xbps_sys::xbps_handle,
    inited: bool,
    pkgname: &str,
) -> Option<Vec<(String, String)>> {
    if !inited || pkgname.is_empty() {
        return None;
    }
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(pkgname.to_string()); // never report itself, even via a cycle
    let mut out: Vec<(String, String)> = Vec::new();
    let mut frontier = vec![pkgname.to_string()];

    while let Some(current) = frontier.pop() {
        let Some(rdeps) = get_rdeps(xh, inited, &current) else {
            continue;
        };
        for name in rdeps {
            if visited.contains(&name) {
                continue;
            }
            visited.insert(name.clone());
            out.push((name.clone(), current.clone()));
            frontier.push(name);
        }
    }

    if out.is_empty() {
        None
    } else {
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

        let provides = read_string_array(d, "provides");
        let conflicts = read_string_array(d, "conflicts");
        let replaces = read_string_array(d, "replaces");
        let shlib_requires = read_string_array(d, "shlib-requires");
        let shlib_provides = read_string_array(d, "shlib-provides");

        Some(PackageExtraInfo {
            homepage,
            license,
            repository,
            install_date,
            automatic_install,
            has_automatic_install,
            download_size,
            provides,
            conflicts,
            replaces,
            shlib_requires,
            shlib_provides,
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

// ── Real transaction preview (dry-run) ───────────────────────────────
//
// Standard Linux errno values (confirmed against /usr/include/errno.h),
// matching the return codes `xbps_transaction_prepare()` uses to signal
// *why* it failed — see `exec_transaction()` in Void's own
// `bin/xbps-install/transaction.c`, which this mirrors. Not pulled from
// the `libc` crate (not a dependency of this crate) for four constants.
const ENOEXEC: c_int = 8;
const EAGAIN: c_int = 11;
const ENODEV: c_int = 19;
const ENOSPC: c_int = 28;

unsafe fn read_string_array(dict: xbps_sys::xbps_dictionary_t, key: &str) -> Vec<String> {
    let arr = xbps_sys::xbps_dictionary_get(dict, cstr(key).as_ptr()) as xbps_sys::xbps_array_t;
    if arr.is_null() {
        return Vec::new();
    }
    let n = xbps_sys::xbps_array_count(arr);
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut s: *const c_char = std::ptr::null();
        xbps_sys::xbps_array_get_cstring_nocopy(arr, i, &mut s);
        if !s.is_null() {
            out.push(CStr::from_ptr(s).to_string_lossy().into_owned());
        }
    }
    out
}

/// Runs every `op` against a fresh, temporary `xbps_handle` — deliberately
/// *not* the worker's own persistent one, since libxbps has no call to
/// reset `xh.transd`/undo a prepared-but-uncommitted transaction, and
/// leaving the long-lived handle in a transaction-dirty state would risk
/// corrupting the next reload or detail-pane lookup. Still runs entirely
/// on the single dedicated xbps worker thread, so the "exactly one thread
/// touches libxbps" invariant documented at the top of this file holds —
/// the two handles are sequential on that one thread, never touched
/// concurrently from two different threads.
///
/// The two handles briefly coexisting in memory (the persistent one just
/// sits unused while this one is alive) is also safe against libxbps's
/// own locking: per `/usr/include/xbps.h`, `xbps_init()` takes no lock at
/// all — locking is explicit and opt-in via `xbps_pkgdb_lock()` (for a
/// write transaction) or `xbps_repo_lock()` (local repo write access),
/// neither of which this function or `xbps_transaction_prepare()` ever
/// calls, since nothing here writes anything.
fn preview_transaction(ops: &[PreviewOp]) -> Result<TransactionPreview, TransactionError> {
    unsafe {
        let mut xh: xbps_sys::xbps_handle = std::mem::zeroed();
        let r = xbps_sys::xbps_init(&mut xh);
        if r != 0 {
            return Err(TransactionError::Other(format!(
                "xbps_init failed: {}",
                std::io::Error::from_raw_os_error(r)
            )));
        }
        let result = run_preview_ops(&mut xh, ops);
        xbps_sys::xbps_end(&mut xh);
        result
    }
}

unsafe fn run_preview_ops(
    xh: &mut xbps_sys::xbps_handle,
    ops: &[PreviewOp],
) -> Result<TransactionPreview, TransactionError> {
    let mut op_errors = Vec::new();
    for op in ops {
        let (name, code) = match op {
            PreviewOp::Install(name) => (
                name,
                xbps_sys::xbps_transaction_install_pkg(xh, cstr(name).as_ptr(), false),
            ),
            PreviewOp::Update(name) => (
                name,
                xbps_sys::xbps_transaction_update_pkg(xh, cstr(name).as_ptr(), false),
            ),
            PreviewOp::Remove(name) => (
                name,
                xbps_sys::xbps_transaction_remove_pkg(xh, cstr(name).as_ptr(), false),
            ),
            PreviewOp::Purge(name) => (
                name,
                xbps_sys::xbps_transaction_remove_pkg(xh, cstr(name).as_ptr(), true),
            ),
        };
        if code != 0 {
            op_errors.push(format!(
                "{}: {}",
                name,
                std::io::Error::from_raw_os_error(code)
            ));
        }
    }
    if !op_errors.is_empty() {
        return Err(TransactionError::Other(op_errors.join("; ")));
    }

    let r = xbps_sys::xbps_transaction_prepare(xh);
    if r != 0 {
        return Err(match r {
            ENODEV => TransactionError::MissingDeps(read_string_array(xh.transd, "missing_deps")),
            ENOEXEC => {
                TransactionError::MissingShlibs(read_string_array(xh.transd, "missing_shlibs"))
            }
            EAGAIN => TransactionError::Conflicts(read_string_array(xh.transd, "conflicts")),
            ENOSPC => {
                let mut need: u64 = 0;
                let mut free: u64 = 0;
                xbps_sys::xbps_dictionary_get_uint64(
                    xh.transd,
                    cstr("total-installed-size").as_ptr(),
                    &mut need,
                );
                xbps_sys::xbps_dictionary_get_uint64(
                    xh.transd,
                    cstr("disk-free-size").as_ptr(),
                    &mut free,
                );
                TransactionError::NotEnoughSpace { need, free }
            }
            _ => TransactionError::Other(format!("{}", std::io::Error::from_raw_os_error(r))),
        });
    }

    let transd = xh.transd;
    let mut preview = TransactionPreview::default();
    xbps_sys::xbps_dictionary_get_uint64(
        transd,
        cstr("total-download-size").as_ptr(),
        &mut preview.total_download_size,
    );
    xbps_sys::xbps_dictionary_get_uint64(
        transd,
        cstr("total-installed-size").as_ptr(),
        &mut preview.total_installed_size,
    );
    xbps_sys::xbps_dictionary_get_uint64(
        transd,
        cstr("total-removed-size").as_ptr(),
        &mut preview.total_removed_size,
    );
    xbps_sys::xbps_dictionary_get_uint32(
        transd,
        cstr("total-download-pkgs").as_ptr(),
        &mut preview.download_pkgs,
    );
    xbps_sys::xbps_dictionary_get_uint32(
        transd,
        cstr("total-install-pkgs").as_ptr(),
        &mut preview.install_pkgs,
    );
    xbps_sys::xbps_dictionary_get_uint32(
        transd,
        cstr("total-update-pkgs").as_ptr(),
        &mut preview.update_pkgs,
    );
    xbps_sys::xbps_dictionary_get_uint32(
        transd,
        cstr("total-remove-pkgs").as_ptr(),
        &mut preview.remove_pkgs,
    );
    xbps_sys::xbps_dictionary_get_uint32(
        transd,
        cstr("total-hold-pkgs").as_ptr(),
        &mut preview.hold_pkgs,
    );

    let packages =
        xbps_sys::xbps_dictionary_get(transd, cstr("packages").as_ptr()) as xbps_sys::xbps_array_t;
    if !packages.is_null() {
        let n = xbps_sys::xbps_array_count(packages);
        for i in 0..n {
            let pkgd = xbps_sys::xbps_array_get(packages, i) as xbps_sys::xbps_dictionary_t;
            if pkgd.is_null() {
                continue;
            }
            let pkgname = dict_str(pkgd, "pkgname").unwrap_or_default();
            let pkgver = dict_str(pkgd, "pkgver").unwrap_or_default();
            let mut ttype: u8 = 0;
            xbps_sys::xbps_dictionary_get_uint8(pkgd, cstr("transaction").as_ptr(), &mut ttype);
            let mut installed_size: u64 = 0;
            xbps_sys::xbps_dictionary_get_uint64(
                pkgd,
                cstr("installed_size").as_ptr(),
                &mut installed_size,
            );
            let mut download_size: u64 = 0;
            xbps_sys::xbps_dictionary_get_uint64(
                pkgd,
                cstr("filename-size").as_ptr(),
                &mut download_size,
            );
            preview.items.push(TransactionPreviewItem {
                pkgname,
                pkgver,
                action: TransAction::from_raw(ttype),
                arch: dict_str(pkgd, "architecture"),
                repository: dict_str(pkgd, "repository"),
                installed_size,
                download_size,
            });
        }
    }

    Ok(preview)
}
