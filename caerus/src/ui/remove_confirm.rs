//! If removing a package would leave any other *currently installed*
//! package's dependency unsatisfied, shows a confirmation dialog
//! transient for `parent` listing them. The install-side equivalent of
//! `deps_confirm.rs`, checking reverse rather than forward dependencies.
//!
//! Asynchronous: `cb` may fire after this function returns (a real
//! dialog was shown) or before it returns (nothing installed actually
//! depends on this package) — same shape as `deps_confirm`.

use crate::backend::package::{PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use gtk::prelude::*;
use std::rc::Rc;

/// A reverse dependency only matters here if it's still going to be
/// installed after this batch runs: already-installed and not itself
/// marked for Remove/Purge.
fn still_installed_afterward(store: &PackageStore, name: &str) -> bool {
    match store.state_and_mark(name) {
        Some((state, mark)) => {
            let installed = matches!(
                state,
                PkgState::Installed | PkgState::Upgradable | PkgState::OnHold | PkgState::Broken
            );
            installed && !matches!(mark, PkgMark::Remove | PkgMark::Purge)
        }
        None => false,
    }
}

pub fn confirm_remove_impact(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    pkgname: &str,
    cb: impl Fn(bool) + 'static,
) {
    let affected: Vec<String> = store
        .get_rdeps(pkgname)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name != pkgname && still_installed_afterward(store, name))
        .collect();

    if affected.is_empty() {
        // The common case — don't interrupt removing a leaf package.
        cb(true);
        return;
    }
    let n = affected.len();
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);

    let dlg = gtk::Window::new();
    dlg.set_title(Some("Other Packages Depend On This"));
    if let Some(p) = parent {
        dlg.set_transient_for(Some(p));
    }
    dlg.set_modal(true);
    dlg.set_resizable(true);
    dlg.set_default_size(420, -1);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.set_margin_start(16);
    outer.set_margin_end(16);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);

    let heading = gtk::Label::new(Some(&format!(
        "Removing {} may break {} other installed package{} that depend{} on it:",
        pkgname,
        n,
        if n == 1 { "" } else { "s" },
        if n == 1 { "s" } else { "" },
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    // Same list-box style as `deps_confirm`'s own list and the detail
    // pane's Dependencies list, not a wrapped comma-separated line.
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(360);
    scroll.set_vexpand(true);

    let mut sorted = affected;
    sorted.sort();
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for name in &sorted {
        let l = gtk::Label::new(Some(name));
        l.set_xalign(0.0);
        l.set_selectable(true);
        l.set_margin_start(8);
        l.set_margin_top(4);
        l.set_margin_bottom(4);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        list.append(&row);
    }
    scroll.set_child(Some(&list));
    outer.append(&scroll);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(4);
    let cancel_btn = gtk::Button::with_label("Cancel");
    let remove_btn = gtk::Button::with_label("Remove Anyway");
    remove_btn.add_css_class("destructive-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&remove_btn);
    outer.append(&btn_box);

    dlg.set_child(Some(&outer));
    // Cancel is the safer default — both as the Enter target and the
    // initial focus (also sidesteps the selectable-list-row-grabs-
    // focus-on-open issue the other confirm dialogs had).
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
        remove_btn.connect_clicked(move |_| {
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

    dlg.present();
    cancel_btn.grab_focus();
}
