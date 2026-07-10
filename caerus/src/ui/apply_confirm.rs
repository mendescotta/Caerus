//! Pre-Apply confirmation: summarizes exactly what's about to happen
//! (install/upgrade/remove/purge, with names) before the privileged
//! batch is queued. Only `deps_confirm` warned about consequences
//! before this — plain removals/purges/upgrades went straight to the
//! Apply progress dialog with no "are you sure" step, which is
//! especially risky for Purge (it can cascade into removing orphaned
//! dependencies too). Same manual-widget style as `deps_confirm.rs`.

use gtk::prelude::*;
use std::rc::Rc;

/// Same look as the Dependencies/Reverse Dependencies lists in the
/// detail pane (see `detail_pane::populate`): a plain `ListBox` of
/// selectable-text rows, not a wrapped comma-separated label — easier
/// to scan and to select one name out of a long list.
fn section(outer: &gtk::Box, title: &str, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let header = gtk::Label::new(Some(&format!("{} ({})", title, names.len())));
    header.set_xalign(0.0);
    header.add_css_class("section-header");
    header.set_margin_top(8);
    outer.append(&header);

    let mut sorted = names.to_vec();
    sorted.sort();

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for name in &sorted {
        let l = gtk::Label::new(Some(name));
        l.set_xalign(0.0);
        l.set_selectable(true);
        l.set_margin_start(8);
        l.set_margin_top(4);
        l.set_margin_bottom(4);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        list.append(&row);
    }
    outer.append(&list);
}

/// Shows a summary dialog and calls `cb(true)` if the user confirms,
/// `cb(false)` if they cancel (button or window-close, same as
/// `deps_confirm`). Never called with everything empty — the caller
/// (`window.rs::on_apply_clicked`) already returns early in that case.
pub fn confirm(
    parent: Option<&gtk::Window>,
    installs: &[String],
    upgrades: &[String],
    removes: &[String],
    purges: &[String],
    cb: impl Fn(bool) + 'static,
) {
    let dlg = gtk::Window::new();
    dlg.set_title(Some("Confirm Changes"));
    if let Some(p) = parent {
        dlg.set_transient_for(Some(p));
    }
    dlg.set_modal(true);
    dlg.set_resizable(true);
    dlg.set_default_size(460, -1);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    outer.set_margin_start(16);
    outer.set_margin_end(16);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);

    let total = installs.len() + upgrades.len() + removes.len() + purges.len();
    let heading = gtk::Label::new(Some(&format!(
        "About to apply changes to {} package{}:",
        total,
        if total == 1 { "" } else { "s" },
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(360);
    scroll.set_vexpand(true);
    scroll.set_margin_top(6);

    let sections = gtk::Box::new(gtk::Orientation::Vertical, 2);
    section(&sections, "Install", installs);
    section(&sections, "Upgrade", upgrades);
    section(&sections, "Remove", removes);
    section(&sections, "Purge (also removes orphaned dependencies)", purges);
    scroll.set_child(Some(&sections));
    outer.append(&scroll);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(10);
    let cancel_btn = gtk::Button::with_label("Cancel");
    let apply_btn = gtk::Button::with_label("Apply");
    // Removing/purging anything makes this the riskier action of the
    // two possible framings; installs/upgrades alone stay "suggested".
    let destructive = !removes.is_empty() || !purges.is_empty();
    if destructive {
        apply_btn.add_css_class("destructive-action");
    } else {
        apply_btn.add_css_class("suggested-action");
    }
    btn_box.append(&cancel_btn);
    btn_box.append(&apply_btn);
    outer.append(&btn_box);

    dlg.set_child(Some(&outer));
    // Same default-widget/focus target: Enter activates Cancel rather
    // than a destructive Apply when removals/purges are involved.
    dlg.set_default_widget(Some(if destructive { &cancel_btn } else { &apply_btn }));

    let cb = Rc::new(cb);

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            cb(false);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        apply_btn.connect_clicked(move |_| {
            cb(true);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    dlg.present();
    // Without this, GTK hands initial keyboard focus to the first
    // focusable widget in the window — one of the selectable-text list
    // rows above — which shows up as that row's entire text looking
    // "pre-selected" the instant the dialog opens. Explicitly focusing
    // a button avoids it.
    if destructive {
        cancel_btn.grab_focus();
    } else {
        apply_btn.grab_focus();
    }
}
