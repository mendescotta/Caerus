use crate::backend::package::pkg_format_size;
use crate::backend::transaction_preview::{TransAction, TransactionError, TransactionPreview};
use crate::ui::dialog_util::{
    cancel_button_row, count_pill, modal_window, present_focused, set_count, text_list_row,
};
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

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
        let label = format!("{} — {}", item.description(), size);
        list.append(&text_list_row(&label, true));
    }
    outer.append(&list);
}

fn totals_footer(outer: &gtk::Box, preview: &TransactionPreview) {
    let footer = gtk::Label::new(Some(&format!(
        "Download {} \u{2022} Estimated disk change {}",
        pkg_format_size(preview.total_download_size),
        preview.disk_change_text(),
    )));
    footer.set_xalign(0.0);
    footer.set_wrap(true);
    footer.add_css_class("dim-label");
    footer.set_margin_top(8);
    outer.append(&footer);
    if cfg!(feature = "apk") {
        let note = gtk::Label::new(Some("Size estimates include available package metadata; actual download and disk use can differ."));
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.add_css_class("dim-label");
        outer.append(&note);
    }
}

fn error_banner(outer: &gtk::Box, err: &TransactionError) {
    let heading = gtk::Label::new(Some(&format!(
        "{} reports a problem: {}",
        if cfg!(feature = "apk") {
            "APK simulation"
        } else {
            "libxbps dry-run"
        },
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

    let note = gtk::Label::new(Some("No changes were applied. Refresh the package database, resolve the reported problem, then review a new plan."));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    note.set_margin_top(4);
    note.set_margin_bottom(4);
    outer.append(&note);
}

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
    } else if preview.is_none() {
        error_banner(
            &outer,
            &TransactionError::Other(
                "A complete transaction preview is unavailable. Reload packages and try again."
                    .to_string(),
            ),
        );
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
            if cfg!(feature = "apk") {
                "Recursive purge is unavailable with APK"
            } else {
                "Purge (also removes orphaned dependencies)"
            },
            purges,
        );
    }
    scroll.set_child(Some(&sections));
    outer.append(&scroll);

    if let Some(Ok(p)) = &preview {
        totals_footer(&outer, p);
    }

    let (btn_box, cancel_btn) = cancel_button_row(10);

    let preview_failed = !matches!(preview, Some(Ok(_)));
    let destructive = !removes.is_empty() || !purges.is_empty() || preview_failed;
    let apply_label = if preview_failed {
        "Preview required"
    } else {
        "Apply changes"
    };
    let apply_btn = gtk::Button::with_label(apply_label);
    apply_btn.set_sensitive(!preview_failed);
    if destructive {
        apply_btn.add_css_class("destructive-action");
    } else {
        apply_btn.add_css_class("suggested-action");
    }
    btn_box.append(&apply_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(if destructive { &cancel_btn } else { &apply_btn }));

    let cb = Rc::new(cb);
    let responded = Rc::new(Cell::new(false));

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        let responded = responded.clone();
        cancel_btn.connect_clicked(move |_| {
            if !responded.replace(true) {
                dlg.destroy();
                cb(false);
            }
        });
    }
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        let responded = responded.clone();
        apply_btn.connect_clicked(move |_| {
            if !responded.replace(true) {
                dlg.destroy();
                cb(true);
            }
        });
    }
    {
        let cb = cb;
        dlg.connect_close_request(move |_| {
            if !responded.replace(true) {
                let cb = cb.clone();
                glib::idle_add_local_once(move || cb(false));
            }
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, if destructive { &cancel_btn } else { &apply_btn });
}

#[cfg(test)]
pub fn smoke_test_incomplete_previews() {
    fn find_button(widget: &gtk::Widget, text: &str) -> Option<gtk::Button> {
        if let Some(button) = widget.downcast_ref::<gtk::Button>() {
            if button.label().as_deref() == Some(text) {
                return Some(button.clone());
            }
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(button) = find_button(&current, text) {
                return Some(button);
            }
            child = current.next_sibling();
        }
        None
    }
    for preview in [
        None,
        Some(Err(TransactionError::Other(
            "fixture dependency conflict".into(),
        ))),
    ] {
        let result = Rc::new(std::cell::Cell::new(None));
        let answer = result.clone();
        confirm(
            None,
            &["foo".into()],
            &[],
            &[],
            &[],
            preview,
            move |value| answer.set(Some(value)),
        );
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
            .find(|window| window.title().as_deref() == Some("Confirm Changes"))
            .expect("preview dialog should be visible");
        let apply = find_button(dialog.upcast_ref(), "Preview required").unwrap();
        assert!(
            !apply.is_sensitive(),
            "missing and failed previews must block apply"
        );
        let cancel = find_button(dialog.upcast_ref(), "Cancel").unwrap();
        assert_eq!(
            gtk::prelude::GtkWindowExt::focus(&dialog),
            Some(cancel.clone().upcast())
        );
        cancel.emit_clicked();
        assert_eq!(result.get(), Some(false));
    }
}
