use crate::backend::history;
use crate::ui::dialog_util::{close_button, modal_window, present_focused};
use crate::ui::widgets::{set_pill, status_pill, EmptyState, PillKind};
use gtk::prelude::*;
use std::rc::Rc;

pub fn show(parent: Option<&gtk::Window>) {
    let (dlg, outer) = modal_window("Transaction History", parent, true, (680, 540), 12);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search packages, versions, dates or logs…"));
    search.update_property(&[gtk::accessible::Property::Label(
        "Search transaction history",
    )]);
    outer.append(&search);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let empty = EmptyState::new(
        "document-open-recent-symbolic",
        "No matching transactions",
        "New transactions include logs and version details.",
    );
    list.set_placeholder(Some(empty.widget()));
    let entries = Rc::new(history::load());
    let rebuild = {
        let list = list.clone();
        move |query: &str| {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for entry in entries.iter().filter(|e| e.matches(query)) {
                let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
                content.set_margin_top(10);
                content.set_margin_bottom(10);
                content.set_margin_start(10);
                content.set_margin_end(10);
                let title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let pill = status_pill("", PillKind::Done);
                if entry.success {
                    set_pill(&pill, "Completed", PillKind::Done);
                } else {
                    set_pill(&pill, "Failed or partially completed", PillKind::Failed);
                }
                title.append(&pill);
                let when = gtk::Label::new(Some(&entry.timestamp));
                when.add_css_class("dim-label");
                title.append(&when);
                content.append(&title);
                let summary = gtk::Label::new(Some(&entry.summary()));
                summary.set_xalign(0.0);
                summary.set_wrap(true);
                summary.set_selectable(true);
                content.append(&summary);
                let details = format!("{}\n\n{}", entry.changes, entry.log);
                if !details.trim().is_empty() {
                    let expander = gtk::Expander::new(Some("Package changes and log"));
                    let label = gtk::Label::new(Some(&details));
                    label.set_xalign(0.0);
                    label.set_wrap(true);
                    label.set_selectable(true);
                    label.add_css_class("monospace");
                    expander.set_child(Some(&label));
                    content.append(&expander);
                    let copy = gtk::Button::with_label("Copy details");
                    copy.set_halign(gtk::Align::Start);
                    copy.connect_clicked(move |b| b.clipboard().set_text(&details));
                    content.append(&copy);
                }
                list.append(&content);
            }
        }
    };
    rebuild("");
    search.connect_search_changed(move |s| rebuild(&s.text()));
    scroll.set_child(Some(&list));
    outer.append(&scroll);
    close_button(&outer, &dlg, 4);
    present_focused(&dlg, &search);
}
