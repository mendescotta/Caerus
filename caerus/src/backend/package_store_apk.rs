use crate::backend::package::{Package, PackageExtraInfo, PackageObject, PkgMark, PkgState};
use crate::backend::transaction_preview::{PreviewOp, TransactionError, TransactionPreview};
use gio::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

type LoadCb = Box<dyn Fn(&str)>;
type LoadFinishedCb = Box<dyn Fn(u32)>;
type NextLoadCb = RefCell<Option<Box<dyn FnOnce(Result<(), String>)>>>;
#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ApkRecord {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    arch: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    maintainer: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    file_size: u64,
    #[serde(default)]
    installed_size: u64,
    #[serde(default)]
    repositories: Vec<String>,
}
enum ResultMsg {
    Packages(Result<Vec<Package>, String>),
}
enum Cmd {
    Reload,
    Deps(String, mpsc::Sender<Option<Vec<String>>>),
    Rdeps(String, mpsc::Sender<Option<Vec<String>>>),
    Files(String, mpsc::Sender<Option<Vec<String>>>),
    Extra(String, mpsc::Sender<Option<PackageExtraInfo>>),
    Missing(mpsc::Sender<Option<Vec<String>>>),
    RdepsMany(Vec<String>, mpsc::Sender<Option<Vec<(String, String)>>>),
    Preview(
        Vec<PreviewOp>,
        mpsc::Sender<Result<TransactionPreview, TransactionError>>,
    ),
    Stop,
}
struct Inner {
    list: gio::ListStore,
    loading: Cell<bool>,
    tx: mpsc::Sender<Cmd>,
    rx: RefCell<mpsc::Receiver<ResultMsg>>,
    started: RefCell<Vec<Box<dyn Fn()>>>,
    finished: RefCell<Vec<LoadFinishedCb>>,
    errors: RefCell<Vec<LoadCb>>,
    next_load_cb: NextLoadCb,
}
impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}
#[derive(Clone)]
pub struct PackageStore {
    inner: Rc<Inner>,
}

