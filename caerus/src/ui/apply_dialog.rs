//! Shows a modal progress dialog and runs `commands` on `session` — an
//! existing, caller-owned `Transaction`. The dialog does not create,
//! destroy, or take ownership of `session`; it stays alive after the
//! dialog closes, so the next call reuses it without re-authenticating.
//! Rust translation of ui/apply_dialog.{h,c}.

use crate::backend::transaction::Transaction;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

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
/// downloads/installs. Any line containing '%' is one of these
/// progress ticks; skip it entirely rather than trying to parse out
/// just the final 100% line.
fn is_percentage_line(line: &str) -> bool {
    line.contains('%')
}

pub fn run(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    done_cb: impl Fn(bool) + 'static,
) {
    let dlg = gtk::Window::new();
    dlg.set_title(Some(title));
    if let Some(p) = parent {
        dlg.set_transient_for(Some(p));
    }
    dlg.set_modal(true);
    dlg.set_default_size(520, 200);
    dlg.set_resizable(true);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.set_margin_start(14);
    outer.set_margin_end(14);
    outer.set_margin_top(14);
    outer.set_margin_bottom(14);

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

    dlg.set_child(Some(&outer));

    // We have no fine-grained fraction to report — a batch is usually
    // just one or two protocol commands regardless of how many
    // packages they cover. A steady pulse, paired with the action
    // label updating per log line, communicates progress more
    // honestly than a fraction we don't really have.
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

    let append_log: Rc<dyn Fn(&str)> = {
        let text_view = text_view.clone();
        let action_label = action_label.clone();
        Rc::new(move |line: &str| {
            if is_percentage_line(line) {
                return;
            }
            let clean = strip_log_decoration(line);
            let buf = text_view.buffer();
            let mut end = buf.end_iter();
            buf.insert(&mut end, &clean);
            buf.insert(&mut end, "\n");
            let mark = buf.get_insert();
            let mut end = buf.end_iter();
            buf.move_mark(&mark, &mut end);
            text_view.scroll_mark_onscreen(&mark);

            if line != "OK" && !clean.is_empty() {
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
    let disconnected_id = {
        let append_log = append_log.clone();
        session.connect_disconnected(move |expected| {
            append_log(if expected {
                "(privileged session ended)"
            } else {
                "(privileged session ended unexpectedly)"
            });
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
