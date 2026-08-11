//! If a package has any not-yet-installed `run_depends` (transitively),
//! shows a confirmation dialog transient for `parent` listing them.
//!
//! Asynchronous: `cb` may fire after this function returns (a real
//! dialog was shown) or before it returns (the no-deps-missing fast path).

use crate::backend::package::PkgMark;
use crate::backend::package_store::PackageStore;
use crate::ui::dialog_util::{cancel_button_row, modal_window, present_focused, text_list_row};
use gtk::prelude::*;
use std::rc::Rc;

pub fn confirm_install_deps(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    pkgname: &str,
    cb: impl Fn(bool) + 'static,
) {
    let parent = parent.cloned();
    let store2 = store.clone();
    let pkgname = pkgname.to_string();
    store.get_missing_deps_async(&pkgname.clone(), move |deps| {
        let Some(deps) = deps else {
            // Nothing missing — don't interrupt the common case.
            cb(true);
            return;
        };
        show_deps_dialog(parent.as_ref(), &store2, &pkgname, deps, cb);
    });
}

fn show_deps_dialog(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    pkgname: &str,
    deps: Vec<String>,
    cb: impl Fn(bool) + 'static,
) {
    let n = deps.len();
    let deps: Rc<Vec<String>> = Rc::new(deps);
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);

    let (dlg, outer) = modal_window("Additional Packages Required", parent, true, (420, -1), 10);

    let heading = gtk::Label::new(Some(&format!(
        "Installing {} also requires {} additional package{}:",
        pkgname,
        n,
        if n == 1 { "" } else { "s" }
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    // propagate-natural-height + capped max-content-height: short lists
    // size to fit, long ones get a real scrollbar instead of an
    // unbounded window.
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(400);
    scroll.set_vexpand(true);

    let mut sorted = (*deps).clone();
    sorted.sort();
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for d in &sorted {
        list.append(&text_list_row(d, false));
    }
    scroll.set_child(Some(&list));
    outer.append(&scroll);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let install_btn = gtk::Button::with_label("Install All");
    install_btn.add_css_class("suggested-action");
    btn_box.append(&install_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&install_btn));

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            cb(false);
            dlg.destroy();
        });
    }
    {
        let store = store.clone();
        let cb = cb.clone();
        let dlg = dlg.clone();
        install_btn.connect_clicked(move |_| {
            dlg.destroy();
            let store = store.clone();
            let cb = cb.clone();
            let deps = deps.clone();
            glib::source::idle_add_local_once(move || {
                for d in deps.iter() {
                    store.set_mark(d, PkgMark::Install);
                }
                cb(true);
            });
        });
    }
    {
        // Window-manager close (title-bar X) or Escape counts as Cancel,
        // same as the button.
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, &install_btn);
}
