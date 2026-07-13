//! "Find Owning Package" — a thin GUI over `xbps-query -o <path>`,
//! which package(s) a given file belongs to. Purely a local pkgdb
//! lookup, same as the package list's own detail queries, so (like
//! those) it runs directly from the unprivileged GUI process — no
//! `caerus-helper`/pkexec involved.

use crate::ui::dialog_util::{close_button, modal_window, present_focused, text_list_row};
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
    let (dlg, outer) = modal_window("Find Owning Package", parent, true, (480, 320), 8);

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

    close_button(&outer, &dlg, 4);

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
                        results_list.append(&text_list_row(line, true));
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
    present_focused(&dlg, &entry);
}
