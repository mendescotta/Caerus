//! `Transaction` — drives `caerus-helper` as a long-lived child process,
//! spawned via `pkexec` directly (the GUI itself is never privileged).
//! Rust translation of backend/transaction.{h,c}.
//!
//! One instance lives for the whole app session: create it once, call
//! `run_async()` each time there's a new batch of commands. The
//! underlying helper process is spawned lazily on first use and then
//! kept alive — repeated batches do NOT re-trigger authentication.
//!
//! The helper is only ever told to exit (QUIT) in two situations:
//!   - it has sat idle (no command in flight, none queued) for
//!     `IDLE_TIMEOUT`, or
//!   - `shutdown()` is called explicitly (app exit).
//! Either way, the *next* call to `run_async()` after that transparently
//! respawns the helper — a fresh `pkexec` prompt — since this is
//! indistinguishable from first use.
//!
//! Callbacks (all invoked on the main thread, mirroring the original's
//! GObject signals):
//!   `connect_log`          (line: &str)              -- every raw line
//!   `connect_finished`     (success)                  -- after the
//!                                                          queued batch
//!                                                          has run
//!   `connect_disconnected` (expected: bool)            -- helper exited

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum TxnState {
    NotRunning,
    Spawning,
    Busy,
    Idle,
}

struct Inner {
    stdin: RefCell<Option<ChildStdin>>,
    line_rx: RefCell<Option<mpsc::Receiver<String>>>,
    pending: RefCell<VecDeque<String>>,
    state: Cell<TxnState>,
    intentional_quit: Cell<bool>,
    idle_timeout_id: RefCell<Option<glib::SourceId>>,

    next_listener_id: Cell<u64>,
    on_log: RefCell<Vec<(u64, Rc<dyn Fn(&str)>)>>,
    on_finished: RefCell<Vec<(u64, Rc<dyn Fn(bool)>)>>,
    on_disconnected: RefCell<Vec<(u64, Rc<dyn Fn(bool)>)>>,
}

#[derive(Clone)]
pub struct Transaction {
    inner: Rc<Inner>,
}

impl Transaction {
    pub fn new() -> Self {
        let inner = Rc::new(Inner {
            stdin: RefCell::new(None),
            line_rx: RefCell::new(None),
            pending: RefCell::new(VecDeque::new()),
            state: Cell::new(TxnState::NotRunning),
            intentional_quit: Cell::new(false),
            idle_timeout_id: RefCell::new(None),
            next_listener_id: Cell::new(0),
            on_log: RefCell::new(Vec::new()),
            on_finished: RefCell::new(Vec::new()),
            on_disconnected: RefCell::new(Vec::new()),
        });

        // One persistent poll, for the object's whole lifetime, rather
        // than one per spawn — same reasoning as PackageStore's reload
        // poll: only plain `String` data crosses the worker-thread
        // boundary, applied here on the main thread only.
        let txn = Transaction { inner };
        {
            let weak = Rc::downgrade(&txn.inner);
            glib::source::timeout_add_local(Duration::from_millis(20), move || {
                let Some(inner) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let t = Transaction { inner };
                t.poll_lines();
                glib::ControlFlow::Continue
            });
        }
        txn
    }

    /// Every `connect_*` returns an id that a later `disconnect_*` call
    /// can use to remove that one listener. Callers that attach a
    /// listener for the duration of something shorter than the
    /// `Transaction`'s own lifetime (e.g. a single apply/sync dialog)
    /// must disconnect it themselves once done — otherwise it stays
    /// registered (and keeps firing, against stale UI state) for as
    /// long as the session itself lives.
    fn next_id(&self) -> u64 {
        let id = self.inner.next_listener_id.get();
        self.inner.next_listener_id.set(id + 1);
        id
    }

    pub fn connect_log(&self, f: impl Fn(&str) + 'static) -> u64 {
        let id = self.next_id();
        self.inner.on_log.borrow_mut().push((id, Rc::new(f)));
        id
    }
    pub fn disconnect_log(&self, id: u64) {
        self.inner.on_log.borrow_mut().retain(|(i, _)| *i != id);
    }

    pub fn connect_finished(&self, f: impl Fn(bool) + 'static) -> u64 {
        let id = self.next_id();
        self.inner.on_finished.borrow_mut().push((id, Rc::new(f)));
        id
    }
    pub fn disconnect_finished(&self, id: u64) {
        self.inner.on_finished.borrow_mut().retain(|(i, _)| *i != id);
    }

    pub fn connect_disconnected(&self, f: impl Fn(bool) + 'static) -> u64 {
        let id = self.next_id();
        self.inner.on_disconnected.borrow_mut().push((id, Rc::new(f)));
        id
    }
    pub fn disconnect_disconnected(&self, id: u64) {
        self.inner.on_disconnected.borrow_mut().retain(|(i, _)| *i != id);
    }

