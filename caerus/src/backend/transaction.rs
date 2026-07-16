//! `Transaction` — drives `caerus-helper` as a long-lived child process,
//! spawned via `pkexec` directly (the GUI itself is never privileged).
//! Rust translation of backend/transaction.{h,c}.
//!
//! One instance lives for the whole app session: create it once, call
//! `run_batch()` each time there's a new batch of commands. The
//! underlying helper process is spawned lazily on first use and then
//! kept alive — repeated batches do NOT re-trigger authentication.
//!
//! Batches are first-class: each `run_batch()` call carries its own
//! completion callback, batches queued while another is in flight wait
//! their turn (they never merge), and a failed command aborts only the
//! batch it belongs to — queued batches behind it still run.
//!
//! The helper is only ever told to exit (QUIT) in two situations:
//!   - it has sat idle (no command in flight, none queued) for
//!     `IDLE_TIMEOUT`, or
//!   - `shutdown()` is called explicitly (app exit).
//!
//! Either way, the *next* call to `run_batch()` after that transparently
//! respawns the helper — a fresh `pkexec` prompt — since this is
//! indistinguishable from first use.
//!
//! Callbacks (all invoked on the main thread, mirroring the original's
//! `GObject` signals):
//!   `connect_log`          (line: &str)              -- every raw line
//!   `connect_disconnected` (`DisconnectReason`)          -- helper exited
//! Batch completion is NOT a session-wide signal: it's the per-batch
//! `on_finished` closure passed to `run_batch()`, which fires exactly
//! once (success, a failed command, or a disconnect all resolve it).

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

type LogCb = Rc<dyn Fn(&str)>;
type LogCbs = RefCell<Vec<(u64, LogCb)>>;
type DisconnectedCb = Rc<dyn Fn(DisconnectReason)>;
type DisconnectedCbs = RefCell<Vec<(u64, DisconnectedCb)>>;

/// One `run_batch()` call: its still-unsent commands plus its
/// completion callback. `on_finished` is an `Option` only so it can be
/// taken out and invoked exactly once after the batch leaves the queue.
struct Batch {
    commands: VecDeque<String>,
    on_finished: Option<Box<dyn FnOnce(bool)>>,
}

impl Batch {
    /// Consumes the callback; safe to call more than once (later calls
    /// are no-ops), which keeps the disconnect path reentrant-safe.
    fn finish(mut self, success: bool) {
        if let Some(cb) = self.on_finished.take() {
            cb(success);
        }
    }
}

/// Why the helper connection ended, passed to `connect_disconnected`
/// listeners — distinguishes "nothing to worry about" from "this will
/// keep failing every time" so the UI can give an actionable message
/// instead of a generic one, without assuming anything about which
/// desktop environment (or none at all) the user is running.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisconnectReason {
    /// `shutdown()` or the idle timeout's own `QUIT` — expected, no UI
    /// action needed.
    Expected,
    /// The helper died after successfully authenticating and sending
    /// `READY` at least once (crashed, killed, etc) — unexpected, but
    /// the *next* attempt will simply re-authenticate and likely work.
    Unexpected,
    /// `pkexec` itself never got past authentication — the helper
    /// never sent `READY`, so this session's `TxnState` never left
    /// `Spawning`. Most commonly: no polkit authentication agent is
    /// registered for this session at all (a real possibility on a
    /// bare window manager setup that never started one — GNOME/KDE/
    /// XFCE start one automatically, a minimal WM doesn't), the user
    /// cancelled/failed the prompt, or `pkexec` itself isn't usable.
    /// Unlike `Unexpected`, simply retrying won't help without fixing
    /// the underlying cause first.
    AuthFailed,
}

struct Inner {
    stdin: RefCell<Option<ChildStdin>>,
    line_rx: RefCell<Option<mpsc::Receiver<String>>>,
    batches: RefCell<VecDeque<Batch>>,
    state: Cell<TxnState>,
    intentional_quit: Cell<bool>,
    idle_timeout_id: RefCell<Option<glib::SourceId>>,

