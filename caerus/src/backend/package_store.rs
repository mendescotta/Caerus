//! `PackageStore` — loads the full package list (repository + installed)
//! via direct `libxbps` calls and exposes it as a `gio::ListStore`.
//!
//! Exactly one dedicated OS thread (`worker_main` below) ever touches
//! `libxbps` or holds an `xbps_handle`, for the entire process lifetime;
//! everything else talks to it via an `mpsc::Sender<Cmd>`. `libxbps`
//! does not tolerate concurrent/re-entrant `xbps_init`/`xbps_end` calls,
//! so this single-thread-owns-the-handle design is load-bearing, not
//! just a style choice.
//!
//! `load_async()` is fire-and-forget, polled off a small main-loop
//! timer; the per-package detail getters (`get_deps` etc.) also poll a
//! reply channel rather than blocking, so a slow worker never freezes
//! the UI thread.

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
    GetRdepsTransitiveMany(Vec<String>, mpsc::Sender<Option<Vec<(String, String)>>>),
    PreviewTransaction(
        Vec<PreviewOp>,
        mpsc::Sender<Result<TransactionPreview, TransactionError>>,
    ),
    Shutdown,
}

/// Downcasts `list.item(i)` to a `PackageObject`, logging and returning
/// `None` if the item is missing or of the wrong type.
pub fn package_obj_at(list: &gio::ListStore, i: u32) -> Option<PackageObject> {
    let obj = list.item(i)?;
    match obj.downcast::<PackageObject>() {
        Ok(po) => Some(po),
        Err(_) => {
            eprintln!("caerus: expected PackageObject in ListStore at index {}", i);
            None
        }
    }
}

/// Whether a carried-over mark still makes sense given the package's
/// freshly-reloaded state (e.g. a stale `Remove` mark on a package
/// that's no longer installed).
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

