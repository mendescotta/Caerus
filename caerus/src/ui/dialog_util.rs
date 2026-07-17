//! Shared scaffolding for the project's many small modal utility windows
//! (`apply_dialog`, `apply_confirm`, `deps_confirm`, `remove_confirm`,
//! `alternatives_dialog`, `repo_manager`, `file_owner_dialog`, plus the
//! repository-rename and keyboard-shortcuts dialogs). Every one of them
//! independently built the same title/transient-for/modal/margins
//! boilerplate, the same "selectable-text list row" shape, and the same
//! present-then-steal-focus-back-from-the-first-selectable-widget
//! workaround — this module gives them one shared implementation instead
//! of N slightly-drifting copies.

use gtk::prelude::*;

/// Builds a modal window's outer chrome — title, optional transient
/// parent, resizability, default size, and an outer vertical `Box` with
/// the 16px margins every dialog in this project uses — and returns both
/// so the caller fills in `outer` with its own content.
///
/// Also wires Escape to close the window: a plain `gtk::Window` (as
/// opposed to a real `gtk::Dialog`) doesn't get this for free, and none
/// of this project's hand-built dialogs had it before. This routes
/// through the same `close-request` signal a window-manager close button
/// would use, so a caller that already overrides `connect_close_request`
/// (e.g. `apply_dialog`, to block closing mid-batch, or the confirm
/// dialogs, to treat it as Cancel) keeps that behavior unchanged — Escape
/// just becomes another way to trigger it.
pub fn modal_window(
    title: &str,
    parent: Option<&gtk::Window>,
    resizable: bool,
    default_size: (i32, i32),
    spacing: i32,
) -> (gtk::Window, gtk::Box) {
    let dlg = gtk::Window::new();
    dlg.set_title(Some(title));
    if let Some(p) = parent {
        dlg.set_transient_for(Some(p));
    }
    dlg.set_modal(true);
    dlg.set_resizable(resizable);
    dlg.set_default_size(default_size.0, default_size.1);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, spacing);
    outer.set_margin_start(16);
    outer.set_margin_end(16);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    dlg.set_child(Some(&outer));

    let key = gtk::EventControllerKey::new();
    let dlg_weak = dlg.downgrade();
    key.connect_key_pressed(move |_, keyval, _keycode, _state| {
        if keyval == gtk::gdk::Key::Escape {
            if let Some(d) = dlg_weak.upgrade() {
                d.close();
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    dlg.add_controller(key);

    (dlg, outer)
}

/// A single selectable-text row for a `gtk::ListBox` — the shape used
/// throughout the app for dependency/reverse-dependency/affected-package/
/// search-result lists. `wrap` is only needed for content that can run
/// long on one line (file paths, query results); the plain package-name
/// lists elsewhere leave it off, relying on the list's own scrolling.
pub fn text_list_row(text: &str, wrap: bool) -> gtk::ListBoxRow {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_selectable(true);
    l.set_wrap(wrap);
    l.set_margin_start(8);
    l.set_margin_top(4);
    l.set_margin_bottom(4);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&l));
    row
}

/// Builds a right-aligned "Close" button, appends it to `outer`, and
/// wires it to destroy `dlg` — the shape every read-only informational
/// dialog in this project (Repositories, Alternatives, Find File Owner,
/// Transaction History) uses for its one and only action.
pub fn close_button(outer: &gtk::Box, dlg: &gtk::Window, margin_top: i32) -> gtk::Button {
    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_margin_top(margin_top);
    outer.append(&close_btn);
    let dlg = dlg.clone();
    close_btn.connect_clicked(move |_| dlg.destroy());
    close_btn
}

/// Builds a right-aligned button row starting with a `Cancel` button —
/// the shape every confirmation dialog in this project (Apply,
/// dependency/removal-impact confirmations, Add Repository) uses for its
/// button row. Doesn't append the row to `outer` or wire `Cancel`'s
/// click itself: callers still need to append their own primary (and
/// sometimes secondary, e.g. `apply_confirm`'s "Copy Dry-Run Output")
/// button(s) after `Cancel` before appending the finished row, and each
/// has its own idea of what "cancel" should do (just `dlg.destroy()`,
/// or also running a callback with `false`).
pub fn cancel_button_row(margin_top: i32) -> (gtk::Box, gtk::Button) {
    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(margin_top);
    let cancel_btn = gtk::Button::with_label("Cancel");
    btn_box.append(&cancel_btn);
    (btn_box, cancel_btn)
}

/// A count pill for section headers/expanders/buttons — the "count-pill"
/// CSS class this project uses everywhere a number needs to render as a
/// rounded badge rather than "(N)"/"· N" text (see the 0.5 design-language
/// rule: counts are always pills). Starts hidden until a count is known.
pub fn count_pill() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("count-pill");
    l.set_visible(false);
    l.set_valign(gtk::Align::Center);
    l
}

/// Updates a pill built by [`count_pill`]: `None` hides it (nothing to
/// show), `Some(n)` sets its text and makes it visible.
pub fn set_count(pill: &gtk::Label, count: Option<usize>) {
    match count {
        Some(n) => {
            pill.set_text(&n.to_string());
            pill.set_visible(true);
        }
        None => pill.set_visible(false),
    }
}

/// Presents `dlg` and immediately moves keyboard focus to `widget`.
///
/// Without the explicit `grab_focus`, GTK hands initial keyboard focus to
/// the first focusable widget in the window — often a selectable-text
/// list row — which shows up as that row's entire text looking
/// "pre-selected" the instant the dialog opens. This is orthogonal to
/// `set_default_widget` (which only controls what Enter activates); most
/// callers want both pointed at the same button, but a couple (the
/// rename/find-owner/add-repo dialogs) focus a text entry instead.
pub fn present_focused(dlg: &gtk::Window, widget: &impl IsA<gtk::Widget>) {
    dlg.present();
    widget.grab_focus();
}

/// Runs `cmd.output()` on a background thread and hands the result to
/// `on_done` back on the GTK main thread — the same mpsc +
/// `timeout_add_local` polling shape `PackageStore`'s worker replies
/// use. For the read-only one-shot subprocess queries some dialogs make
/// (`xbps-query -o`, `vkpurge list`, `xbps-alternatives -l`), which
/// previously blocked the main thread for the subprocess's lifetime.
pub fn run_command_async(
    mut cmd: std::process::Command,
    on_done: impl FnOnce(Result<std::process::Output, String>) + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output().map_err(|e| e.to_string()));
    });

    let mut on_done = Some(on_done);
    glib::source::timeout_add_local(std::time::Duration::from_millis(15), move || {
        match rx.try_recv() {
            Ok(result) => {
                if let Some(on_done) = on_done.take() {
                    on_done(result);
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            // Sender dropped without sending — thread panicked; nothing
            // to deliver.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}