    next_listener_id: Cell<u64>,
    on_log: LogCbs,
    on_disconnected: DisconnectedCbs,
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
            batches: RefCell::new(VecDeque::new()),
            state: Cell::new(TxnState::NotRunning),
            intentional_quit: Cell::new(false),
            idle_timeout_id: RefCell::new(None),
            next_listener_id: Cell::new(0),
            on_log: RefCell::new(Vec::new()),
            on_disconnected: RefCell::new(Vec::new()),
        });

        // One persistent poll, for the object's whole lifetime, rather
        // than one per spawn — same reasoning as PackageStore's reload
        // poll: only plain `String` data crosses the worker-thread
        // boundary, applied here on the main thread only.
        let txn = Self { inner };
        {
            let weak = Rc::downgrade(&txn.inner);
            glib::source::timeout_add_local(Duration::from_millis(20), move || {
                let Some(inner) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let t = Self { inner };
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

    pub fn connect_disconnected(&self, f: impl Fn(DisconnectReason) + 'static) -> u64 {
        let id = self.next_id();
        self.inner
            .on_disconnected
            .borrow_mut()
            .push((id, Rc::new(f)));
        id
    }
    pub fn disconnect_disconnected(&self, id: u64) {
        self.inner
            .on_disconnected
            .borrow_mut()
            .retain(|(i, _)| *i != id);
    }

    // Each `emit_*` clones the current listener `Rc`s into a temporary
    // `Vec` and drops the `RefCell` borrow before invoking any of them,
    // specifically so a listener is free to call `disconnect_*` (on
    // itself or another id) from within its own callback without
    // hitting a double-borrow panic.
    fn emit_log(&self, line: &str) {
        let cbs: Vec<LogCb> = self
            .inner
            .on_log
            .borrow()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        for cb in cbs {
            cb(line);
        }
    }
    fn emit_disconnected(&self, reason: DisconnectReason) {
        let cbs: Vec<DisconnectedCb> = self
            .inner
            .on_disconnected
            .borrow()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        for cb in cbs {
            cb(reason);
        }
    }

    /// Queues `commands` (protocol lines, no trailing newline) as one
    /// batch and ensures the helper is running (spawning/authenticating
    /// if needed). `on_finished` fires exactly once, on the main
    /// thread, when this batch — and only this batch — resolves:
    /// `true` after every command answered OK, `false` on the first
    /// ERROR, a helper disconnect, or a spawn failure. Batches queued
    /// while another is in flight wait their turn; they never merge
    /// into the running batch's outcome.
    ///
    /// Refuses (and logs, rather than silently dropping) any command
    /// containing a control character. Every caller builds these lines
    /// by joining package/repo names that ultimately come from repo
    /// index data, not literal user input — `write_line` below appends
    /// exactly one `\n` per queued command, so a name with an embedded
    /// `\n` (or other control character) would otherwise let a single
    /// malicious/malformed repo entry smuggle a second, unintended
    /// command into this already-`pkexec`-authenticated session. This
    /// is the single choke point every command line passes through
    /// (via `apply_dialog::run`), so it covers every verb — including
    /// ones like `INSTALL`/`REMOVE` that don't do their own validation
    /// the way `repo_manager`'s URL entry does.
    pub fn run_batch(&self, commands: Vec<String>, on_finished: impl FnOnce(bool) + 'static) {
        let mut accepted: VecDeque<String> = VecDeque::with_capacity(commands.len());
        for command in commands {
            if !command_is_safe(&command) {
                self.emit_log(&format!(
                    "refusing to queue malformed command (contains control characters): {command:?}"
                ));
                continue;
            }
            accepted.push_back(command);
        }
        if accepted.is_empty() {
            on_finished(true); // nothing to run — don't spawn (and prompt) for nothing
            return;
        }
        self.inner.batches.borrow_mut().push_back(Batch {
            commands: accepted,
            on_finished: Some(Box::new(on_finished)),
        });

        match self.inner.state.get() {
            TxnState::NotRunning => {
                self.inner.state.set(TxnState::Spawning);
                if !self.spawn_helper() {
                    self.inner.state.set(TxnState::NotRunning);
                    self.fail_all_batches();
                }
            }
            TxnState::Spawning | TxnState::Busy => {
                // Already working — this batch gets picked up once the
                // in-flight one resolves, via send_next_command().
            }
            TxnState::Idle => {
                self.cancel_idle_timer();
                self.send_next_command();
            }
        }
    }

    /// Drains the whole batch queue, resolving every callback with
    /// failure — spawn failed or the helper disconnected, so none of
    /// the queued work can run. The queue is snapshotted before any
    /// callback runs, so a callback is free to call `run_batch` again
    /// (respawning the helper) without its fresh batch being swept up
    /// in this failure pass.
    fn fail_all_batches(&self) {
        let batches = std::mem::take(&mut *self.inner.batches.borrow_mut());
        for batch in batches {
            batch.finish(false);
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
        const INSTALL_PATH: &str = "/usr/libexec/caerus-helper";

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
                self.emit_log(&format!("ERROR failed to launch helper: {e}"));
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
            let full = format!("{line}\n");
            if let Err(e) = stdin.write_all(full.as_bytes()) {
                self.emit_log(&format!("write error: {e}"));
            }
        }
    }

    /// Advances the queue: sends the front batch's next command, or —
    /// when the front batch has no commands left — resolves it as
    /// successful and moves on to the next batch, going `Idle` only
    /// once the queue is empty. Each completed batch's callback runs
    /// with no queue borrow held (it may call `run_batch` itself; the
    /// state is still `Busy` at that point, so the new batch just
    /// queues and this loop picks it up).
    fn send_next_command(&self) {
        loop {
            enum Step {
                Send(String),
                Completed(Batch),
                QueueEmpty,
            }
            let step = {
                let mut batches = self.inner.batches.borrow_mut();
                match batches.front_mut() {
                    None => Step::QueueEmpty,
                    Some(front) => match front.commands.pop_front() {
                        Some(cmd) => Step::Send(cmd),
                        None => Step::Completed(batches.pop_front().expect("front batch exists")),
                    },
                }
            };
            match step {
                Step::Send(cmd) => {
                    self.inner.state.set(TxnState::Busy);
                    self.write_line(&cmd);
                    return;
                }
                Step::Completed(batch) => batch.finish(true),
                Step::QueueEmpty => {
                    self.inner.state.set(TxnState::Idle);
                    self.start_idle_timer();
                    return;
                }
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
            // Abort only the batch the failed command belongs to —
            // its remaining commands are dropped with it. Batches
            // queued behind it are independent requests and still run.
            let failed = self.inner.batches.borrow_mut().pop_front();
            if let Some(batch) = failed {
                batch.finish(false);
            }
            self.send_next_command();
        }
        // "LOG ..." or anything else — already emitted via emit_log above.
    }

    /// Process exited (expectedly via our own QUIT, or not) — or we're
    /// being torn down proactively via `shutdown()`. Reentrant-safe.
    fn handle_disconnect(&self) {
        if self.inner.state.get() == TxnState::NotRunning {
            return;
        }
        let expected = self.inner.intentional_quit.get();
        // Still `Spawning` here means `READY` was never received — the
        // helper process (if it ever even started) never got as far as
        // `send_next_command`, which is the only place that moves state
        // out of `Spawning`. That's the distinguishing signal for
        // "pkexec never authenticated" versus "the helper died later".
        let never_authenticated = self.inner.state.get() == TxnState::Spawning;

        *self.inner.stdin.borrow_mut() = None;
        *self.inner.line_rx.borrow_mut() = None;
        self.cancel_idle_timer();

        self.inner.state.set(TxnState::NotRunning);
        self.inner.intentional_quit.set(false);

        let reason = if expected {
            DisconnectReason::Expected
        } else if never_authenticated {
            DisconnectReason::AuthFailed
        } else {
            DisconnectReason::Unexpected
        };
        self.emit_disconnected(reason);

        // Every unresolved batch — the one mid-flight and any queued
        // behind it — resolves as failed, so nothing waiting on an
        // `on_finished` hangs forever. State is already `NotRunning`,
        // so a callback that retries via `run_batch` respawns cleanly.
        self.fail_all_batches();
    }

    /// Shared by both the idle-timeout and the explicit `shutdown()`
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
            let txn = Self { inner };
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
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
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

/// A protocol command line is safe to queue iff it contains no control
/// characters — see `run_batch`'s doc comment for why an embedded
/// newline (or any other control character) in a package/repo name
/// must never reach the already-authenticated helper.
fn command_is_safe(command: &str) -> bool {
    !command.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_commands_are_safe() {
        assert!(command_is_safe("SYNC"));
        assert!(command_is_safe("INSTALL foo bar-2.0"));
        assert!(command_is_safe("ADDREPO https://repo.example/current?a=b"));
        assert!(command_is_safe("INSTALL p\u{e4}ckage")); // non-ASCII is fine
        assert!(command_is_safe("")); // vacuously safe; run_batch drops it upstream anyway
    }

    #[test]
    fn control_characters_are_rejected() {
        // An embedded newline is the actual attack: it would smuggle a
        // second command line into the pkexec-authenticated session.
        assert!(!command_is_safe("INSTALL foo\nREMOVE bar"));
        assert!(!command_is_safe("INSTALL foo\tbar"));
        assert!(!command_is_safe("INSTALL foo\u{0}"));
        assert!(!command_is_safe("INSTALL foo\u{1b}[31m")); // ESC
        assert!(!command_is_safe("\rINSTALL foo"));
    }
}
