//! "Find Owning Package" — a thin GUI over `xbps-query -o <path>`,
//! which package(s) a given file belongs to. Purely a local pkgdb
//! lookup, same as the package list's own detail queries, so (like
//! those) it runs directly from the unprivileged GUI process — no
//! `caerus-helper`/pkexec involved.

use crate::ui::dialog_util::{
    close_button, modal_window, present_focused, run_command_async, text_list_row,
};
use gtk::prelude::*;
use std::process::Command;

/// Flattens an `xbps-query -o` result into displayable text (stdout,
/// plus stderr when the query failed). The subprocess itself runs off
/// the main thread via `run_command_async` — `xbps-query -o` scans the
/// whole pkgdb and can take long enough to visibly freeze the UI.
fn owner_output_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() {
            text.push_str(err.trim());
        }
    }
    text
}

fn message_row(text: &str, css_class: &str) -> gtk::ListBoxRow {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(css_class);
    l.set_margin_top(24);
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&l));
    row
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
        let results_list = results_list;
        let search_btn = search_btn.clone();
        move || {
            // One search at a time — the button is re-enabled when the
            // async reply lands.
            if !search_btn.is_sensitive() {
                return;
            }
            let query = entry.text().trim().to_string();
            while let Some(child) = results_list.first_child() {
                results_list.remove(&child);
            }
            if query.is_empty() {
                return;
            }
            search_btn.set_sensitive(false);
            results_list.append(&message_row("Searching\u{2026}", "dim-label"));

            let mut cmd = Command::new("xbps-query");
            cmd.arg("-o").arg(&query);

            let results_list = results_list.clone();
            let search_btn = search_btn.clone();
            run_command_async(cmd, move |result| {
                search_btn.set_sensitive(true);
                while let Some(child) = results_list.first_child() {
                    results_list.remove(&child);
                }
                match result {
                    Ok(output) => {
                        let text = owner_output_text(&output);
                        if text.trim().is_empty() {
                            results_list.append(&message_row(
                                "No package owns a file matching that.",
                                "dim-label",
                            ));
                        } else {
                            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                                results_list.append(&text_list_row(line, true));
                            }
                        }
                    }
                    Err(e) => {
                        results_list.append(&message_row(
                            &format!("failed to run xbps-query: {e}"),
                            "error",
                        ));
                    }
                }
            });
        }
    };

    {
        let run_search = run_search.clone();
        search_btn.connect_clicked(move |_| run_search());
    }
    {
        let run_search = run_search;
        entry.connect_activate(move |_| run_search());
    }
    present_focused(&dlg, &entry);
}
