use crate::backend::progress::{self, Event};
use crate::backend::transaction::{DisconnectReason, Transaction};
use crate::ui::dialog_util::modal_window;
use crate::ui::widgets::{set_pill, status_pill, PillKind};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

thread_local! { static AUTO_CLOSE_ON_SUCCESS: Cell<bool> = const { Cell::new(false) }; }
pub fn set_auto_close_on_success(enabled: bool) {
    AUTO_CLOSE_ON_SUCCESS.set(enabled);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResultState {
    Pending,
    Working,
    Completed,
    Interrupted,
    NotProcessed,
}
impl ResultState {
    fn finish(self, success: bool) -> Self {
        match self {
            Self::Working | Self::Pending if success => Self::Completed,
            Self::Working => Self::Interrupted,
            Self::Pending => Self::NotProcessed,
            other => other,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Working => "Working",
            Self::Completed => "Completed",
            Self::Interrupted => "Failed or interrupted — inspect log",
            Self::NotProcessed => "Not processed",
        }
    }
}
impl ResultState {
    fn pill(self) -> (&'static str, PillKind) {
        match self {
            Self::Pending => ("Pending", PillKind::Neutral),
            Self::Working => ("Working", PillKind::Accent),
            Self::Completed => ("Completed", PillKind::Done),
            Self::Interrupted => ("Failed", PillKind::Failed),
            Self::NotProcessed => ("Not processed", PillKind::Warn),
        }
    }
}

/// One package line: its name and version, and a status pill.
struct PackageRow {
    widget: gtk::Box,
    name: gtk::Label,
    pill: gtk::Label,
    state: ResultState,
}

impl PackageRow {
    fn new(name: &str) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        widget.add_css_class("urow");
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        let (text, kind) = ResultState::Pending.pill();
        let pill = status_pill(text, kind);
        widget.append(&label);
        widget.append(&pill);
        Self {
            widget,
            name: label,
            pill,
            state: ResultState::Pending,
        }
    }

    /// A stage reported while the command runs.
    fn show(&self, name: &str, version: &str, stage: &str) {
        self.name.set_text(&format!("{name} {version}"));
        let kind = if self.state == ResultState::Completed {
            PillKind::Done
        } else {
            PillKind::Accent
        };
        set_pill(&self.pill, stage, kind);
        self.name
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{name}, {stage}"
            ))]);
    }

    /// The settled result once the command is over.
    fn show_final(&self, name: &str) {
        let (text, kind) = self.state.pill();
        set_pill(&self.pill, text, kind);
        self.name
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{name}, {}",
                self.state.label()
            ))]);
    }
}

/// The phases shown as chips in the overall card, in order.
fn phase_plan(commands: &[String]) -> Vec<&'static str> {
    let verbs: Vec<&str> = commands
        .iter()
        .map(|c| c.split_once(' ').map_or(c.as_str(), |(verb, _)| verb))
        .collect();
    let installs = |v: &&str| {
        matches!(
            *v,
            "INSTALL" | "INSTALL_FORCE" | "UPDATE" | "UPGRADE" | "REINSTALL"
        )
    };
    if verbs.iter().any(installs) {
        vec!["Download", "Install"]
    } else if !verbs.is_empty()
        && verbs
            .iter()
            .all(|v| matches!(*v, "REMOVE" | "REMOVE_FORCE" | "PURGE" | "PURGE_FORCE"))
    {
        vec!["Remove"]
    } else if !verbs.is_empty() && verbs.iter().all(|v| *v == "SYNC") {
        vec!["Refresh"]
    } else if !verbs.is_empty() && verbs.iter().all(|v| *v == "DOWNLOAD") {
        vec!["Download"]
    } else {
        vec!["Run"]
    }
}

