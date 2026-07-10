//! Add/remove xbps repositories. Listing is a read-only scan of
//! `/etc/xbps.d/*.conf` and `/usr/share/xbps.d/*.conf` for
//! `repository=` lines (same files xbps itself reads, per xbps.d(5)) —
//! no privilege needed just to look. Only entries caerus itself added
//! (tracked by which came from its own managed conf file) can be
//! removed here; anything configured some other way is shown but
//! read-only, so this can never surprise-edit someone else's setup.

use crate::backend::transaction::Transaction;
use crate::ui::dialog_util::{modal_window, present_focused};
use gtk::prelude::*;
use std::rc::Rc;

/// Must match `MANAGED_REPO_CONF` in caerus-helper/src/main.rs — the
/// one file ADDREPO/REMOVEREPO ever touch.
const MANAGED_REPO_CONF: &str = "/etc/xbps.d/90-caerus.conf";

/// (url, removable-by-us), deduplicated, sorted by URL.
fn scan_configured_repos() -> Vec<(String, bool)> {
    let mut map: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for dir in ["/etc/xbps.d", "/usr/share/xbps.d"] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "conf"))
            .collect();
        paths.sort();
        for path in paths {
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            let is_managed = path.to_str() == Some(MANAGED_REPO_CONF);
            for line in contents.lines() {
                if let Some(url) = line.strip_prefix("repository=") {
                    let url = url.trim();
                    if !url.is_empty() {
                        let entry = map.entry(url.to_string()).or_insert(false);
                        *entry = *entry || is_managed;
                    }
                }
            }
        }
    }
    map.into_iter().collect()
}

struct Inner {
    dlg: gtk::Window,
    session: Transaction,
    repos_list: gtk::ListBox,
    on_changed: Box<dyn Fn()>,
}

fn refresh(inner: &Rc<Inner>) {
    while let Some(child) = inner.repos_list.first_child() {
        inner.repos_list.remove(&child);
    }
    for (url, removable) in scan_configured_repos() {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);

        let l = gtk::Label::new(Some(&url));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        l.set_selectable(true);
        row_box.append(&l);

        if removable {
            let btn = gtk::Button::from_icon_name("user-trash-symbolic");
            btn.set_tooltip_text(Some("Remove this repository"));
            btn.add_css_class("flat");
            let inner = inner.clone();
            let url = url.clone();
            btn.connect_clicked(move |_| {
                let inner2 = inner.clone();
                crate::ui::apply_dialog::run(
                    Some(&inner.dlg),
                    &inner.session,
                    &[format!("REMOVEREPO {}", url), "SYNC".to_string()],
                    "Removing Repository",
                    move |_success| {
                        refresh(&inner2);
                        (inner2.on_changed)();
                    },
                );
            });
            row_box.append(&btn);
        } else {
            let badge = gtk::Label::new(Some("system"));
            badge.add_css_class("dim-label");
            badge.set_tooltip_text(Some("Configured outside caerus — not removable here"));
            row_box.append(&badge);
        }

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.set_child(Some(&row_box));
        inner.repos_list.append(&row);
    }
}

pub fn show(parent: Option<&gtk::Window>, session: &Transaction, on_changed: impl Fn() + 'static) {
    let (dlg, outer) = modal_window("Repositories", parent, true, (500, 380), 8);

    let repos_scroll = gtk::ScrolledWindow::new();
    repos_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    repos_scroll.set_vexpand(true);
    let repos_list = gtk::ListBox::new();
    repos_list.set_selection_mode(gtk::SelectionMode::None);
    repos_scroll.set_child(Some(&repos_list));
    outer.append(&repos_scroll);

    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_row.set_margin_top(8);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://repo-default.voidlinux.org/current"));
    entry.set_hexpand(true);
    entry.set_activates_default(true);
    let add_btn = gtk::Button::with_label("Add");
    add_btn.add_css_class("suggested-action");
    add_row.append(&entry);
    add_row.append(&add_btn);
    outer.append(&add_row);

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_margin_top(4);
    outer.append(&close_btn);

    dlg.set_default_widget(Some(&add_btn));

    let inner = Rc::new(Inner {
        dlg: dlg.clone(),
        session: session.clone(),
        repos_list: repos_list.clone(),
        on_changed: Box::new(on_changed),
    });

    {
        let inner = inner.clone();
        let entry = entry.clone();
        add_btn.connect_clicked(move |_| {
            let url = entry.text().trim().to_string();
            // A URL never legitimately contains whitespace/control
            // characters — reject rather than forward them, since this
            // string ends up as a whole line in the newline-delimited
            // helper protocol (`ADDREPO <url>`) and an embedded newline
            // would otherwise be read back as extra, unintended commands.
            if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return;
            }
            let inner2 = inner.clone();
            let entry2 = entry.clone();
            crate::ui::apply_dialog::run(
                Some(&inner.dlg),
                &inner.session,
                &[format!("ADDREPO {}", url), "SYNC".to_string()],
                "Adding Repository",
                move |_success| {
                    entry2.set_text("");
                    refresh(&inner2);
                    (inner2.on_changed)();
                },
            );
        });
    }
    {
        let dlg = dlg.clone();
        close_btn.connect_clicked(move |_| dlg.destroy());
    }

    refresh(&inner);

    present_focused(&dlg, &entry);
}
