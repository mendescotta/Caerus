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
use crate::ui::dialog_util::{modal_window, present_focused, text_list_row};
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
    // Transitive: `(affected_pkgname, direct_parent_that_pulled_it_in)`.
    // A name reached only through an intermediate package (parent !=
    // `pkgname` itself) gets annotated "(via parent)" below so the
    // dialog shows *why* it would break, not just that it would.
    let affected: Vec<(String, String)> = store
        .get_rdeps_transitive(pkgname)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| name != pkgname && still_installed_afterward(store, name))
        .collect();

    if affected.is_empty() {
        // The common case — don't interrupt removing a leaf package.
        cb(true);
        return;
    }
    let n = affected.len();
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);

    let (dlg, outer) = modal_window("Other Packages Depend On This", parent, true, (420, -1), 10);

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
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for (name, via) in &sorted {
        let label = if via == pkgname {
            name.clone()
        } else {
            format!("{} (via {})", name, via)
        };
        list.append(&text_list_row(&label, false));
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

    present_focused(&dlg, &cancel_btn);
}