/// Which chip a reported stage belongs to.
fn phase_for_stage(stage: &str, plan: &[&str]) -> usize {
    let wanted = match stage {
        "Downloading" | "Verifying" | "Checking" => "Download",
        "Removing" | "Purging" => "Remove",
        _ => "Install",
    };
    plan.iter()
        .position(|p| *p == wanted)
        .unwrap_or(plan.len().saturating_sub(1))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChipState {
    Idle,
    Active,
    Done,
    Failed,
}

fn phase_chip_state(index: usize, current: usize) -> ChipState {
    match index.cmp(&current) {
        std::cmp::Ordering::Less => ChipState::Done,
        std::cmp::Ordering::Equal => ChipState::Active,
        std::cmp::Ordering::Greater => ChipState::Idle,
    }
}

/// After the command ends: success completes every phase, a failure marks the
/// phase it stopped in.
fn final_chip_state(index: usize, current: usize, success: bool) -> ChipState {
    if success {
        ChipState::Done
    } else {
        match phase_chip_state(index, current) {
            ChipState::Active => ChipState::Failed,
            other => other,
        }
    }
}

fn set_phase_chip(chip: &gtk::Label, state: ChipState) {
    for class in ["active", "done", "err"] {
        chip.remove_css_class(class);
    }
    match state {
        ChipState::Idle => {}
        ChipState::Active => chip.add_css_class("active"),
        ChipState::Done => chip.add_css_class("done"),
        ChipState::Failed => chip.add_css_class("err"),
    }
}

fn requested_names(commands: &[String]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for command in commands {
        let (verb, rest) = command.split_once(' ').unwrap_or((command, ""));
        // Forced variants target the same packages as their plain verbs.
        let verb = verb.strip_suffix("_FORCE").unwrap_or(verb);
        if [
            "INSTALL",
            "UPDATE",
            "REMOVE",
            "PURGE",
            "HOLD",
            "UNHOLD",
            "REINSTALL",
            "DOWNLOAD",
            "RECONFIGURE",
        ]
        .contains(&verb)
        {
            names.extend(rest.split_whitespace().map(str::to_owned));
        }
    }
    names.into_iter().collect()
}

fn run_inner(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    changes: Option<String>,
    done_cb: impl Fn(bool) + 'static,
    done_after_close: bool,
) {
    let (dlg, outer) = modal_window(title, parent, true, (620, -1), 12);
    // Overall card: what is happening, how far along, and which phase.
    let plan = phase_plan(commands);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("overall-card");
    card.add_css_class("card");
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let status = gtk::Label::new(Some("Waiting for authorization…"));
    status.set_xalign(0.0);
    status.set_hexpand(true);
    status.set_wrap(true);
    status.add_css_class("heading");
    status.add_css_class("overall-title");
    let pct = gtk::Label::new(None);
    pct.add_css_class("overall-pct");
    top.append(&status);
    top.append(&pct);
    card.append(&top);
    let phase = gtk::Label::new(Some("Preparing transaction"));
    phase.set_xalign(0.0);
    phase.set_wrap(true);
    phase.add_css_class("dim-label");
    phase.add_css_class("overall-meta");
    card.append(&phase);
    let progress = gtk::ProgressBar::new();
    progress.add_css_class("overall-bar");
    card.append(&progress);
    let chips_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let chips: Vec<gtk::Label> = plan
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let chip = gtk::Label::new(Some(&format!("{} · {name}", i + 1)));
            chip.add_css_class("phase-chip");
            chips_box.append(&chip);
            chip
        })
        .collect();
    card.append(&chips_box);
    outer.append(&card);
    let phase_index = Rc::new(Cell::new(0usize));
    let rows_box = gtk::ListBox::new();
    rows_box.set_selection_mode(gtk::SelectionMode::None);
    let rows: Rc<RefCell<BTreeMap<String, PackageRow>>> = Rc::new(RefCell::new(BTreeMap::new()));
    for name in requested_names(commands) {
        let row = PackageRow::new(&name);
        rows_box.append(&row.widget);
        rows.borrow_mut().insert(name, row);
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_widget_name("transaction-packages");
    scroll.set_max_content_height(144);
    scroll.set_propagate_natural_height(true);
    scroll.set_visible(!rows.borrow().is_empty());
    scroll.set_child(Some(&rows_box));
    outer.append(&scroll);
    // The raw output is behind a toggle; the card is the default view.
    let log_scroll = gtk::ScrolledWindow::new();
    log_scroll.set_widget_name("transaction-log");
    log_scroll.set_min_content_height(180);
    log_scroll.set_vexpand(true);
    log_scroll.set_visible(false);
    let text = gtk::TextView::new();
    text.set_editable(false);
    text.set_cursor_visible(false);
    // Never focus the read-only console: ibus can dereference an unrooted native surface.
    text.set_focusable(false);
    text.set_monospace(true);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    log_scroll.set_child(Some(&text));
    outer.append(&log_scroll);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let toggle_output = gtk::ToggleButton::with_label("Show output");
    toggle_output.set_halign(gtk::Align::Start);
    toggle_output.set_hexpand(true);
    {
        let log_scroll = log_scroll.clone();
        toggle_output.connect_toggled(move |button| {
            log_scroll.set_visible(button.is_active());
            button.set_label(if button.is_active() {
                "Hide output"
            } else {
                "Show output"
            });
        });
    }
    let copy = gtk::Button::with_label("Copy log");
    let close = gtk::Button::with_label("Close");
    close.set_sensitive(false);
    actions.append(&toggle_output);
    actions.append(&copy);
    actions.append(&close);
    outer.append(&actions);
    let log_text = Rc::new(RefCell::new(String::new()));
    {
        let log = log_text.clone();
        copy.connect_clicked(move |b| b.clipboard().set_text(&log.borrow()));
    }
    let busy = Rc::new(Cell::new(true));
    {
        let busy = busy.clone();
        let progress = progress.clone();
        let rows = rows.clone();
        glib::source::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if !busy.get() {
                return glib::ControlFlow::Break;
            }
            // Package rows give a real fraction; until then show activity.
            if rows.borrow().is_empty() {
                progress.pulse();
            }
            glib::ControlFlow::Continue
        });
    }
    let refresh_overall: Rc<dyn Fn()> = {
        let rows = rows.clone();
        let progress = progress.clone();
        let pct = pct.clone();
        let chips = chips.clone();
        let phase_index = phase_index.clone();
        Rc::new(move || {
            let (total, done) = {
                let rows = rows.borrow();
                (
                    rows.len(),
                    rows.values()
                        .filter(|row| row.state == ResultState::Completed)
                        .count(),
                )
            };
            if total > 0 {
                let fraction = done as f64 / total as f64;
                progress.set_fraction(fraction);
                pct.set_text(&format!("{}%", (fraction * 100.0).round()));
            }
            for (i, chip) in chips.iter().enumerate() {
                set_phase_chip(chip, phase_chip_state(i, phase_index.get()));
            }
        })
    };
    let active = Rc::new(RefCell::new(HashSet::<String>::new()));
    let apply_event: Rc<dyn Fn(Event)> = {
        let rows = rows.clone();
        let active = active.clone();
        let phase = phase.clone();
        let status = status.clone();
        let refresh_overall = refresh_overall.clone();
        let phase_index = phase_index.clone();
        let plan = plan.clone();
        Rc::new(move |event| {
            debug_assert!(Event::parse(&event.encode()).is_some());
            match event {
                Event::Command { verb, names } => {
                    active.borrow_mut().clear();
                    for name in names {
                        if rows.borrow().contains_key(&name) {
                            active.borrow_mut().insert(name.clone());
                        }
                    }
                    let label = match verb.as_str() {
                        "SYNC" => "Refreshing repositories",
                        "INSTALL" | "UPDATE" | "UPGRADE" => "Resolving package changes",
                        "REMOVE" | "PURGE" => "Removing packages",
                        _ => "Running package operation",
                    };
                    phase.set_text(label);
                    status.set_text("Applying changes…");
                }
                Event::Phase(stage) => {
                    phase.set_text(&stage);
                    phase.update_property(&[gtk::accessible::Property::Description(&stage)]);
                    phase_index.set(phase_for_stage(&stage, &plan).max(phase_index.get()));
                }
                Event::Package {
                    name,
                    version,
                    stage,
                } => {
                    {
                        let mut rows = rows.borrow_mut();
                        let row = rows.entry(name.clone()).or_insert_with(|| {
                            let row = PackageRow::new(&name);
                            rows_box.append(&row.widget);
                            scroll.set_visible(true);
                            row
                        });
                        row.state = if stage == "Completed" {
                            ResultState::Completed
                        } else {
                            ResultState::Working
                        };
                        row.show(&name, &version, &stage);
                    }
                    active.borrow_mut().insert(name);
                    phase.set_text(&stage);
                    phase_index.set(phase_for_stage(&stage, &plan).max(phase_index.get()));
                }
                Event::Finished(success) => {
                    for name in active.borrow().iter() {
                        if let Some(row) = rows.borrow_mut().get_mut(name) {
                            row.state = row.state.finish(success);
                            row.show_final(name);
                        }
                    }
                }
            }
            refresh_overall();
        })
    };
    let log_id = {
        let log = log_text.clone();
        let text = text;
        let apply_event = apply_event.clone();
        session.connect_log(move |line| {
            if let Some(event) = Event::parse(line) {
                apply_event(event);
                return;
            }
            // Old helpers remain usable; only observed package stages are inferred.
            if let Some(raw) = line.strip_prefix("LOG ") {
                if let Some(event) = progress::observe(raw) {
                    apply_event(event);
                }
            }
            if line == "OK" {
                apply_event(Event::Finished(true));
            }
            let raw = line.strip_prefix("LOG ").unwrap_or(line);
            let mut log = log.borrow_mut();
            if log.len() + raw.len() < crate::backend::history::MAX_TRANSACTION_LOG {
                log.push_str(raw);
                log.push('\n');
                let buffer = text.buffer();
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &format!("{raw}\n"));
            }
        })
    };
    let auth_failed = Rc::new(Cell::new(false));
    let disconnected_id = {
        let auth = auth_failed.clone();
        session.connect_disconnected(move |reason| {
            if reason == DisconnectReason::AuthFailed {
                auth.set(true);
            }
        })
    };
    let result_on_close = Rc::new(Cell::new(None));
    let done_cb = Rc::new(done_cb);
    {
        let dlg = dlg.clone();
        let result_on_close = result_on_close.clone();
        let done_cb = done_cb.clone();
        close.connect_clicked(move |_| {
            dlg.destroy();
            if let Some(success) = result_on_close.take() {
                done_cb(success);
            }
        });
    }
    {
        let busy = busy.clone();
        let result_on_close = result_on_close.clone();
        let done_cb = done_cb.clone();
        dlg.connect_close_request(move |_| {
            if busy.get() {
                glib::Propagation::Stop
            } else {
                if let Some(success) = result_on_close.take() {
                    let done_cb = done_cb.clone();
                    glib::idle_add_local_once(move || done_cb(success));
                }
                glib::Propagation::Proceed
            }
        });
    }
    dlg.present();
    let session_cleanup = session.clone();
    let commands = commands.to_vec();
    let recorded_commands = commands.clone();
    session.run_batch(commands, move |success| {
        busy.set(false);
        let mut details = changes.clone().unwrap_or_default();
        for (name, row) in rows.borrow_mut().iter_mut() {
            row.state = row.state.finish(success);
            row.show_final(name);
            details.push_str(&format!("\n{name} · {}", row.state.label()));
        }
        refresh_overall();
        let completed = rows
            .borrow()
            .values()
            .filter(|row| row.state == ResultState::Completed)
            .count();
        if success {
            progress.set_fraction(1.0);
            pct.set_text("100%");
            card.add_css_class("overall-ok");
        } else if completed > 0 {
            card.add_css_class("overall-warn");
        } else {
            card.add_css_class("overall-err");
        }
        for (i, chip) in chips.iter().enumerate() {
            set_phase_chip(chip, final_chip_state(i, phase_index.get(), success));
        }
        phase.set_text(if success {
            "Completed"
        } else {
            "Stopped — review the results and the output"
        });
        let message = if success {
            "Finished successfully."
        } else if auth_failed.get() {
            "Authorization failed. Check that a polkit authentication agent is running."
        } else {
            "Failed or partially completed. Reload package state before trying again."
        };
        status.set_text(message);
        status.update_property(&[gtk::accessible::Property::Description(message)]);
        close.set_sensitive(true);
        close.grab_focus();
        if !success {
            toggle_output.set_active(true);
        }
        if changes.is_some() {
            crate::backend::history::record_detailed(
                &recorded_commands,
                success,
                &details,
                &log_text.borrow(),
            );
        }
        session_cleanup.disconnect_log(log_id);
        session_cleanup.disconnect_disconnected(disconnected_id);
        if done_after_close {
            result_on_close.set(Some(success));
        } else {
            done_cb(success);
        }
        if success && AUTO_CLOSE_ON_SUCCESS.get() {
            close.emit_clicked();
        }
    });
}

