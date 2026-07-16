//! Add/remove xbps repositories. Listing is a read-only scan of
//! `/etc/xbps.d/*.conf` and `/usr/share/xbps.d/*.conf` for
//! `repository=` lines (same files xbps itself reads, per xbps.d(5)) —
//! no privilege needed just to look. Only entries caerus itself added
//! (tracked by which came from its own managed conf file) can be
//! removed here; anything configured some other way is shown but
//! read-only, so this can never surprise-edit someone else's setup.

use crate::backend::transaction::Transaction;
use crate::ui::dialog_util::{cancel_button_row, close_button, modal_window, present_focused};
use gtk::prelude::*;
use std::rc::Rc;

/// Warns before actually adding a custom repository: unlike the official
/// Void mirrors, an arbitrary repository added here has no guarantee of
/// being signed, and `caerus-helper` always installs/upgrades with `-y`
/// (auto-confirming whatever prompt `xbps-install` would otherwise show
/// for an unsigned repo) — so without this, a user could add a hostile
/// or accidental local repo and have packages from it installed with no
/// warning ever shown. `cb(true)` only fires if the user explicitly
/// confirms; closing/Escape/Cancel all resolve to `cb(false)`.
fn confirm_add_repo(parent: Option<&gtk::Window>, url: &str, cb: impl Fn(bool) + 'static) {
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);
    let (dlg, outer) = modal_window("Add Repository?", parent, false, (420, -1), 10);

    let heading = gtk::Label::new(Some(
        "Caerus doesn't verify custom repositories the way the official \
         Void mirrors are verified. If this repository is unsigned or \
         untrusted, packages installed from it could compromise your \
         system — Caerus always installs with automatic confirmation, so \
         no further warning will be shown at install time.",
    ));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let url_label = gtk::Label::new(Some(url));
    url_label.set_xalign(0.0);
    url_label.set_wrap(true);
    url_label.set_selectable(true);
    url_label.add_css_class("dim-label");
    outer.append(&url_label);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let add_btn = gtk::Button::with_label("Add Repository");
    add_btn.add_css_class("destructive-action");
    btn_box.append(&add_btn);
    outer.append(&btn_box);

    // Cancel is the safer default, both as the Enter target and initial
    // focus — same reasoning as `remove_confirm`'s "Remove Anyway".
    dlg.set_default_widget(Some(&cancel_btn));

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
        add_btn.connect_clicked(move |_| {
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

    present_focused(&dlg, &cancel_btn);
}

/// Every URL configured in an xbps.d conf file, enabled or disabled —
/// the sidebar uses this to mark package-origin repos that aren't
/// configured anywhere as stale.
pub(crate) fn configured_repo_urls() -> std::collections::HashSet<String> {
    scan_configured_repos()
        .into_iter()
        .map(|(url, _, _)| url)
        .collect()
}

/// (url, in-/etc (⇒ removable), enabled), deduplicated, sorted by URL.
/// Disabled means a `#repository=` line — only recognized under
/// /etc/xbps.d, where caerus (or an admin) put it; vendor files aren't
/// ours to comment, so their commented examples never count.
///
/// Mirrors xbps.d(5)'s override rule: a file in `/etc/xbps.d` *replaces*
/// the `/usr/share/xbps.d` file of the same name entirely — so a
/// same-named vendor file's repositories must not be listed when an
/// `/etc` override exists (the override may exist precisely to disable
/// them).
fn scan_configured_repos() -> Vec<(String, bool, bool)> {
    let mut map: std::collections::BTreeMap<String, (bool, bool)> =
        std::collections::BTreeMap::new();
    let mut etc_names: std::collections::HashSet<std::ffi::OsString> =
        std::collections::HashSet::new();
    for dir in ["/etc/xbps.d", "/usr/share/xbps.d"] {
        let is_etc = dir == "/etc/xbps.d";
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "conf")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| !n.starts_with('.'))
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Some(name) = path.file_name() {
                if is_etc {
                    etc_names.insert(name.to_os_string());
                } else if etc_names.contains(name) {
                    continue; // overridden by the /etc/xbps.d file above
                }
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in contents.lines() {
                if let Some(url) = line.strip_prefix("repository=") {
                    let url = url.trim();
                    if !url.is_empty() {
                        map.entry(url.to_string()).or_insert((is_etc, true));
                    }
                } else if is_etc {
                    if let Some(url) = line.strip_prefix("#repository=") {
                        let url = url.trim();
                        if !url.is_empty() {
                            map.entry(url.to_string()).or_insert((true, false));
                        }
                    }
                }
            }
        }
    }
    map.into_iter().map(|(u, (r, e))| (u, r, e)).collect()
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
    for (url, in_etc, enabled) in scan_configured_repos() {
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
        if !enabled {
            l.add_css_class("dim-label");
        }
        row_box.append(&l);

        if !in_etc {
            let badge = gtk::Label::new(Some("vendor"));
            badge.add_css_class("dim-label");
            badge.set_tooltip_text(Some(
                "Void default repository — can be disabled, but not removed here",
            ));
            row_box.append(&badge);
        }

        let switch = gtk::Switch::new();
        switch.set_state(enabled);
        switch.set_active(enabled);
        switch.set_valign(gtk::Align::Center);
        switch.set_tooltip_text(Some("Enable/disable this repository"));
        {
            let inner = inner.clone();
            let url = url.clone();
            switch.connect_state_set(move |switch, want_enabled| {
                let inner2 = inner.clone();
                let switch = switch.clone();
                let verb = if want_enabled {
                    "ENABLEREPO"
                } else {
                    "DISABLEREPO"
                };
                let commands = vec![format!("{verb} {url}"), "SYNC".to_string()];
                let commands_for_history = commands.clone();
                let title = if want_enabled {
                    "Enabling Repository"
                } else {
                    "Disabling Repository"
                };
                crate::ui::apply_dialog::run(
                    Some(&inner.dlg),
                    &inner.session,
                    &commands,
                    title,
                    move |success| {
                        crate::backend::history::record(&commands_for_history, success);
                        if success {
                            switch.set_state(want_enabled);
                        }
                        refresh(&inner2);
                        (inner2.on_changed)();
                    },
                );
                glib::Propagation::Stop
            });
        }
        row_box.append(&switch);

        if in_etc {
            let btn = gtk::Button::from_icon_name("user-trash-symbolic");
            btn.set_tooltip_text(Some("Remove this repository"));
            btn.add_css_class("flat");
            let inner = inner.clone();
            let url = url.clone();
            btn.connect_clicked(move |_| {
                let inner2 = inner.clone();
                let commands = vec![format!("REMOVEREPO {}", url), "SYNC".to_string()];
                let commands_for_history = commands.clone();
                crate::ui::apply_dialog::run(
                    Some(&inner.dlg),
                    &inner.session,
                    &commands,
                    "Removing Repository",
                    move |success| {
                        crate::backend::history::record(&commands_for_history, success);
                        refresh(&inner2);
                        (inner2.on_changed)();
                    },
                );
            });
            row_box.append(&btn);
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

    close_button(&outer, &dlg, 4);

    dlg.set_default_widget(Some(&add_btn));

    let inner = Rc::new(Inner {
        dlg: dlg.clone(),
        session: session.clone(),
        repos_list,
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
            let url_for_run = url.clone();
            confirm_add_repo(Some(&inner.dlg), &url, move |confirmed| {
                if !confirmed {
                    return;
                }
                let inner3 = inner2.clone();
                let entry3 = entry2.clone();
                let commands = vec![format!("ADDREPO {}", url_for_run), "SYNC".to_string()];
                let commands_for_history = commands.clone();
                crate::ui::apply_dialog::run(
                    Some(&inner2.dlg),
                    &inner2.session,
                    &commands,
                    "Adding Repository",
                    move |success| {
                        crate::backend::history::record(&commands_for_history, success);
                        entry3.set_text("");
                        refresh(&inner3);
                        (inner3.on_changed)();
                    },
                );
            });
        });
    }

    refresh(&inner);

    present_focused(&dlg, &entry);
}