impl PackageStore {
    pub fn new() -> Self {
        let list = gio::ListStore::new::<PackageObject>();
        let (tx, rx) = mpsc::channel();
        let (out, results) = mpsc::channel();
        thread::Builder::new()
            .name("caerus-apk-worker".into())
            .spawn(move || worker(rx, out))
            .expect("failed to start apk worker");
        let inner = Rc::new(Inner {
            list,
            loading: Cell::new(false),
            tx,
            rx: RefCell::new(results),
            started: RefCell::new(Vec::new()),
            finished: RefCell::new(Vec::new()),
            errors: RefCell::new(Vec::new()),
            next_load_cb: RefCell::new(None),
        });
        let weak = Rc::downgrade(&inner);
        glib::source::timeout_add_local(Duration::from_millis(30), move || {
            let Some(i) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            loop {
                let r = i.rx.borrow().try_recv();
                match r {
                    Ok(ResultMsg::Packages(Ok(mut ps))) => {
                        i.loading.set(false);
                        let mut marks = HashMap::new();
                        for x in 0..i.list.n_items() {
                            if let Some(o) = package_obj_at(&i.list, x) {
                                let p = o.pkg();
                                if p.mark != PkgMark::None {
                                    marks.insert(p.name.clone(), p.mark);
                                }
                            }
                        }
                        for p in &mut ps {
                            if let Some(m) = marks.get(&p.name) {
                                if mark_valid(*m, p.state) {
                                    p.mark = *m;
                                }
                            }
                        }
                        let objects = ps.into_iter().map(PackageObject::new).collect::<Vec<_>>();
                        let n = objects.len() as u32;
                        i.list.splice(0, i.list.n_items(), &objects);
                        for cb in i.finished.borrow().iter() {
                            cb(n);
                        }
                        let next = i.next_load_cb.borrow_mut().take();
                        if let Some(cb) = next {
                            cb(Ok(()));
                        }
                    }
                    Ok(ResultMsg::Packages(Err(e))) => {
                        i.loading.set(false);
                        for cb in i.errors.borrow().iter() {
                            cb(&e);
                        }
                        let next = i.next_load_cb.borrow_mut().take();
                        if let Some(cb) = next {
                            cb(Err(e));
                        }
                    }
                    Err(_) => break,
                }
            }
            glib::ControlFlow::Continue
        });
        Self { inner }
    }
    pub fn list(&self) -> gio::ListStore {
        self.inner.list.clone()
    }
    pub fn connect_load_started(&self, f: impl Fn() + 'static) {
        self.inner.started.borrow_mut().push(Box::new(f));
    }
    pub fn connect_load_finished(&self, f: impl Fn(u32) + 'static) {
        self.inner.finished.borrow_mut().push(Box::new(f));
    }
    pub fn connect_load_error(&self, f: impl Fn(&str) + 'static) {
        self.inner.errors.borrow_mut().push(Box::new(f));
    }
    /// Whether a package database reload is in progress.
    pub fn is_loading(&self) -> bool {
        self.inner.loading.get()
    }
    pub fn load_async(&self) {
        if self.inner.loading.replace(true) {
            return;
        }
        for cb in self.inner.started.borrow().iter() {
            cb();
        }
        if self.inner.tx.send(Cmd::Reload).is_err() {
            self.inner.loading.set(false);
            let next = self.inner.next_load_cb.borrow_mut().take();
            if let Some(cb) = next {
                cb(Err("package database worker stopped".to_string()));
            }
        }
    }
    /// Reloads packages and calls `cb` after the list has been replaced or an
    /// error has been reported. An already-running reload fails this request.
    pub fn load_async_with_result(&self, cb: impl FnOnce(Result<(), String>) + 'static) {
        if self.inner.loading.get() {
            cb(Err("package database is already loading".to_string()));
            return;
        }
        *self.inner.next_load_cb.borrow_mut() = Some(Box::new(cb));
        self.load_async();
    }
    fn all(&self) -> Vec<Package> {
        (0..self.inner.list.n_items())
            .filter_map(|i| package_obj_at(&self.inner.list, i).map(|o| o.pkg().clone()))
            .collect()
    }
    pub fn count_installed(&self) -> u32 {
        self.all()
            .iter()
            .filter(|p| p.state != PkgState::NotInstalled)
            .count() as u32
    }
    pub fn count_upgradable(&self) -> u32 {
        self.upgradable_names().len() as u32
    }
    pub fn count_marked(&self) -> u32 {
        self.all()
            .iter()
            .filter(|p| p.mark != PkgMark::None)
            .count() as u32
    }
    pub fn upgradable_names(&self) -> Vec<String> {
        self.all()
            .iter()
            .filter(|p| p.state == PkgState::Upgradable)
            .map(|p| p.name.clone())
            .collect()
    }
    pub fn held_back_names(&self) -> Vec<String> {
        Vec::new()
    }
    pub fn marked_names(&self, m: PkgMark) -> Vec<String> {
        self.all()
            .iter()
            .filter(|p| p.mark == m)
            .map(|p| p.name.clone())
            .collect()
    }
    pub fn state_and_mark(&self, n: &str) -> Option<(PkgState, PkgMark)> {
        self.all()
            .into_iter()
            .find(|p| p.name == n)
            .map(|p| (p.state, p.mark))
    }
    pub fn state_and_mark_snapshot(&self) -> HashMap<String, (PkgState, PkgMark)> {
        self.all()
            .into_iter()
            .map(|p| (p.name, (p.state, p.mark)))
            .collect()
    }
    pub fn snapshot_objects(&self) -> HashMap<String, PackageObject> {
        let mut h = HashMap::new();
        for x in 0..self.inner.list.n_items() {
            if let Some(o) = package_obj_at(&self.inner.list, x) {
                h.insert(o.name(), o);
            }
        }
        h
    }
    pub fn set_mark(&self, n: &str, m: PkgMark) {
        for x in 0..self.inner.list.n_items() {
            if let Some(o) = package_obj_at(&self.inner.list, x) {
                if o.name() == n {
                    let mut p = o.pkg().clone();
                    if mark_valid(m, p.state) {
                        p.mark = m;
                    }
                    self.inner.list.splice(x, 1, &[PackageObject::new(p)]);
                    break;
                }
            }
        }
    }
    pub fn set_marks(&self, names: &HashSet<String>, m: PkgMark) {
        for x in 0..self.inner.list.n_items() {
            if let Some(o) = package_obj_at(&self.inner.list, x) {
                if names.contains(&o.name()) {
                    let mut p = o.pkg().clone();
                    if mark_valid(m, p.state) {
                        p.mark = m;
                    }
                    self.inner.list.splice(x, 1, &[PackageObject::new(p)]);
                }
            }
        }
    }
    pub fn clear_all_marks(&self) {
        for x in 0..self.inner.list.n_items() {
            if let Some(o) = package_obj_at(&self.inner.list, x) {
                let mut p = o.pkg().clone();
                p.mark = PkgMark::None;
                self.inner.list.splice(x, 1, &[PackageObject::new(p)]);
            }
        }
    }
    fn request<T: Send + 'static>(
        &self,
        make: impl FnOnce(mpsc::Sender<Option<T>>) -> Cmd,
        f: impl FnOnce(Option<T>) + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        if self.inner.tx.send(make(tx)).is_err() {
            f(None);
            return;
        }
        let cb = Cell::new(Some(Box::new(f) as Box<dyn FnOnce(Option<T>)>));
        glib::source::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
            Ok(v) => {
                if let Some(f) = cb.take() {
                    f(v);
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                if let Some(f) = cb.take() {
                    f(None);
                }
                glib::ControlFlow::Break
            }
        });
    }
    pub fn get_deps_async(&self, n: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let n = n.to_owned();
        self.request(|r| Cmd::Deps(n, r), f);
    }
    pub fn get_rdeps_async(&self, n: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let n = n.to_owned();
        self.request(|r| Cmd::Rdeps(n, r), f);
    }
    pub fn get_files_async(&self, n: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let n = n.to_owned();
        self.request(|r| Cmd::Files(n, r), f);
    }
    pub fn get_extra_info_async(
        &self,
        n: &str,
        f: impl FnOnce(Option<PackageExtraInfo>) + 'static,
    ) {
        let n = n.to_owned();
        self.request(|r| Cmd::Extra(n, r), f);
    }
    pub fn get_missing_deps_async(&self, n: &str, f: impl FnOnce(Option<Vec<String>>) + 'static) {
        let _ = n;
        self.request(Cmd::Missing, f);
    }
    pub fn get_rdeps_transitive_async(
        &self,
        n: &str,
        f: impl FnOnce(Option<Vec<(String, String)>>) + 'static,
    ) {
        self.get_rdeps_transitive_many_async(vec![n.to_owned()], f);
    }
    pub fn get_rdeps_transitive_many_async(
        &self,
        n: Vec<String>,
        f: impl FnOnce(Option<Vec<(String, String)>>) + 'static,
    ) {
        self.request(|r| Cmd::RdepsMany(n, r), f);
    }
    pub fn preview_transaction_async(
        &self,
        ops: Vec<PreviewOp>,
        f: impl FnOnce(Option<Result<TransactionPreview, TransactionError>>) + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        if self.inner.tx.send(Cmd::Preview(ops, tx)).is_err() {
            f(None);
            return;
        }
        let cb =
            Cell::new(Some(Box::new(f)
                as Box<
                    dyn FnOnce(Option<Result<TransactionPreview, TransactionError>>),
                >));
        glib::source::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
            Ok(v) => {
                if let Some(f) = cb.take() {
                    f(Some(v));
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                if let Some(f) = cb.take() {
                    f(None);
                }
                glib::ControlFlow::Break
            }
        });
    }
}

