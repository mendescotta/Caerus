//! Pre-Apply confirmation: summarizes exactly what's about to happen
//! before the privileged batch is queued. When a real `libxbps`-computed
//! preview is available (see `backend::transaction_preview`, built from
//! `xbps_transaction_prepare()`), shows actual per-package sizes/
//! versions/actions; otherwise falls back to plain grouped name lists.

use crate::backend::package::pkg_format_size;
use crate::backend::transaction_preview::{TransAction, TransactionError, TransactionPreview};
use crate::ui::dialog_util::{
    cancel_button_row, count_pill, modal_window, present_focused, set_count, text_list_row,
};
use gtk::prelude::*;
use std::rc::Rc;

/// A section header: title + a count pill.
fn section_header(title: &str, count: usize) -> gtk::Box {
    let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header_row.set_margin_top(8);
    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("section-header");
    let pill = count_pill();
    set_count(&pill, Some(count));
    header_row.append(&header);
    header_row.append(&pill);
    header_row
}

/// A plain `ListBox` of selectable-text rows, not a wrapped
/// comma-separated label.
fn section(outer: &gtk::Box, title: &str, names: &[String]) {
    if names.is_empty() {
        return;
    }
    outer.append(&section_header(title, names.len()));

    let mut sorted = names.to_vec();
    sorted.sort();

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for name in &sorted {
        list.append(&text_list_row(name, false));
    }
    outer.append(&list);
}

/// Same shape as `section()` above, but sourced from real preview items
/// for one `TransAction` bucket — each row shows version + real size.
/// Purge's orphan-removal cascade shows up here for free, as ordinary
/// `Remove` items alongside whatever the user directly marked.
fn preview_section(
    outer: &gtk::Box,
    title: &str,
    action: TransAction,
    preview: &TransactionPreview,
) {
    let items: Vec<_> = preview
        .items
        .iter()
        .filter(|i| i.action == action)
        .collect();
    if items.is_empty() {
        return;
    }
    outer.append(&section_header(title, items.len()));

    let mut sorted = items;
    sorted.sort_by(|a, b| a.pkgname.cmp(&b.pkgname));

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for item in &sorted {
        let size = if item.download_size > 0 {
            format!(
                "{} (download {})",
                pkg_format_size(item.installed_size),
                pkg_format_size(item.download_size)
            )
        } else {
            pkg_format_size(item.installed_size)
        };
        let label = format!("{} {} — {}", item.pkgname, item.pkgver, size);
        list.append(&text_list_row(&label, false));
    }
    outer.append(&list);
}

fn totals_footer(outer: &gtk::Box, preview: &TransactionPreview) {
    let footer = gtk::Label::new(Some(&format!(
        "Download {} \u{2022} Installed size change {} \u{2022} Freed {}",
        pkg_format_size(preview.total_download_size),
        pkg_format_size(preview.total_installed_size),
        pkg_format_size(preview.total_removed_size),
    )));
    footer.set_xalign(0.0);
    footer.set_wrap(true);
    footer.add_css_class("dim-label");
    footer.set_margin_top(8);
    outer.append(&footer);
}

fn error_banner(outer: &gtk::Box, err: &TransactionError) {
    let heading = gtk::Label::new(Some(&format!(
        "libxbps dry-run reports a problem: {}",
        err.summary()
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    heading.add_css_class("error");
    outer.append(&heading);

    let details = err.details();
    if !details.is_empty() {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        for d in details {
            list.append(&text_list_row(d, true));
        }
        outer.append(&list);
    }

    let note = gtk::Label::new(Some(
        "The list below is still the app's own best-effort summary of what you marked; \
         the real xbps tools may behave differently from what libxbps predicted here.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    note.set_margin_top(4);
    note.set_margin_bottom(4);
    outer.append(&note);
}

/// Shows a summary dialog and calls `cb(true)` if the user confirms,
/// `cb(false)` if they cancel. Never called with everything empty — the
/// caller already returns early in that case.
///
/// `preview`: `Some(Ok(p))` renders real per-package data, `Some(Err(e))`
/// shows the libxbps-reported problem above the name-list fallback,
/// `None` renders the plain name-list summary alone.
pub fn confirm(
    parent: Option<&gtk::Window>,
    installs: &[String],
    upgrades: &[String],
    removes: &[String],
    purges: &[String],
    preview: Option<Result<TransactionPreview, TransactionError>>,
    cb: impl Fn(bool) + 'static,
) {
    let (dlg, outer) = modal_window("Confirm Changes", parent, true, (480, -1), 4);

    // With a real preview, count what the transaction will actually
    // touch (including deps libxbps pulled in), not just what was marked.
    let total = match &preview {
        Some(Ok(p)) => p.items.len(),
        _ => installs.len() + upgrades.len() + removes.len() + purges.len(),
    };
    let heading = gtk::Label::new(Some(&format!(
        "About to apply changes to {} package{}:",
        total,
        if total == 1 { "" } else { "s" },
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    if let Some(Err(err)) = &preview {
        error_banner(&outer, err);
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(360);
    scroll.set_vexpand(true);
    scroll.set_margin_top(6);

    let sections = gtk::Box::new(gtk::Orientation::Vertical, 2);
    if let Some(Ok(p)) = &preview {
        preview_section(&sections, "Download", TransAction::Download, p);
        preview_section(&sections, "Install", TransAction::Install, p);
        preview_section(&sections, "Reinstall", TransAction::Reinstall, p);
        preview_section(&sections, "Update", TransAction::Update, p);
        preview_section(&sections, "Configure", TransAction::Configure, p);
        preview_section(&sections, "Remove", TransAction::Remove, p);
        preview_section(&sections, "Hold", TransAction::Hold, p);
    } else {
        section(&sections, "Install", installs);
        section(&sections, "Upgrade", upgrades);
        section(&sections, "Remove", removes);
        section(
            &sections,
            "Purge (also removes orphaned dependencies)",
            purges,
        );
    }
    scroll.set_child(Some(&sections));
    outer.append(&scroll);

    if let Some(Ok(p)) = &preview {
        totals_footer(&outer, p);
    }

    let (btn_box, cancel_btn) = cancel_button_row(10);

    let had_error = matches!(preview, Some(Err(_)));
    let destructive = !removes.is_empty() || !purges.is_empty() || had_error;
    let apply_label = if had_error { "Apply Anyway" } else { "Apply" };
    let apply_btn = gtk::Button::with_label(apply_label);
    if destructive {
        apply_btn.add_css_class("destructive-action");
    } else {
        apply_btn.add_css_class("suggested-action");
    }
    btn_box.append(&apply_btn);
    outer.append(&btn_box);

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
        let cb = cb;
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, if destructive { &cancel_btn } else { &apply_btn });
}