pub fn run_recorded(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    done_cb: impl Fn(bool) + 'static,
) {
    run_inner(
        parent,
        session,
        commands,
        title,
        Some(String::new()),
        done_cb,
        false,
    );
}

/// Records the command result and its previewed changes when it finishes,
/// then calls `done_cb` after the progress window closes so another modal
/// can be opened safely.
pub fn run_recorded_until_closed(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    changes: String,
    done_cb: impl Fn(bool) + 'static,
) {
    run_inner(
        parent,
        session,
        commands,
        title,
        Some(changes),
        done_cb,
        true,
    );
}

/// Exercises the real transaction pipe and history writer with an unprivileged
/// fixture. Called by the one display test so GTK stays on one test thread.
#[cfg(test)]
pub fn smoke_test_transaction_history() {
    // The package results must fit their rows; spare dialog height belongs to
    // the visible log, including after a failed transaction.
    fn check_layout(widget: &gtk::Widget) {
        if widget.widget_name() == "transaction-packages" {
            assert!(
                widget.height() <= 144,
                "package results must not reserve blank space"
            );
        }
        if widget.widget_name() == "transaction-log" {
            assert!(
                widget.is_visible() && widget.height() >= 150,
                "a failed transaction must show its output ({} high)",
                widget.height()
            );
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            check_layout(&current);
            child = current.next_sibling();
        }
    }
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("caerus-ui-transaction-{}", std::process::id()));
    // The helper trust check rejects group-writable ancestors; do not depend
    // on the caller's umask or reuse a directory left by another process.
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let launcher = root.join("pkexec");
    std::fs::write(&launcher, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
    let helper = root.join("helper");
    std::fs::write(
        &helper,
        r#"#!/bin/sh
printf 'READY\n'
while IFS= read -r command; do
    case "$command" in
        QUIT) exit 0 ;;
        'INSTALL foo bar')
            printf 'EVENT 1\tcommand\tINSTALL\tfoo bar\n'
            printf 'LOG foo-2.0_1: installed successfully.\n'
            printf 'EVENT 1\tpackage\tfoo\t2.0_1\tCompleted\n'
            printf 'EVENT 1\tpackage\tdependency\t1.0_1\tConfiguring\n'
            printf 'LOG simulated dependency configuration failure\n'
            printf 'EVENT 1\tfinished\terror\n'
            printf 'ERROR fixture failed\n' ;;
    esac