/// Cheaply-`Clone`able handle (an `Rc` around the shared state).
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

        // Only plain `Send` data crosses the thread boundary; it's applied
        // to the `gio::ListStore` exclusively from this main-thread closure.
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

                            // Carry pending marks over by pkgname so a reload
                            // doesn't silently drop them.
                            let mut old_marks: HashMap<String, PkgMark> = HashMap::new();
                            let old_n = inner.list.n_items();
                            for i in 0..old_n {
                                if let Some(obj_ref) = package_obj_at(&inner.list, i) {
                                    let p = obj_ref.pkg();
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

        Self { inner }
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

    /// Kicks off a background reload; a request while one is already in
    /// flight is dropped, since that load will deliver current data anyway.
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
            if let Some(obj) = package_obj_at(&self.inner.list, i) {
                f(&obj);
            }
        }
    }

    /// Counts every installed package (any state but `NotInstalled`).
    /// Must match `PackageList::visible_counts`'s definition.
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
    /// Current (state, mark) for a single package by name, if present.
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

    /// (state, mark) for every package, keyed by name — bulk counterpart
    /// to `state_and_mark`, one scan instead of one per name.
    pub fn state_and_mark_snapshot(&self) -> HashMap<String, (PkgState, PkgMark)> {
        let mut out = HashMap::new();
        self.for_each(|o| {
            let p = o.pkg();
            out.insert(p.name.clone(), (p.state, p.mark));
        });
        out
    }

    /// Live `PackageObject` handles for every package, keyed by name.
    /// Cloning a `PackageObject` is a cheap ref-count bump, not a deep copy.
    pub fn snapshot_objects(&self) -> HashMap<String, PackageObject> {
        let mut out = HashMap::new();
        self.for_each(|o| {
            out.insert(o.name(), o.clone());
        });
        out
    }

    /// Names of every package currently `Upgradable`, regardless of mark —
    /// local approximation of what `xbps-install -Su` would touch.
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

    /// A name with no matching store entry can be a virtual/`provides`-based
    /// dependency (e.g. "awk") rather than a real package; logs instead of
    /// silently no-oping so that case is distinguishable from a real bug.
    pub fn set_mark(&self, pkgname: &str, mark: PkgMark) {
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj_ref) = package_obj_at(&self.inner.list, i) {
                if obj_ref.name() == pkgname {
                    // GtkColumnView's model chain compares item(i) pointer
                    // identity to decide whether to rebind a row, so an
                    // in-place mutation wouldn't trigger a visible refresh.
                    let mut pkg = obj_ref.pkg().clone();
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

    /// Same effect as `set_mark` per name in `pkgnames`, but one pass
    /// over the list instead of one scan per name.
    pub fn set_marks(&self, pkgnames: &std::collections::HashSet<String>, mark: PkgMark) {
        if pkgnames.is_empty() {
            return;
        }
        let n = self.inner.list.n_items();
        for i in 0..n {
            if let Some(obj_ref) = package_obj_at(&self.inner.list, i) {
                if pkgnames.contains(&obj_ref.name()) {
                    let mut pkg = obj_ref.pkg().clone();
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
            if let Some(obj_ref) = package_obj_at(&self.inner.list, i) {
                if obj_ref.pkg().mark != PkgMark::None {
                    let mut pkg = obj_ref.pkg().clone();
                    pkg.mark = PkgMark::None;
                    self.inner.list.splice(i, 1, &[PackageObject::new(pkg)]);
                }
            }
        }
    }

    // ── Asynchronous per-package detail queries ─────────────────────
    // Each query polls its reply channel from a main-loop timeout instead
    // of blocking, since it shares the worker's strictly-sequential
    // channel with `Cmd::Reload`.

    /// Sends `cmd` to the worker and polls for the reply on the GTK main
    /// loop, invoking `on_reply` exactly once — with `None` if the worker
    /// thread is gone.
    fn request<T: Send + 'static>(
        &self,
        make_cmd: impl FnOnce(mpsc::Sender<T>) -> Cmd,
        on_reply: impl FnOnce(Option<T>) + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        if self.inner.cmd_tx.send(make_cmd(tx)).is_err() {
            on_reply(None);
            return;
        }
        let cb = Cell::new(Some(Box::new(on_reply) as Box<dyn FnOnce(Option<T>)>));
        glib::source::timeout_add_local(Duration::from_millis(15), move || match rx.try_recv() {
            Ok(v) => {
                if let Some(f) = cb.take() {
                    f(Some(v));
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(f) = cb.take() {
                    f(None);
                }
                glib::ControlFlow::Break
            }
        });
    }

    pub fn get_deps_async(&self, pkgname: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let name = pkgname.to_string();
        self.request(|tx| Cmd::GetDeps(name, tx), move |r| f(r.flatten()));
    }

    pub fn get_rdeps_async(&self, pkgname: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let name = pkgname.to_string();
        self.request(|tx| Cmd::GetRdeps(name, tx), move |r| f(r.flatten()));
    }

    pub fn get_files_async(&self, pkgname: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let name = pkgname.to_string();
        self.request(|tx| Cmd::GetFiles(name, tx), move |r| f(r.flatten()));
    }

    pub fn get_extra_info_async(
        &self,
        pkgname: &str,
        f: impl FnOnce(Option<PackageExtraInfo>) + 'static,
    ) {
        let name = pkgname.to_string();
        self.request(|tx| Cmd::GetExtraInfo(name, tx), move |r| f(r.flatten()));
    }

    /// Resolves `pkgname`'s full `run_depends` closure (transitive,
    /// cycle-safe) and reports the subset not currently installed. Builds
    /// a name -> `PkgState` snapshot first so the worker thread never
    /// touches GTK objects.
    pub fn get_missing_deps_async(
        &self,
        pkgname: &str,
        f: impl FnOnce(Option<Vec<String>>) + 'static,
    ) {
        let mut snapshot = HashMap::new();
        self.for_each(|o| {
            let p = o.pkg();
            snapshot.insert(p.name.clone(), p.state);
        });
        let name = pkgname.to_string();
        self.request(
            |tx| Cmd::GetMissingDeps(name, snapshot, tx),
            move |r| f(r.flatten()),
        );
    }

    /// Full transitive closure of `pkgname`'s reverse dependencies. Each
    /// entry is `(affected_pkgname, direct_parent_that_pulled_it_in)` so
    /// the UI can show why a transitively-reached package is affected.
    /// Single-root wrapper over `get_rdeps_transitive_many_async`.
    pub fn get_rdeps_transitive_async(
        &self,
        pkgname: &str,
        f: impl FnOnce(Option<Vec<(String, String)>>) + 'static,
    ) {
        self.get_rdeps_transitive_many_async(vec![pkgname.to_string()], f);
    }

    /// Multi-root version of `get_rdeps_transitive_async`: every
    /// currently-installed package that would break if all of `pkgnames`
    /// were removed together, in one BFS. Roots are seeded into the
    /// visited set together so a root never self-reports as affected.
    pub fn get_rdeps_transitive_many_async(
        &self,
        pkgnames: Vec<String>,
        f: impl FnOnce(Option<Vec<(String, String)>>) + 'static,
    ) {
        self.request(
            |tx| Cmd::GetRdepsTransitiveMany(pkgnames, tx),
            move |r| f(r.flatten()),
        );
    }

    /// Runs a real `libxbps` dry-run (`xbps_transaction_prepare()`
    /// without `xbps_transaction_commit()`) so nothing on disk changes.
    /// `f` receives `None` only if the worker thread is unreachable.
    pub fn preview_transaction_async(
        &self,
        ops: Vec<PreviewOp>,
        f: impl FnOnce(Option<Result<TransactionPreview, TransactionError>>) + 'static,
    ) {
        self.request(|tx| Cmd::PreviewTransaction(ops, tx), f);
    }
}

/// Compares two xbps version strings with `libxbps`'s own comparator
/// (plain string ordering gets e.g. "1.10" vs "1.9" backwards).
/// `xbps_cmpver` takes no `xbps_handle`, so calling it from the main
/// thread doesn't violate the one-thread-owns-the-handle invariant.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let ca = cstr(a);
    let cb = cstr(b);
    unsafe { xbps_sys::xbps_cmpver(ca.as_ptr(), cb.as_ptr()) }.cmp(&0)
}

// ── Worker thread ────────────────────────────────────────────────────
// Everything below runs exclusively on the dedicated xbps worker thread;
// `xh` never leaves this function's stack frame.

fn worker_main(cmd_rx: mpsc::Receiver<Cmd>, result_tx: mpsc::Sender<LoadResult>) {
    // SAFETY: zero-init is a valid starting bit-pattern for this
    // plain-data C struct, required before the first `xbps_init`.
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
            Cmd::GetRdepsTransitiveMany(names, reply) => {
                let _ = reply.send(get_rdeps_transitive_many(&mut xh, inited, &names));
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
    // Falls back to empty rather than panicking on a stray NUL byte.
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
/// `HashMap<String, Package>` on `do_reload`'s stack; safe since the
/// call is synchronous and single-threaded.
///
/// Must use `xbps_dictionary_all_keys()` + `xbps_dictionary_keysym_cstring_nocopy`
/// to enumerate — `xbps_dictionary_iterator()` doesn't reliably enumerate
/// on the target libxbps build, and `xbps_string_cstring_nocopy` on a
/// keysym silently returns NULL.
unsafe extern "C" fn rpool_repo_cb(
    repo: *mut xbps_sys::xbps_repo,
    arg: *mut c_void,
    _done: *mut bool,
) -> c_int {
    if repo.is_null() {
        return 0;
    }
    let ht = &mut *arg.cast::<HashMap<String, Package>>();
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
        // Property is "tags", not "categories"; may be string-or-array.
        let tags = dict_str_or_array_joined(pkgd, "tags").unwrap_or_default();
        let arch = dict_str(pkgd, "architecture");

        let ver = pkgver
            .as_deref()
            .map(|pv| extract_version(pv, &pkgname).to_string());

        let mut isize_: u64 = 0;
        xbps_sys::xbps_dictionary_get_uint64(pkgd, cstr("installed_size").as_ptr(), &mut isize_);
        // Download size is stored as "filename-size", not "download_size".
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

/// Callback for `xbps_pkgdb_foreach_cb_multi`. Same safety as `rpool_repo_cb`.
unsafe extern "C" fn pkgdb_cb(
    _xh: *mut xbps_sys::xbps_handle,
    obj: xbps_sys::xbps_object_t,
    _key: *const c_char,
    arg: *mut c_void,
    _done: *mut bool,
) -> c_int {
    let ht = &mut *arg.cast::<HashMap<String, Package>>();
    let dict = obj as xbps_sys::xbps_dictionary_t;

    let Some(pkgname) = dict_str(dict, "pkgname") else {
        return 0;
    };
    let pkgver = dict_str(dict, "pkgver");
    let ver = pkgver
        .as_deref()
        .map(|pv| extract_version(pv, &pkgname).to_string())
        .or_else(|| pkgver.clone())
        .unwrap_or_default();

    if !ht.contains_key(&pkgname) {
        // Orphan: installed but not in any configured repo.
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

// AUTOFIX: Consider replacing `.unwrap()` with `match ... { Some(x) => x, None => { eprintln!(\"...\"); return; } }` or `if let Some(x) = ...` depending on context. Found: `let p = ht.get_mut(&pkgname).unwrap();`

    let p = match ht.get_mut(&pkgname) {
        Some(p) => p,
        None => { eprintln!("caerus: expected package {} in hash table", pkgname); continue; }
    };
    p.version_installed = Some(ver.clone());
    // pkgdb's own "repository" is more authoritative than a
    // currently-configured repo that happens to carry a matching pkgver.
    if let Some(repo) = dict_str(dict, "repository") {
        p.repository = Some(repo);
    }

    // Must read before the hold early-return so a held essential package
    // still keeps its cannot-be-removed guard.
    p.is_repolocked = dict_str(dict, "repolock").as_deref() == Some("yes");

    let mut essential: bool = false;
    xbps_sys::xbps_dictionary_get_bool(dict, cstr("essential").as_ptr(), &mut essential);
    p.essential = essential;

    // Same precedence as "repository" above.
    if let Some(arch) = dict_str(dict, "architecture") {
        p.arch = Some(arch);
    }

    let hold = dict_str(dict, "hold");
    if hold.as_deref() == Some("yes") {
        p.state = PkgState::OnHold;
        return 0;
    }

    if let Some(avail) = p.version_available.clone() {
        if ver == avail {
            p.state = PkgState::Installed;
        } else {
            let cver = cstr(&ver);
            let cavail = cstr(&avail);
            let cmp = xbps_sys::xbps_cmpver(cver.as_ptr(), cavail.as_ptr());
            p.state = if cmp < 0 {
                PkgState::Upgradable
            } else {
                PkgState::Installed
            };
        }
    } else {
        p.state = PkgState::Installed;
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
            return LoadResult::Err(format!("xbps_init failed (errno {r})"));
        }
        *inited = true;

        let mut ht: HashMap<String, Package> = HashMap::new();
        let ht_ptr = (&mut ht as *mut HashMap<String, Package>).cast::<c_void>();

        // Non-zero here isn't necessarily fatal (e.g. one repo failed to
        // open); `ht` is still used, but log it rather than silently
        // returning a shorter list.
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

        // Same orphan set `xbps-remove -o` would act on. `orphans` param
        // left null: current-state orphans, not hypothetical ones.
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
            // `run_depends` entries are pkgpatterns (e.g. `foo>=1.2_1`);
            // strip to a bare name for exact matching.
            out.push(if s.is_null() {
                String::new()
            } else {
                bare_pkgname_from_dep(&CStr::from_ptr(s).to_string_lossy())
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
            // Unlike `run_depends`, revdeps entries are full pkgver
            // strings (e.g. `foo-1.2.3_1`), not pkgpatterns.
            out.push(if s.is_null() {
                String::new()
            } else {
                bare_pkgname_from_pkgver(&CStr::from_ptr(s).to_string_lossy())
            });
        }
        Some(out)
    }
}

/// Walks the reverse-dependency closure of every name in `pkgnames`
/// breadth-first, recording which direct parent pulled each newly-
/// discovered name in. Roots are seeded into `visited` together, so a
/// root reachable from another root never reports itself as affected.
fn get_rdeps_transitive_many(
    xh: &mut xbps_sys::xbps_handle,
    inited: bool,
    pkgnames: &[String],
) -> Option<Vec<(String, String)>> {
    if !inited || pkgnames.is_empty() {
        return None;
    }
    let mut visited: HashSet<String> = pkgnames.iter().cloned().collect();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut frontier: Vec<String> = pkgnames.to_vec();

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
                    out.push(format!("{f} -> {t}"));
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
                    out.push(format!("{f}/"));
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
/// demand for the selected package. "install-date"/"automatic-install"
/// only exist on entries from the local pkgdb (installed packages).
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

/// Turns one `run_depends` entry (a pkgpattern like `foo>=1.2_1`, or
/// occasionally a bare "foo") into the plain package name.
fn bare_pkgname_from_dep(dep: &str) -> String {
    unsafe {
        let cdep = cstr(dep);
        let mut buf = [0 as c_char; 256];
        let ok = xbps_sys::xbps_pkgpattern_name(buf.as_mut_ptr(), buf.len(), cdep.as_ptr());
        if ok {
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        } else {
            dep.to_string()
        }
    }
}

/// Turns one revdeps entry (a full pkgver like `foo-1.2.3_1`) into the
/// plain package name — pkgver counterpart to `bare_pkgname_from_dep`.
fn bare_pkgname_from_pkgver(pkgver: &str) -> String {
    unsafe {
        let c = cstr(pkgver);
        let mut buf = [0 as c_char; 256];
        let ok = xbps_sys::xbps_pkg_name(buf.as_mut_ptr(), buf.len(), c.as_ptr());
        if ok {
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        } else {
            pkgver.to_string()
        }
    }
}

/// Fetches pkgname's own `run_depends` and, for each dependency not
/// already satisfied (per `by_name`), adds it to `missing` and
/// recurses into that dependency's own deps too.
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
        // Treating a held dependency as "missing" would silently violate
        // the user's hold via `xbps-install <held-pkg>` upgrading it.
        let already_installed = matches!(
            by_name.get(&dep_name),
            Some(PkgState::Installed | PkgState::Upgradable | PkgState::OnHold | PkgState::Broken)
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
// errno values `xbps_transaction_prepare()` uses to signal failure
// reason; not worth pulling in `libc` for four constants.
const EEXIST: c_int = 17;
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

/// Runs every `op` against a fresh, temporary `xbps_handle` rather than
/// the worker's persistent one: libxbps has no call to reset `xh.transd`
/// after a prepared-but-uncommitted transaction, so reusing the
/// long-lived handle would risk corrupting the next reload. Still runs
/// on the single xbps worker thread, so the two handles are sequential,
/// never concurrent.
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
        // EEXIST can fire harmlessly when `name` was already staged as
        // another op's exact-version-pinned dependency this loop (e.g.
        // updating a base package auto-includes an installed "-devel"
        // sibling); `xbps_transaction_prepare()` still succeeds after,
        // so it must not abort the preview here.
        if code != 0 && code != EEXIST {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_is_numeric_not_lexicographic() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("1.9_1", "1.10_1"), Ordering::Less);
        assert_eq!(compare_versions("1.10_1", "1.9_1"), Ordering::Greater);
        assert_eq!(compare_versions("2.0_1", "2.0_1"), Ordering::Equal);
        assert_eq!(compare_versions("1.0_2", "1.0_10"), Ordering::Less);
    }
}