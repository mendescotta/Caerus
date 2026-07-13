//! Shows a modal progress dialog and runs `commands` on `session` — an
//! existing, caller-owned `Transaction`. The dialog does not create,
//! destroy, or take ownership of `session`; it stays alive after the
//! dialog closes, so the next call reuses it without re-authenticating.
//! Rust translation of ui/apply_dialog.{h,c}.

use crate::backend::transaction::{DisconnectReason, Transaction};
use crate::ui::dialog_util::modal_window;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// How many packages this batch actually names, summed across every
/// command that takes package-name arguments (INSTALL/REMOVE/PURGE/HOLD/
/// UNHOLD) — used only to show a "package N of M" counter; commands with
/// no package-name concept (SYNC, UPGRADE, ORPHANS, ...) don't contribute.
fn count_target_packages(commands: &[String]) -> usize {
    commands
        .iter()
        .filter(|c| {
            for verb in ["INSTALL ", "REMOVE ", "PURGE ", "HOLD ", "UNHOLD "] {
                if c.starts_with(verb) {
                    return true;
                }
            }
            false
        })
        .map(|c| c.split_whitespace().skip(1).count())
        .sum()
}

/// The helper's own log lines look like:
///   LOG [*] Updating repository `https://...` ...
///   LOG [*] Installing `foo-1.0_1` ... 42%
///   OK
/// "LOG " and "[*]" are stripped for display only; the underlying
/// protocol line is untouched everywhere else.
fn strip_log_decoration(line: &str) -> String {
    let mut s = line;
    if let Some(rest) = s.strip_prefix("LOG ") {
        s = rest;
    }
    if let Some(rest) = s.strip_prefix("[*] ") {
        s = rest;
    } else if let Some(rest) = s.strip_prefix("[*]") {
        s = rest;
    }
    s.trim().to_string()
}

/// The helper emits one line per percentage tick while a package
/// downloads/installs/verifies — xbps-install's own progress format is
/// `%s: [%s %d%%] %s ETA: %s` (confirmed against its format strings),
/// i.e. a plain integer immediately followed by a single `%` somewhere
/// in the line. Returns the *last* such `<digits>%` run in the line
/// (the ETA/size fields after it don't contain '%', but this is the
/// safer direction regardless).
fn extract_percentage(line: &str) -> Option<u8> {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate().rev() {
        if b != b'%' {
            continue;
        }
        let mut start = i;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == i {
            continue; // '%' with no digits immediately before it
        }
        if let Ok(pct) = line[start..i].parse::<u32>() {
            return Some(pct.min(100) as u8);
        }
    }
    None
}