done
"#,
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::var_os("PATH");
    let data = std::env::var_os("XDG_DATA_HOME");
    let override_path = std::env::var_os("CAERUS_HELPER_PATH");
    std::env::set_var("PATH", &root);
    std::env::set_var("CAERUS_HELPER_PATH", &helper);
    std::env::set_var("XDG_DATA_HOME", root.join("data"));
    let transaction = Transaction::new();
    let finished = Rc::new(Cell::new(None));
    let result = finished.clone();
    run_inner(
        None,
        &transaction,
        &["INSTALL foo bar".into()],
        "Fixture transaction",
        Some("foo · 1.0_1 → 2.0_1".into()),
        move |success| result.set(Some(success)),
        false,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let context = glib::MainContext::default();
    while finished.get().is_none() && std::time::Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Let the dialog re-measure after the failure revealed its output.
    for _ in 0..30 {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let entries = crate::backend::history::load();
    transaction.shutdown();
    for widget in gtk::Window::list_toplevels() {
        if let Ok(window) = widget.downcast::<gtk::Window>() {
            if window.title().as_deref() == Some("Fixture transaction") {
                check_layout(window.upcast_ref());
                window.destroy();
            }
        }
    }
    for (key, value) in [
        ("PATH", path),
        ("XDG_DATA_HOME", data),
        ("CAERUS_HELPER_PATH", override_path),
    ] {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(finished.get(), Some(false));
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].changes.contains("foo · Completed"),
        "fixture changes: {}; log: {}",
        entries[0].changes,
        entries[0].log
    );
    assert!(entries[0].changes.contains("bar · Not processed"));
    assert!(entries[0]
        .changes
        .contains("dependency · Failed or interrupted"));
    assert!(entries[0].changes.contains("1.0_1 → 2.0_1"));
    assert!(entries[0]
        .log
        .contains("simulated dependency configuration failure"));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_failure_preserves_completed_and_unprocessed_states() {
        assert_eq!(ResultState::Completed.finish(false), ResultState::Completed);
        assert_eq!(ResultState::Working.finish(false), ResultState::Interrupted);
        assert_eq!(
            ResultState::Pending.finish(false),
            ResultState::NotProcessed
        );
    }
    #[test]
    fn command_targets_exclude_repository_urls_and_deduplicate() {
        assert_eq!(
            requested_names(&[
                "INSTALL foo bar".into(),
                "REMOVE foo".into(),
                "ADDREPO https://example.org".into()
            ]),
            vec!["bar", "foo"]
        );
    }
    #[test]
    fn forced_commands_still_list_their_packages() {
        assert_eq!(
            requested_names(&[
                "REMOVE_FORCE foo bar".into(),
                "PURGE_FORCE baz".into(),
                "INSTALL_FORCE qux".into(),
            ]),
            vec!["bar", "baz", "foo", "qux"]
        );
    }
    #[test]
    fn whole_system_commands_have_no_package_rows() {
        assert!(
            requested_names(&["SYNC".into(), "UPGRADE".into(), "CLEANCACHE".into()]).is_empty()
        );
        assert!(requested_names(&["INSTALL".into()]).is_empty());
    }
    #[test]
    fn phase_plan_follows_the_command_verbs() {
        let plan =
            |cmds: &[&str]| phase_plan(&cmds.iter().map(|c| (*c).to_string()).collect::<Vec<_>>());
        assert_eq!(plan(&["INSTALL foo"]), ["Download", "Install"]);
        assert_eq!(plan(&["UPGRADE"]), ["Download", "Install"]);
        assert_eq!(plan(&["REMOVE foo", "PURGE bar"]), ["Remove"]);
        assert_eq!(plan(&["SYNC"]), ["Refresh"]);
        assert_eq!(plan(&["DOWNLOAD foo"]), ["Download"]);
        assert_eq!(plan(&["CLEANCACHE"]), ["Run"]);
        assert_eq!(plan(&[]), ["Run"]);
    }

    #[test]
    fn stages_map_to_phases_and_chips_progress() {
        let plan = ["Download", "Install"];
        assert_eq!(phase_for_stage("Downloading", &plan), 0);
        assert_eq!(phase_for_stage("Verifying", &plan), 0);
        assert_eq!(phase_for_stage("Installing", &plan), 1);
        assert_eq!(phase_for_stage("Removing", &plan), 1);
        assert_eq!(phase_for_stage("Removing", &["Remove"]), 0);
        assert_eq!(phase_for_stage("Configuring", &["Run"]), 0);
        assert_eq!(phase_chip_state(0, 1), ChipState::Done);
        assert_eq!(phase_chip_state(1, 1), ChipState::Active);
        assert_eq!(phase_chip_state(2, 1), ChipState::Idle);
        assert_eq!(final_chip_state(1, 1, true), ChipState::Done);
        assert_eq!(final_chip_state(1, 1, false), ChipState::Failed);
        assert_eq!(final_chip_state(0, 1, false), ChipState::Done);
        assert_eq!(final_chip_state(2, 1, false), ChipState::Idle);
    }

    #[test]
    fn success_completes_every_unfinished_row() {
        assert_eq!(ResultState::Pending.finish(true), ResultState::Completed);
        assert_eq!(ResultState::Working.finish(true), ResultState::Completed);
        assert_eq!(
            ResultState::NotProcessed.finish(true),
            ResultState::NotProcessed
        );
    }
}
