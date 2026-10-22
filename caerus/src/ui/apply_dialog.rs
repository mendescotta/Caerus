use crate::backend::progress::{self, Event};
use crate::backend::transaction::{DisconnectReason, Transaction};
use crate::ui::dialog_util::modal_window;
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
struct PackageRow {
    label: gtk::Label,
    state: ResultState,
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
    let (dlg, outer) = modal_window(title, parent, true, (620, 420), 12);
    let status = gtk::Label::new(Some("Waiting for authorization…"));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("heading");
    outer.append(&status);
    let phase = gtk::Label::new(Some("Preparing transaction"));
    phase.set_xalign(0.0);
    phase.set_wrap(true);
    outer.append(&phase);
    let progress = gtk::ProgressBar::new();
    progress.set_show_text(true);
    progress.set_text(Some(
        "Package dependencies are included as they are processed",
    ));
    outer.append(&progress);
    let rows_box = gtk::ListBox::new();
    rows_box.set_selection_mode(gtk::SelectionMode::None);
    let rows: Rc<RefCell<BTreeMap<String, PackageRow>>> = Rc::new(RefCell::new(BTreeMap::new()));
    for name in requested_names(commands) {
        let label = gtk::Label::new(Some(&format!("{name} · Pending")));
        label.set_xalign(0.0);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        rows_box.append(&label);
        rows.borrow_mut().insert(
            name,
            PackageRow {
                label,
                state: ResultState::Pending,
            },
        );
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_widget_name("transaction-packages");
    scroll.set_max_content_height(144);
    scroll.set_propagate_natural_height(true);
    scroll.set_visible(!rows.borrow().is_empty());
    scroll.set_child(Some(&rows_box));
    outer.append(&scroll);
    let expander = gtk::Expander::new(Some("Transaction log"));
    expander.set_expanded(true);
    expander.set_vexpand(true);
    expander.set_valign(gtk::Align::Fill);
    let log_scroll = gtk::ScrolledWindow::new();
    log_scroll.set_min_content_height(180);
    log_scroll.set_vexpand(true);
    let text = gtk::TextView::new();
    text.set_editable(false);
    text.set_cursor_visible(false);
    // Never focus the read-only console: ibus can dereference an unrooted native surface.
    text.set_focusable(false);
    text.set_monospace(true);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    log_scroll.set_child(Some(&text));
    expander.set_child(Some(&log_scroll));
    outer.append(&expander);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let copy = gtk::Button::with_label("Copy log");
    let close = gtk::Button::with_label("Close");
    close.set_sensitive(false);
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
        glib::source::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if !busy.get() {
                return glib::ControlFlow::Break;
            }
            progress.pulse();
            glib::ControlFlow::Continue
        });
    }
    let active = Rc::new(RefCell::new(HashSet::<String>::new()));
    let apply_event: Rc<dyn Fn(Event)> = {
        let rows = rows.clone();
        let active = active.clone();
        let phase = phase.clone();
        let status = status.clone();
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
                }
                Event::Package {
                    name,
                    version,
                    stage,
                } => {
                    let mut rows = rows.borrow_mut();
                    let row = rows.entry(name.clone()).or_insert_with(|| {
                        let label = gtk::Label::new(None);
                        label.set_xalign(0.0);
                        label.set_wrap(true);
                        label.set_margin_top(4);
                        label.set_margin_bottom(4);
                        rows_box.append(&label);
                        scroll.set_visible(true);
                        PackageRow {
                            label,
                            state: ResultState::Pending,
                        }
                    });
                    row.state = if stage == "Completed" {
                        ResultState::Completed
                    } else {
                        ResultState::Working
                    };
                    row.label.set_text(&format!("{name} {version} · {stage}"));
                    row.label
                        .update_property(&[gtk::accessible::Property::Label(&format!(
                            "{name}, {stage}"
                        ))]);
                    active.borrow_mut().insert(name);
                    phase.set_text(&stage);
                }
                Event::Finished(success) => {
                    for name in active.borrow().iter() {
                        if let Some(row) = rows.borrow_mut().get_mut(name) {
                            row.state = row.state.finish(success);
                            row.label
                                .set_text(&format!("{name} · {}", row.state.label()));
                        }
                    }
                }
            }
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
            row.label
                .set_text(&format!("{name} · {}", row.state.label()));
            details.push_str(&format!("\n{name} · {}", row.state.label()));
        }
        progress.set_fraction(if success { 1.0 } else { 0.0 });
        progress.set_text(Some(if success {
            "Completed"
        } else {
            "Stopped — review results and log"
        }));
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
            expander.set_expanded(true);
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
        if let Some(expander) = widget.downcast_ref::<gtk::Expander>() {
            assert!(
                expander.is_expanded(),
                "transaction output should be visible"
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
    std::fs::create_dir_all(&root).unwrap();
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
    assert!(entries[0].changes.contains("foo · Completed"));
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
    fn success_completes_every_unfinished_row() {
        assert_eq!(ResultState::Pending.finish(true), ResultState::Completed);
        assert_eq!(ResultState::Working.finish(true), ResultState::Completed);
        assert_eq!(
            ResultState::NotProcessed.finish(true),
            ResultState::NotProcessed
        );
    }
}
