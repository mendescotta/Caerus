//! "Transaction History" — a read-only view over the batch records
//! `backend::history` persists (one row per Apply batch or maintenance
//! action). Rollback is explicitly out of scope: this only shows what
//! happened, newest first.

use crate::backend::history;
use crate::ui::dialog_util::{modal_window, present_focused};
use gtk::prelude::*;

fn history_row(entry: &history::HistoryEntry) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.set_margin_start(4);
    row_box.set_margin_end(4);
    row_box.set_margin_top(6);
    row_box.set_margin_bottom(6);

    let icon = gtk::Image::from_icon_name(if entry.success {
        "object-select-symbolic"
    } else {
        "dialog-warning-symbolic"
    });
    row_box.append(&icon);

    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let cmd_label = gtk::Label::new(Some(&entry.commands));
    cmd_label.set_xalign(0.0);
    cmd_label.set_selectable(true);
    cmd_label.set_wrap(true);
    text_box.append(&cmd_label);

    let time_label = gtk::Label::new(Some(&entry.timestamp));
    time_label.set_xalign(0.0);
    time_label.add_css_class("dim-label");
    text_box.append(&time_label);

    text_box.set_hexpand(true);
    row_box.append(&text_box);

    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&row_box));
    row
}

pub fn show(parent: Option<&gtk::Window>) {
    let (dlg, outer) = modal_window("Transaction History", parent, true, (520, 420), 8);

    let entries = history::load();

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    if entries.is_empty() {
        let placeholder = gtk::Label::new(Some("No transactions recorded yet."));
        placeholder.add_css_class("dim-label");
        placeholder.set_margin_top(24);
        list.set_placeholder(Some(&placeholder));
    } else {
        for entry in &entries {
            list.append(&history_row(entry));
        }
    }
    scroll.set_child(Some(&list));
    outer.append(&scroll);

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_margin_top(4);
    outer.append(&close_btn);

    dlg.set_default_widget(Some(&close_btn));

    {
        let dlg = dlg.clone();
        close_btn.connect_clicked(move |_| dlg.destroy());
    }

    present_focused(&dlg, &close_btn);
}