pub fn run(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    done_cb: impl Fn(bool) + 'static,
) {
    let (dlg, outer) = modal_window(title, parent, true, (520, 200), 8);

    let total_pkgs = count_target_packages(commands);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spinner = gtk::Spinner::new();
    spinner.start();
    let status_label = gtk::Label::new(Some("Applying changes\u{2026}"));
    status_label.set_xalign(0.0);
    status_label.set_hexpand(true);
    header.append(&spinner);
    header.append(&status_label);
    outer.append(&header);

    let action_label = gtk::Label::new(Some("\u{2026}"));
    action_label.set_xalign(0.0);
    action_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    action_label.add_css_class("dim-label");
    outer.append(&action_label);

    // Only shown when the batch actually names packages (INSTALL/REMOVE/
    // PURGE/HOLD/UNHOLD) and there's more than one — for a single
    // package, or a command with no package-name concept at all (SYNC,
    // full UPGRADE, ...), the action line above already says enough.
    let progress_count_label = gtk::Label::new(None);
    progress_count_label.set_xalign(0.0);
    progress_count_label.add_css_class("dim-label");
    progress_count_label.set_visible(total_pkgs > 1);
    outer.append(&progress_count_label);

    let progress_bar = gtk::ProgressBar::new();
    outer.append(&progress_bar);

    let expander = gtk::Expander::new(Some("Details"));
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_size_request(-1, 220);
    scroll.set_vexpand(true);
    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_monospace(true);
    text_view.set_wrap_mode(gtk::WrapMode::WordChar);
    text_view.set_margin_start(4);
    text_view.set_margin_end(4);
    scroll.set_child(Some(&text_view));
    expander.set_child(Some(&scroll));
    outer.append(&expander);

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_sensitive(false);
    outer.append(&close_btn);

    // Starts as an indeterminate pulse — commands like SYNC/HOLD/ORPHANS
    // report no percentage at all, so there's nothing better to show
    // until (if ever) a real one shows up. The moment `append_log` sees
    // an actual percentage tick (see `extract_percentage`), it flips
    // `pulsing` to false, which stops this timer for good and switches
    // the bar to a real fraction from then on.
    let pulsing = Rc::new(Cell::new(true));
    {
        let pulsing = pulsing.clone();
        let progress_bar = progress_bar.clone();
        glib::source::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if !pulsing.get() {
                return glib::ControlFlow::Break;
            }
            progress_bar.pulse();
            glib::ControlFlow::Continue
        });
    }

    // Heuristic package counter: xbps prints one non-percentage action
    // line ("Installing `foo-1.0_1' ...") per package, so counting
    // *distinct, consecutive* action-line changes approximates "package N
    // of total" without parsing xbps's exact log format. Capped at
    // `total_pkgs` since the heuristic can occasionally over-count (e.g.
    // an intermediate status line for the same package that happens to
    // differ from the last one shown).
    let seen_count = Rc::new(Cell::new(0usize));
    let last_action: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    let append_log: Rc<dyn Fn(&str)> = {
        let text_view = text_view.clone();
        let action_label = action_label.clone();
        let progress_count_label = progress_count_label.clone();
        let pulsing = pulsing.clone();
        let progress_bar = progress_bar.clone();
        Rc::new(move |line: &str| {
            if let Some(pct) = extract_percentage(line) {
                // A real progress tick — switch the bar from pulsing to
                // an actual fraction. Still not logged to the Details
                // pane individually; there can be dozens of these per
                // file and the surrounding "Installing `foo' ..." line
                // already says what's happening.
                pulsing.set(false);
                progress_bar.set_fraction(pct as f64 / 100.0);
                return;
            }
            let clean = strip_log_decoration(line);
            let buf = text_view.buffer();
            let mut end = buf.end_iter();
            buf.insert(&mut end, &clean);
            buf.insert(&mut end, "\n");
            let mark = buf.get_insert();
            let end = buf.end_iter();
            buf.move_mark(&mark, &end);
            text_view.scroll_mark_onscreen(&mark);

            if line != "OK" && !clean.is_empty() {
                if total_pkgs > 1 && *last_action.borrow() != clean {
                    let n = (seen_count.get() + 1).min(total_pkgs);
                    seen_count.set(n);
                    progress_count_label.set_text(&format!("Package {} of {}", n, total_pkgs));
                }
                *last_action.borrow_mut() = clean.clone();
                action_label.set_text(&clean);
            }
        })
    };

    // This dialog's three listeners live on the shared, long-lived
    // `session` — not on the dialog itself — so they must be detached
    // once this batch is done, or they'd keep firing (against a
    // destroyed dialog's widgets) on every future batch for the rest
    // of the app's lifetime. `finished` always fires exactly once per
    // batch (success, a failed step, or a mid-batch disconnect all
    // route through `end_of_batch`/`emit_finished`), so it's the one
    // reliable place to detach all three.
    let log_id = {
        let append_log = append_log.clone();
        session.connect_log(move |line| append_log(line))
    };
    let auth_failed = Rc::new(Cell::new(false));
    let disconnected_id = {
        let append_log = append_log.clone();
        let auth_failed = auth_failed.clone();
        session.connect_disconnected(move |reason| match reason {
            DisconnectReason::Expected => append_log("(privileged session ended)"),
            DisconnectReason::Unexpected => append_log("(privileged session ended unexpectedly)"),
            DisconnectReason::AuthFailed => {
                auth_failed.set(true);
                append_log(
                    "(could not authenticate as root — no polkit authentication agent \
                     responded; a bare window manager setup may need one added to its \
                     startup, e.g. polkit-gnome, lxqt-policykit, polkit-mate)",
                );
            }
        })
    };
    {
        let pulsing = pulsing.clone();
        let spinner = spinner.clone();
        let progress_bar = progress_bar.clone();
        let status_label = status_label.clone();
        let close_btn = close_btn.clone();
        let done_cb = Rc::new(done_cb);
        let session_for_cleanup = session.clone();
        let finished_id_cell: Rc<Cell<u64>> = Rc::new(Cell::new(0));
        let finished_id_cell2 = finished_id_cell.clone();
        let finished_id = session.connect_finished(move |success| {
            pulsing.set(false);
            spinner.stop();
            progress_bar.set_fraction(1.0);
            status_label.set_text(if success {
                "Finished successfully."
            } else if auth_failed.get() {
                "Could not authenticate as root — see details below."
            } else {
                "Finished with errors."
            });
            close_btn.set_sensitive(true);
            done_cb(success);

            session_for_cleanup.disconnect_log(log_id);
            session_for_cleanup.disconnect_disconnected(disconnected_id);
            session_for_cleanup.disconnect_finished(finished_id_cell2.get());
        });
        finished_id_cell.set(finished_id);
    }

    {
        let dlg_c = dlg.clone();
        close_btn.connect_clicked(move |_| dlg_c.destroy());
    }
    {
        // The Close button is disabled while a batch is in flight, but
        // that alone doesn't stop the window manager's own close
        // affordance. The underlying xbps operation would still run to
        // completion regardless (it's driven by the long-lived
        // session, not the dialog) — but nothing would clear marks or
        // reload afterward if the dialog (and its `finished` listener)
        // disappeared early. Block the close request outright while
        // busy, matching the disabled button.
        let close_btn = close_btn.clone();
        dlg.connect_close_request(move |_| {
            if close_btn.is_sensitive() {
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            }
        });
    }

    for cmd in commands {
        session.add_command(cmd);
    }

    dlg.present();
    session.run_async();
}