    // Each `emit_*` clones the current listener `Rc`s into a temporary
    // `Vec` and drops the `RefCell` borrow before invoking any of them,
    // specifically so a listener is free to call `disconnect_*` (on
    // itself or another id) from within its own callback without
    // hitting a double-borrow panic.
    fn emit_log(&self, line: &str) {
        let cbs: Vec<Rc<dyn Fn(&str)>> =
            self.inner.on_log.borrow().iter().map(|(_, f)| f.clone()).collect();
        for cb in cbs {
            cb(line);
        }
    }
    fn emit_finished(&self, success: bool) {
        let cbs: Vec<Rc<dyn Fn(bool)>> =
            self.inner.on_finished.borrow().iter().map(|(_, f)| f.clone()).collect();
        for cb in cbs {
            cb(success);
        }
    }
    fn emit_disconnected(&self, expected: bool) {
        let cbs: Vec<Rc<dyn Fn(bool)>> = self
            .inner
            .on_disconnected
            .borrow()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        for cb in cbs {
            cb(expected);
        }
    }

    /// Queue a protocol command line (without trailing newline).
    pub fn add_command(&self, command: &str) {
        self.inner.pending.borrow_mut().push_back(command.to_string());
    }

    /// Ensures the helper is running (spawning/authenticating if
    /// needed) and runs every currently-queued command.
    pub fn run_async(&self) {
        match self.inner.state.get() {
            TxnState::NotRunning => {
                if self.inner.pending.borrow().is_empty() {
                    return; // don't spawn (and prompt) for nothing
                }
                self.inner.state.set(TxnState::Spawning);
                if !self.spawn_helper() {
                    self.inner.state.set(TxnState::NotRunning);
                    self.emit_finished(false);
                }
            }
            TxnState::Spawning | TxnState::Busy => {
                // Already working through a batch — newly queued
                // commands get picked up automatically as the existing
                // poll loop drains the queue via send_next_command().
            }
            TxnState::Idle => {
                if self.inner.pending.borrow().is_empty() {
                    return;
                }
                self.cancel_idle_timer();
                self.send_next_command();
            }
        }
    }

    /// Sends QUIT to the helper if it's running and tears down cleanly.
    pub fn shutdown(&self) {
        if self.inner.state.get() == TxnState::NotRunning {
            return;
        }
        self.initiate_shutdown(None);
    }

    // ── Helper binary discovery ─────────────────────────────────────

