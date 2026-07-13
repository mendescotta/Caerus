//! Switches between packages that provide the same "alternative"
//! (e.g. multiple lua/cc/editor implementations providing the same
//! symlinked binaries) — a GUI over `xbps-alternatives`.
//!
//! Listing is read-only and runs directly from the unprivileged GUI
//! process, same rationale as `file_owner_dialog`. Actually switching
//! a group's active provider rewrites symlinks under `/usr` and goes
//! through the privileged helper's `ALTERNATIVE` command.

use crate::backend::transaction::Transaction;
use crate::ui::dialog_util::{modal_window, present_focused};
use gtk::prelude::*;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

/// `xbps-alternatives -l` (no group filter) only reports each group's
/// *current* provider, not the full candidate list — the full list per
/// group only comes back when a specific group is requested via `-g`
/// (confirmed empirically: compare plain `-l` against `-g <group> -l`
/// for the same group). So this overview is only good for building the
/// left-hand group list; `fetch_candidates` below does the per-group
/// follow-up query, lazily, once a group is actually selected.
fn fetch_overview() -> Vec<(String, String)> {
    let Ok(output) = Command::new("xbps-alternatives").arg("-l").output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    let mut current_group: Option<String> = None;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            current_group = Some(line.trim().to_string());
            continue;
        }
        let leading = line.len() - line.trim_start().len();
        if leading == 1 {
            if let Some(group) = current_group.take() {
                let rest = line.trim_start().trim_start_matches("- ").trim();
                let provider = rest.trim_end_matches("(current)").trim().to_string();
                out.push((group, provider));
            }
        }
    }
    out
}

/// (provider pkgname, is_current) for every candidate in `group`.
fn fetch_candidates(group: &str) -> Vec<(String, bool)> {
    let Ok(output) = Command::new("xbps-alternatives")
        .arg("-g")
        .arg(group)
        .arg("-l")
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() || !line.starts_with(' ') {
            continue;
        }
        let leading = line.len() - line.trim_start().len();
        if leading == 1 {
            let rest = line.trim_start().trim_start_matches("- ").trim();
            let is_current = rest.ends_with("(current)");
            let provider = rest.trim_end_matches("(current)").trim().to_string();
            out.push((provider, is_current));
        }
    }
    out
}

struct Inner {
    dlg: gtk::Window,
    session: Transaction,
    groups_list: gtk::ListBox,
    providers_list: gtk::ListBox,
    providers_header: gtk::Label,
    selected_group: RefCell<Option<String>>,
}

fn refresh_groups(inner: &Rc<Inner>) {
    let previously_selected = inner.selected_group.borrow().clone();

    while let Some(child) = inner.groups_list.first_child() {
        inner.groups_list.remove(&child);
    }
    let overview = fetch_overview();
    let mut restore_row: Option<gtk::ListBoxRow> = None;
    for (group, current) in &overview {
        let l = gtk::Label::new(Some(&format!("{}  ({})", group, current)));
        l.set_xalign(0.0);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        l.set_margin_start(8);
        l.set_margin_end(8);
        l.set_margin_top(5);
        l.set_margin_bottom(5);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        // NOTE: relies on group names never containing a tab; used to
        // recover the bare group name from the row on selection
        // without a parallel index Vec.
        unsafe {
            row.set_data("group-name", group.clone());
        }
        inner.groups_list.append(&row);
        if Some(group) == previously_selected.as_ref() {
            restore_row = Some(row);
        }
    }

    match restore_row {
        Some(row) => inner.groups_list.select_row(Some(&row)),
        None => {
            *inner.selected_group.borrow_mut() = None;
            refresh_providers(inner);
        }
    }
}

fn refresh_providers(inner: &Rc<Inner>) {
    while let Some(child) = inner.providers_list.first_child() {
        inner.providers_list.remove(&child);
    }
    let Some(group) = inner.selected_group.borrow().clone() else {
        inner
            .providers_header
            .set_text("Select a group on the left.");
        return;
    };
    inner
        .providers_header
        .set_text(&format!("Providers for \u{201c}{}\u{201d}:", group));

    for (provider, is_current) in fetch_candidates(&group) {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);
        let l = gtk::Label::new(Some(&provider));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        row_box.append(&l);

        if is_current {
            let badge = gtk::Label::new(Some("current"));
            badge.add_css_class("dim-label");
            row_box.append(&badge);
        } else {
            let btn = gtk::Button::with_label("Set Active");
            btn.add_css_class("suggested-action");
            let inner2 = inner.clone();
            let group2 = group.clone();
            let provider2 = provider.clone();
            btn.connect_clicked(move |_| {
                let cmd = format!("ALTERNATIVE {} {}", group2, provider2);
                let inner3 = inner2.clone();
                crate::ui::apply_dialog::run(
                    Some(&inner2.dlg),
                    &inner2.session,
                    &[cmd],
                    "Switching Alternative",
                    move |_success| refresh_groups(&inner3),
                );
            });
            row_box.append(&btn);
        }

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.set_child(Some(&row_box));
        inner.providers_list.append(&row);
    }
}

pub fn show(parent: Option<&gtk::Window>, session: &Transaction) {
    let (dlg, outer) = modal_window("Alternatives", parent, true, (520, 420), 8);

    let split = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    split.set_vexpand(true);

    let groups_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    groups_col.set_width_request(200);
    let groups_header = gtk::Label::new(Some("GROUPS"));
    groups_header.set_xalign(0.0);
    groups_header.add_css_class("section-header");
    groups_col.append(&groups_header);
    let groups_scroll = gtk::ScrolledWindow::new();
    groups_scroll.set_vexpand(true);
    let groups_list = gtk::ListBox::new();
    groups_list.set_selection_mode(gtk::SelectionMode::Single);
    groups_scroll.set_child(Some(&groups_list));
    groups_col.append(&groups_scroll);
    split.append(&groups_col);

    split.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let providers_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    providers_col.set_hexpand(true);
    let providers_header = gtk::Label::new(Some("Select a group on the left."));
    providers_header.set_xalign(0.0);
    providers_header.add_css_class("section-header");
    providers_col.append(&providers_header);
    let providers_scroll = gtk::ScrolledWindow::new();
    providers_scroll.set_vexpand(true);
    let providers_list = gtk::ListBox::new();
    providers_list.set_selection_mode(gtk::SelectionMode::None);
    providers_scroll.set_child(Some(&providers_list));
    providers_col.append(&providers_scroll);
    split.append(&providers_col);

    outer.append(&split);

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_margin_top(8);
    outer.append(&close_btn);

    let inner = Rc::new(Inner {
        dlg: dlg.clone(),
        session: session.clone(),
        groups_list: groups_list.clone(),
        providers_list,
        providers_header,
        selected_group: RefCell::new(None),
    });

    {
        let inner = inner.clone();
        groups_list.connect_row_selected(move |_, row| {
            let group: Option<String> = row.and_then(|r| unsafe {
                r.data::<String>("group-name").map(|p| p.as_ref().clone())
            });
            *inner.selected_group.borrow_mut() = group;
            refresh_providers(&inner);
        });
    }
    {
        let dlg = dlg.clone();
        close_btn.connect_clicked(move |_| dlg.destroy());
    }

    refresh_groups(&inner);

    // Without an explicit focus target, GTK auto-focuses the first
    // focusable widget on present — the first row of `groups_list` here
    // — which also auto-selects that group (firing `refresh_providers`
    // for a group the user never actually clicked). Same class of
    // unwanted-auto-focus issue `present_focused` exists to avoid
    // elsewhere in this project's dialogs.
    present_focused(&dlg, &close_btn);
}
