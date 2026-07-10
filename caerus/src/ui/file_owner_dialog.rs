//! "Find Owning Package" — a thin GUI over `xbps-query -o <path>`,
//! which package(s) a given file belongs to. Purely a local pkgdb
//! lookup, same as the package list's own detail queries, so (like
//! those) it runs directly from the unprivileged GUI process — no
//! `caerus-helper`/pkexec involved.

use gtk::prelude::*;
use std::process::Command;

/// `xbps-query -o` is a fast local pkgdb scan (comparable to the FFI
/// detail-pane lookups `PackageStore` does on its worker thread) and
/// this dialog is a rare, explicit user action rather than something on
/// a hot path, so a brief synchronous block here — same tradeoff the
/// rest of the app already makes for local xbps queries — is simpler
/// than standing up a background-thread/channel/poll pipeline for a
/// single one-shot subprocess call.
fn query_owner(path: &str) -> Result<String, String> {
    let output = Command::new("xbps-query")
        .arg("-o")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run xbps-query: {}", e))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() {
            text.push_str(err.trim());
        }
    }
    Ok(text)
}

pub fn show(parent: Option<&gtk::Window>) {
    let dlg = gtk::Window::new();
    dlg.set_title(Some("Find Owning Package"));
    if let Some(p) = parent {
        dlg.set_transient_for(Some(p));
    }
    dlg.set_modal(true);
    dlg.set_default_size(480, 320);
    dlg.set_resizable(true);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.set_margin_start(16);
    outer.set_margin_end(16);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);

    let hint = gtk::Label::new(Some(
        "Enter a file path (or a regex — same matching xbps-query itself uses):",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("dim-label");
    outer.append(&hint);

    let entry_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("/usr/bin/bash"));
    entry.set_hexpand(true);
    entry.set_activates_default(true);
    let search_btn = gtk::Button::with_label("Search");
    search_btn.add_css_class("suggested-action");
    entry_row.append(&entry);
    entry_row.append(&search_btn);
    outer.append(&entry_row);

    let results_scroll = gtk::ScrolledWindow::new();
    results_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    results_scroll.set_vexpand(true);
    results_scroll.set_margin_top(4);
    let results_list = gtk::ListBox::new();
    results_list.set_selection_mode(gtk::SelectionMode::None);
    let placeholder = gtk::Label::new(Some("Enter a path above and press Search."));
    placeholder.add_css_class("dim-label");
    placeholder.set_margin_top(24);
    results_list.set_placeholder(Some(&placeholder));
    results_scroll.set_child(Some(&results_list));
    outer.append(&results_scroll);

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_margin_top(4);
    outer.append(&close_btn);

    dlg.set_child(Some(&outer));
    dlg.set_default_widget(Some(&search_btn));

    let run_search = {
        let entry = entry.clone();
        let results_list = results_list.clone();
        move || {
            let query = entry.text().trim().to_string();
            while let Some(child) = results_list.first_child() {
                results_list.remove(&child);
            }
            if query.is_empty() {
                return;
            }
            match query_owner(&query) {
                Ok(text) if !text.trim().is_empty() => {
                    for line in text.lines().filter(|l| !l.trim().is_empty()) {
                        let l = gtk::Label::new(Some(line));
                        l.set_xalign(0.0);
                        l.set_selectable(true);
                        l.set_wrap(true);
                        l.set_margin_start(8);
                        l.set_margin_top(4);
                        l.set_margin_bottom(4);
                        let row = gtk::ListBoxRow::new();
                        row.set_child(Some(&l));
                        results_list.append(&row);
                    }
                }
                Ok(_) => {
                    let l = gtk::Label::new(Some("No package owns a file matching that."));
                    l.add_css_class("dim-label");
                    l.set_margin_top(24);
                    let row = gtk::ListBoxRow::new();
                    row.set_selectable(false);
                    row.set_activatable(false);
                    row.set_child(Some(&l));
                    results_list.append(&row);
                }
                Err(e) => {
                    let l = gtk::Label::new(Some(&e));
                    l.add_css_class("error");
                    l.set_margin_top(24);
                    let row = gtk::ListBoxRow::new();
                    row.set_selectable(false);
                    row.set_activatable(false);
                    row.set_child(Some(&l));
                    results_list.append(&row);
                }
            }
        }
    };

    {
        let run_search = run_search.clone();
        search_btn.connect_clicked(move |_| run_search());
    }
    {
        let run_search = run_search.clone();
        entry.connect_activate(move |_| run_search());
    }
    {
        let dlg = dlg.clone();
        close_btn.connect_clicked(move |_| dlg.destroy());
    }

    dlg.present();
    entry.grab_focus();
}