    fn find_helper_path() -> Option<PathBuf> {
        if let Ok(over) = std::env::var("CAERUS_HELPER_PATH") {
            let p = PathBuf::from(&over);
            if is_executable(&p) {
                return Some(p);
            }
        }

        // Dev build: helper sits next to the running caerus binary.
        if let Ok(self_exe) = std::fs::read_link("/proc/self/exe") {
            if let Some(dir) = self_exe.parent() {
                let candidate = dir.join("caerus-helper");
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }

        const INSTALL_PATH: &str = "/usr/libexec/caerus-helper";
        let p = PathBuf::from(INSTALL_PATH);
        if is_executable(&p) {
            return Some(p);
        }

        which("caerus-helper")
    }

    // ── Spawning ─────────────────────────────────────────────────────

    fn spawn_helper(&self) -> bool {
        let Some(helper_path) = Self::find_helper_path() else {
            self.emit_log("ERROR caerus-helper not found");
            return false;
        };
        let Some(pkexec_path) = which("pkexec") else {
            self.emit_log("ERROR pkexec not found");
            return false;
        };

        let mut cmd = Command::new(&pkexec_path);
        cmd.arg(&helper_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.emit_log(&format!("ERROR failed to launch helper: {}", e));
                return false;
            }
        };

        let stdin = child.stdin.take().expect("helper stdin was piped");
        let stdout = child.stdout.take().expect("helper stdout was piped");
        let stderr = child.stderr.take().expect("helper stderr was piped");

        // Forward stdout+stderr into one channel (functional equivalent
        // of the original's dup2()-based G_SUBPROCESS_FLAGS_STDERR_MERGE),
        // then reap the child once both readers finish. Nothing outside
        // this closure ever touches `child` again.
        let (line_tx, line_rx) = mpsc::channel::<String>();
        let tx_out = line_tx.clone();
        let out_handle = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx_out.send(line).is_err() {
                    break;
                }
            }
        });
        let tx_err = line_tx;
        let err_handle = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if tx_err.send(line).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let _ = out_handle.join();
            let _ = err_handle.join();
            let _ = child.wait(); // reap; exit code isn't otherwise used
        });

        *self.inner.stdin.borrow_mut() = Some(stdin);
        *self.inner.line_rx.borrow_mut() = Some(line_rx);
        self.inner.intentional_quit.set(false);
        true // waits for READY, which triggers the queue via poll_lines()
    }

    // ── Poll loop (runs on the GTK main thread) ─────────────────────

    fn poll_lines(&self) {
        // Snapshot whether a receiver exists without holding the
        // RefCell borrow across handle_line (which may itself need to
        // clear `line_rx` via handle_disconnect()).
        let mut disconnected = false;
        loop {
            let line = {
                let rx_ref = self.inner.line_rx.borrow();
                match rx_ref.as_ref() {
                    None => break,
                    Some(rx) => match rx.try_recv() {
                        Ok(l) => Some(l),
                        Err(mpsc::TryRecvError::Empty) => None,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            None
                        }
                    },
                }
            };
            match line {
                Some(l) => self.handle_line(&l),
                None => break,
            }
        }
        if disconnected {
            self.handle_disconnect();
        }
    }

    // ── Protocol state machine ───────────────────────────────────────

    fn write_line(&self, line: &str) {
        if let Some(stdin) = self.inner.stdin.borrow_mut().as_mut() {
            let full = format!("{}\n", line);
            if let Err(e) = stdin.write_all(full.as_bytes()) {
                self.emit_log(&format!("write error: {}", e));
            }
        }
    }

    fn end_of_batch(&self, success: bool) {
        self.inner.state.set(TxnState::Idle);
        self.start_idle_timer();
        self.emit_finished(success);
    }

    fn send_next_command(&self) {
        let next = self.inner.pending.borrow_mut().pop_front();
        match next {
            None => self.end_of_batch(true),
            Some(cmd) => {
                self.inner.state.set(TxnState::Busy);
                self.write_line(&cmd);
            }
        }
    }

    fn handle_line(&self, line: &str) {
        self.emit_log(line);

        if self.inner.intentional_quit.get() {
            // Already told it to exit — ignore further protocol lines
            // (including its own "OK" for our QUIT), just wait for EOF.
            return;
        }

        if line == "READY" {
            self.send_next_command();
            return;
        }
        if line == "OK" {
            self.send_next_command();
            return;
        }
        if line.starts_with("ERROR") {
            self.inner.pending.borrow_mut().clear();
            self.end_of_batch(false);
            return;
        }
        // "LOG ..." or anything else — already emitted via emit_log above
    }

    /// Process exited (expectedly via our own QUIT, or not) — or we're
    /// being torn down proactively via `shutdown()`. Reentrant-safe.
    fn handle_disconnect(&self) {
        if self.inner.state.get() == TxnState::NotRunning {
            return;
        }
        let expected = self.inner.intentional_quit.get();

        *self.inner.stdin.borrow_mut() = None;
        *self.inner.line_rx.borrow_mut() = None;
        self.cancel_idle_timer();

        let was_mid_batch = matches!(self.inner.state.get(), TxnState::Busy | TxnState::Spawning);

        self.inner.state.set(TxnState::NotRunning);
        self.inner.pending.borrow_mut().clear();
        self.inner.intentional_quit.set(false);

        self.emit_disconnected(expected);

        // Unexpected death mid-batch (crash, killed, etc.) — make sure
        // whatever was waiting on "finished" doesn't hang forever.
        if was_mid_batch && !expected {
            self.emit_finished(false);
        }
    }

    /// Shared by both the idle-timeout and the explicit shutdown()
    /// entry point: tell the helper to exit, then tear our own state
    /// down immediately rather than waiting on the process's own exit
    /// timing.
    fn initiate_shutdown(&self, reason_log: Option<&str>) {
        self.inner.intentional_quit.set(true);
        if let Some(r) = reason_log {
            self.emit_log(r);
        }
        self.write_line("QUIT"); // best-effort courtesy to the helper
        self.handle_disconnect();
    }

    // ── Idle timer ────────────────────────────────────────────────────

    fn start_idle_timer(&self) {
        self.cancel_idle_timer();
        let weak = Rc::downgrade(&self.inner);
        let id = glib::source::timeout_add_local_once(IDLE_TIMEOUT, move || {
            let Some(inner) = weak.upgrade() else { return };
            *inner.idle_timeout_id.borrow_mut() = None;
            let txn = Transaction { inner };
            txn.initiate_shutdown(Some(
                "Session idle — re-authentication will be required for the next action.",
            ));
        });
        *self.inner.idle_timeout_id.borrow_mut() = Some(id);
    }

    fn cancel_idle_timer(&self) {
        if let Some(id) = self.inner.idle_timeout_id.borrow_mut().take() {
            id.remove();
        }
    }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

fn which(program: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}