/// Orders two APK versions the way apk-tools does: numeric components, an
/// optional letter, `_suffix[N]` (`alpha`/`beta`/`pre`/`rc` sort before the
/// release, `cvs`/`svn`/`git`/`hg`/`p` after) and `-rN`. A version apk-tools
/// would reject falls back to plain string order.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (Some(ta), Some(tb)) = (version_tokens(a), version_tokens(b)) else {
        return a.cmp(b);
    };
    let mut ia = ta.iter();
    let mut ib = tb.iter();
    loop {
        let (x, y) = (ia.next(), ib.next());
        match (x, y) {
            (None, None) => return Ordering::Equal,
            (Some(&(kx, vx)), Some(&(ky, vy))) if kx == ky => match vx.cmp(&vy) {
                Ordering::Equal => continue,
                o => return o,
            },
            _ => {
                // A pre-release suffix sorts before whatever the other side has;
                // otherwise the side that ran out, or moved to a later kind of
                // token, is the older one.
                let kx = x.map_or(VersionToken::End, |t| t.0);
                let ky = y.map_or(VersionToken::End, |t| t.0);
                if kx == VersionToken::Suffix && x.is_some_and(|t| t.1 < 0) {
                    return Ordering::Less;
                }
                if ky == VersionToken::Suffix && y.is_some_and(|t| t.1 < 0) {
                    return Ordering::Greater;
                }
                return ky.cmp(&kx);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum VersionToken {
    Digit,
    Letter,
    Suffix,
    SuffixNo,
    Revision,
    End,
}

fn version_tokens(v: &str) -> Option<Vec<(VersionToken, i64)>> {
    fn number(s: &[u8], i: &mut usize) -> Option<i64> {
        let start = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        std::str::from_utf8(&s[start..*i]).ok()?.parse().ok()
    }
    let s = v.as_bytes();
    let mut i = 0;
    let mut out = vec![(VersionToken::Digit, number(s, &mut i)?)];
    while i < s.len() {
        match s[i] {
            b'.' if matches!(out.last(), Some((VersionToken::Digit, _))) => {
                i += 1;
                out.push((VersionToken::Digit, number(s, &mut i)?));
            }
            c @ b'a'..=b'z' if matches!(out.last(), Some((VersionToken::Digit, _))) => {
                i += 1;
                out.push((VersionToken::Letter, i64::from(c)));
            }
            b'_' => {
                i += 1;
                let start = i;
                while i < s.len() && s[i].is_ascii_lowercase() {
                    i += 1;
                }
                let rank = match &s[start..i] {
                    b"alpha" => -4,
                    b"beta" => -3,
                    b"pre" => -2,
                    b"rc" => -1,
                    b"cvs" => 1,
                    b"svn" => 2,
                    b"git" => 3,
                    b"hg" => 4,
                    b"p" => 5,
                    _ => return None,
                };
                out.push((VersionToken::Suffix, rank));
                if i < s.len() && s[i].is_ascii_digit() {
                    out.push((VersionToken::SuffixNo, number(s, &mut i)?));
                }
            }
            b'-' if s.get(i + 1) == Some(&b'r') => {
                i += 2;
                out.push((VersionToken::Revision, number(s, &mut i)?));
                if i != s.len() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

pub fn package_obj_at(l: &gio::ListStore, i: u32) -> Option<PackageObject> {
    l.item(i)?.downcast().ok()
}
fn mark_valid(m: PkgMark, s: PkgState) -> bool {
    match m {
        PkgMark::None => true,
        PkgMark::Install => s == PkgState::NotInstalled,
        PkgMark::Upgrade => s == PkgState::Upgradable,
        PkgMark::Remove => s != PkgState::NotInstalled,
        PkgMark::Purge => !cfg!(feature = "apk") && s != PkgState::NotInstalled,
    }
}
fn run(args: &[&str]) -> Result<String, String> {
    let o = Command::new("apk")
        .args(args)
        .output()
        .map_err(|e| format!("could not run apk: {e}"))?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().into());
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}
fn worker(rx: mpsc::Receiver<Cmd>, out: mpsc::Sender<ResultMsg>) {
    while let Ok(c) = rx.recv() {
        match c {
            Cmd::Reload => {
                let _ = out.send(ResultMsg::Packages(load()));
            }
            Cmd::Deps(n, t) => {
                let _ = t.send(info(&n, "--depends"));
            }
            Cmd::Rdeps(n, t) => {
                let _ = t.send(info(&n, "--rdepends"));
            }
            Cmd::Files(n, t) => {
                let _ = t.send(info(&n, "--contents"));
            }
            Cmd::Extra(n, t) => {
                let _ = t.send(extra(&n));
            }
            Cmd::Missing(t) => {
                // APK's solver handles dependency closure and reports it in
                // the transaction preview; the XBPS-specific checklist does
                // not apply here.
                let _ = t.send(None);
            }
            Cmd::RdepsMany(ns, t) => {
                let mut v = Vec::new();
                for n in ns {
                    if let Some(rows) = info(&n, "--rdepends") {
                        v.extend(rows.into_iter().map(|r| (r, n.clone())));
                    }
                }
                let _ = t.send(Some(v));
            }
            Cmd::Preview(ops, t) => {
                let _ = t.send(preview(&ops));
            }
            Cmd::Stop => break,
        }
    }
}
fn load() -> Result<Vec<Package>, String> {
    const FIELDS: &str = "name,version,description,arch,license,maintainer,url,file-size,installed-size,repositories";
    let all = run(&[
        "query", "--from", "system", "--fields", FIELDS, "--format", "json", "*",
    ])?;
    let ins = run(&[
        "query",
        "--installed",
        "--fields",
        FIELDS,
        "--format",
        "json",
        "*",
    ])?;
    let up = run(&[
        "query",
        "--upgradable",
        "--fields",
        FIELDS,
        "--format",
        "json",
        "*",
    ])?;
    let installed = records(&ins)?
        .into_iter()
        .map(|p| (p.name.clone(), p))
        .collect::<HashMap<_, _>>();
    let ups = records(&up)?
        .into_iter()
        .map(|p| p.name)
        .collect::<HashSet<_>>();
    let mut map = HashMap::new();
    for available in records(&all)? {
        let old = installed.get(&available.name);
        map.insert(
            available.name.clone(),
            Package {
                name: available.name,
                version_installed: old.map(|p| p.version.clone()),
                version_available: Some(available.version),
                short_desc: available.description,
                long_desc: None,
                tags: available.license,
                maintainer: available.maintainer,
                install_size: available.installed_size,
                download_size: available.file_size,
                repository: repository_url(&available.repositories),
                state: if old.is_some() {
                    if ups.contains(old.map(|p| p.name.as_str()).unwrap_or_default()) {
                        PkgState::Upgradable
                    } else {
                        PkgState::Installed
                    }
                } else {
                    PkgState::NotInstalled
                },
                mark: PkgMark::None,
                essential: false,
                arch: Some(available.arch),
                is_orphan: false,
                is_repolocked: false,
            },
        );
    }
    for (n, p) in installed {
        map.entry(n.clone()).or_insert(Package {
            name: n,
            version_installed: Some(p.version),
            version_available: None,
            short_desc: p.description,
            tags: p.license,
            maintainer: p.maintainer,
            download_size: p.file_size,
            repository: repository_url(&p.repositories),
            install_size: p.installed_size,
            state: PkgState::Installed,
            arch: Some(p.arch),
            ..Package::default()
        });
    }
    let mut v = map.into_values().collect::<Vec<_>>();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v)
}
fn records(s: &str) -> Result<Vec<ApkRecord>, String> {
    serde_json::from_str(s).map_err(|error| format!("could not parse APK query JSON: {error}"))
}
fn repository_url(repositories: &[String]) -> Option<String> {
    repositories
        .iter()
        .find(|repo| repo.as_str() != "lib/apk/db/installed")
        .cloned()
}
fn safe(n: &str) -> bool {
    !n.is_empty()
        && !n.starts_with('-')
        && n != "."
        && n != ".."
        && !n.ends_with(".apk")
        && n.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+_.-".contains(&c))
}
fn info(n: &str, flag: &str) -> Option<Vec<String>> {
    if !safe(n) {
        return None;
    }
    if flag == "--depends" {
        let output = run(&[
            "query",
            "--installed",
            "--fields",
            "depends",
            "--format",
            "json",
            n,
        ])
        .or_else(|_| run(&["query", "--fields", "depends", "--format", "json", n]))
        .ok()?;
        let value: serde_json::Value = serde_json::from_str(&output).ok()?;
        return Some(
            value
                .as_array()?
                .iter()
                .filter_map(|record| record.get("depends")?.as_array())
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    let out = run(&["info", flag, n]).ok()?;
    Some(
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.ends_with(':'))
            .map(str::to_owned)
            .collect(),
    )
}
fn extra(n: &str) -> Option<PackageExtraInfo> {
    if !safe(n) {
        return None;
    }
    let mut x = PackageExtraInfo::default();
    let query = run(&[
        "query",
        "--fields",
        "name,version,url,license,file-size,repositories",
        "--format",
        "json",
        n,
    ])
    .ok()?;
    let record = records(&query).ok()?.into_iter().next()?;
    x.homepage = Some(record.url);
    x.license = Some(record.license);
    x.repository = record
        .repositories
        .into_iter()
        .find(|repo| repo != "lib/apk/db/installed");
    x.download_size = record.file_size;
    let installed = run(&[
        "query",
        "--installed",
        "--fields",
        "name,version",
        "--format",
        "json",
        n,
    ])
    .ok()
    .and_then(|json| records(&json).ok())
    .is_some_and(|rows| rows.iter().any(|row| row.name == n));
    if installed {
        if let Ok(world) = std::fs::read_to_string("/etc/apk/world") {
            x.apk_world_root = Some(world_names(&world).contains(n));
        }
    }
    if let Some(provides) = info(n, "--provides") {
        x.provides = provides;
    }
    if let Some(replaces) = info(n, "--replaces") {
        x.replaces = replaces;
    }
    Some(x)
}

fn preview(ops: &[PreviewOp]) -> Result<TransactionPreview, TransactionError> {
    use crate::backend::transaction_preview::{TransAction, TransactionPreviewItem};
    if ops.is_empty() {
        return Ok(TransactionPreview::default());
    }
    let mut kind = None;
    let mut names = Vec::new();
    let mut full_upgrade = false;
    for op in ops {
        let (verb, name) = match op {
            PreviewOp::Install(name) => ("add", name),
            PreviewOp::Update(name) => ("upgrade", name),
            PreviewOp::Remove(name) => ("del", name),
            PreviewOp::Purge(name) => {
                return Err(TransactionError::Other(format!(
                    "APK does not provide Void-style recursive purge ({name})"
                )));
            }
            #[cfg(feature = "apk")]
            PreviewOp::FullUpgrade => {
                if ops.len() != 1 {
                    return Err(TransactionError::Other(
                        "A full APK upgrade cannot be combined with other preview operations"
                            .into(),
                    ));
                }
                full_upgrade = true;
                kind = Some("upgrade");
                continue;
            }
        };
        if !safe(name) {
            return Err(TransactionError::Other("Invalid package name".into()));
        }
        if kind.is_some_and(|previous| previous != verb) {
            return Err(TransactionError::Other(
                "APK preview cannot safely combine installation, upgrade, and removal requests; apply one operation type at a time".into(),
            ));
        }
        kind = Some(verb);
        names.push(name.as_str());
    }
    if names.is_empty() && !full_upgrade {
        return Ok(TransactionPreview::default());
    }
    names.sort_unstable();
    names.dedup();
    let verb = kind.ok_or_else(|| TransactionError::Other("empty APK preview".into()))?;
    let mut command = Command::new("apk");
    command
        .env("LC_ALL", "C")
        .args([verb, "--simulate", "--no-progress"]);
    if !full_upgrade {
        command.arg("--");
    }
    command.args(&names);
    let output = command
        .output()
        .map_err(|error| TransactionError::Other(format!("could not run apk: {error}")))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(TransactionError::Other(text.trim().to_owned()));
    }
    let plan = parse_simulation(&text).ok_or_else(|| {
        TransactionError::Other(format!(
            "APK simulation returned an unrecognized transaction plan; refusing an incomplete preview. Output: {}",
            text.trim()
        ))
    })?;
    let mut p = TransactionPreview::default();
    for (name, version, action) in plan {
        let query = run(&[
            "query",
            "--fields",
            "name,version,file-size,installed-size",
            "--format",
            "json",
            &name,
        ]);
        let record = query.ok().and_then(|json| {
            records(&json)
                .ok()?
                .into_iter()
                .find(|row| row.name == name && row.version == version)
        });
        let previous_record = run(&[
            "query",
            "--installed",
            "--fields",
            "name,version,installed-size",
            "--format",
            "json",
            &name,
        ])
        .ok()
        .and_then(|json| records(&json).ok()?.into_iter().find(|r| r.name == name));
        let previous_version = previous_record.as_ref().map(|row| row.version.clone());
        let installed_size = if action == TransAction::Remove {
            previous_record.as_ref().map_or(0, |row| row.installed_size)
        } else {
            record.as_ref().map_or(0, |row| row.installed_size)
        };
        let item = TransactionPreviewItem {
            requested: (full_upgrade && action != TransAction::Install)
                || names.contains(&name.as_str()),
            previous_version,
            pkgname: name,
            pkgver: version,
            action,
            installed_size,
            download_size: if action == TransAction::Remove {
                0
            } else {
                record.as_ref().map_or(0, |row| row.file_size)
            },
        };
        match action {
            TransAction::Install => {
                p.install_pkgs += 1;
                p.download_pkgs += 1;
                p.total_download_size = p.total_download_size.saturating_add(item.download_size);
                p.total_installed_size = p.total_installed_size.saturating_add(item.installed_size);
            }
            TransAction::Update | TransAction::Reinstall => {
                p.update_pkgs += 1;
                p.download_pkgs += 1;
                p.total_download_size = p.total_download_size.saturating_add(item.download_size);
                if let (Some(new), Some(old)) = (&record, &previous_record) {
                    p.total_installed_size = p
                        .total_installed_size
                        .saturating_add(new.installed_size.saturating_sub(old.installed_size));
                    p.total_removed_size = p
                        .total_removed_size
                        .saturating_add(old.installed_size.saturating_sub(new.installed_size));
                }
            }
            TransAction::Remove => {
                p.remove_pkgs += 1;
                p.total_removed_size = p.total_removed_size.saturating_add(item.installed_size);
            }
            _ => {}
        }
        if action == TransAction::Remove || record.is_some() {
            p.disk_size_change += crate::backend::transaction_preview::installed_delta(
                action,
                previous_record.as_ref().map(|row| row.installed_size),
                item.installed_size,
            );
        }
        p.items.push(item);
    }
    Ok(p)
}

fn parse_simulation(
    output: &str,
) -> Option<
    Vec<(
        String,
        String,
        crate::backend::transaction_preview::TransAction,
    )>,
> {
    use crate::backend::transaction_preview::TransAction;
    let mut items = Vec::new();
    for line in output.lines().map(str::trim) {
        if !line.starts_with('(') {
            continue;
        }
        let (_, rest) = line.split_once(')')?;
        let (verb, package) = rest.trim().split_once(' ')?;
        let (name, version) = package.split_once(' ')?;
        let action = match verb {
            "Installing" => TransAction::Install,
            "Upgrading" | "Downgrading" => TransAction::Update,
            "Reinstalling" => TransAction::Reinstall,
            "Purging" | "Removing" => TransAction::Remove,
            _ => return None,
        };
        if !safe(name) || !version.starts_with('(') || !version.ends_with(')') {
            return None;
        }
        let version = &version[1..version.len() - 1];
        let version = version.rsplit_once(" -> ").map_or(version, |(_, new)| new);
        items.push((name.to_owned(), version.to_owned(), action));
    }
    if items.is_empty() && (output.contains("OK") || output.contains("No changes")) {
        Some(items)
    } else if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn apk_version_order() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        for (a, b, want) in [
            ("1.0", "1.0", Equal),
            ("1.0-r1", "1.0-r0", Greater),
            ("1.10", "1.9", Greater),
            ("1.0", "1.0.1", Less),
            ("1.0a", "1.0", Greater),
            ("1.0a", "1.0.1", Less),
            ("1.0_rc1", "1.0", Less),
            ("1.0_alpha2", "1.0_beta1", Less),
            ("1.0_p1", "1.0", Greater),
            ("1.0_p1", "1.0-r5", Greater),
            ("26.03-r0", "26.02-r3", Greater),
            ("2.0", "1.99_git20260101", Greater),
        ] {
            assert_eq!(compare_versions(a, b), want, "{a} vs {b}");
            assert_eq!(compare_versions(b, a), want.reverse(), "{b} vs {a}");
        }
    }

    use super::*;
    #[test]
    fn parses_apk_query_json_fields() {
        let r = records(
            r#"[{"name":"7zip","version":"26.03-r0","description":"File archiver with a high compression ratio","arch":"x86_64","license":"LGPL-2.0-only AND BSD-3-Clause AND custom:unrar","url":"https://7-zip.org","installed-size":2908560,"file-size":1278773,"repositories":["https://repo.chimera-linux.org/current/main"]}]"#,
        )
        .unwrap();
        assert_eq!(r[0].name, "7zip");
        assert_eq!(r[0].installed_size, 2_908_560);
        assert_eq!(
            repository_url(&r[0].repositories).as_deref(),
            Some("https://repo.chimera-linux.org/current/main")
        );
    }
    #[test]
    fn serde_parser_handles_escaped_unicode_and_nested_arrays() {
        let r = records(r#"[{"name":"foo","version":"1-r0","description":"say \"héllo\"","repositories":["lib/apk/db/installed","https://repo.example"],"depends":["so:libc.so"]}]"#).unwrap();
        assert_eq!(r[0].description, "say \"héllo\"");
        assert_eq!(
            repository_url(&r[0].repositories).as_deref(),
            Some("https://repo.example")
        );
    }
    #[test]
    fn package_name_allowlist_rejects_switches() {
        assert!(!safe("--help"));
        assert!(!safe("/tmp/foo.apk"));
        assert!(!safe("foo.apk"));
        assert!(safe("foo+bar"));
    }

    #[test]
    fn parses_all_solver_actions_in_apk_output() {
        let output = "(1/2) Installing dep (1.0-r0)\n(2/2) Upgrading app (2.0-r0 -> 3.0-r0)\n(1/3) Purging old (1.0-r0)\n";
        let plan = parse_simulation(output).unwrap();
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].0, "dep");
        assert_eq!(
            plan[1].2,
            crate::backend::transaction_preview::TransAction::Update
        );
        assert_eq!(plan[1].1, "3.0-r0");
        assert_eq!(plan[2].0, "old");
        assert_eq!(
            plan[2].2,
            crate::backend::transaction_preview::TransAction::Remove
        );
    }
}

/// World constraints may include a repository tag or version operator.
fn world_names(world: &str) -> HashSet<String> {
    world
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split(['@', '<', '>', '=', '~', '!', ' ', '\t'])
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .filter(|name| !name.is_empty())
        .collect()
}
#[cfg(test)]
mod world_tests {
    use super::*;
    #[test]
    fn handles_tagged_versioned_and_commented_world_constraints() {
        assert_eq!(
            world_names("# comment\nfoo@testing>=1.2\nbar~2\nbaz=3\n"),
            HashSet::from(["foo".into(), "bar".into(), "baz".into()])
        );
    }
}
