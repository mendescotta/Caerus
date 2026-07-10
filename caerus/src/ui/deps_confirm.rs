//! If a package has any not-yet-installed run_depends (transitively),
//! shows a confirmation dialog transient for `parent` listing them.
//! Rust translation of ui/deps_confirm.{h,c}.
//!
//! Asynchronous: `cb` may fire after this function returns (a real
//! dialog was shown) or before it returns (the no-deps-missing fast
//! path) — mirroring the original exactly.

use crate::backend::package::PkgMark;
use crate::backend::package_store::PackageStore;
use crate::ui::dialog_util::{modal_window, present_focused, text_list_row};
use gtk::prelude::*;
use std::rc::Rc;

pub fn confirm_install_deps(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    pkgname: &str,
    cb: impl Fn(bool) + 'static,
) {
    let Some(deps) = store.get_missing_deps(pkgname) else {
        // Nothing missing — don't interrupt the common case.
        cb(true);
        return;
    };
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

    // propagate-natural-height + a capped max-content-height gives us
    // both cases for free: short lists size to fit naturally, long
    // ones get a real scrollbar rather than the window itself growing
    // unboundedly — same rationale as the original.
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(400);
    scroll.set_vexpand(true);

    // Same list-box style as `remove_confirm`'s affected-package list and
    // the detail pane's Dependencies list — a plain wrapped label here
    // used to be the odd one out, selecting as a single opaque block of
    // text instead of one name at a time.
    let mut sorted = (*deps).clone();
    sorted.sort();
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for d in &sorted {
        list.append(&text_list_row(d, false));
    }
    scroll.set_child(Some(&list));
    outer.append(&scroll);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(4);
    let cancel_btn = gtk::Button::with_label("Cancel");
    let install_btn = gtk::Button::with_label("Install All");
    install_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&install_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&install_btn));

    {
        let store = store.clone();
        let deps = deps.clone();
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            let _ = &store; // cancel never marks anything
            let _ = &deps;
            cb(false);
            dlg.destroy();
        });
    }
    {
        let store = store.clone();
        let deps = deps.clone();
        let cb = cb.clone();
        let dlg = dlg.clone();
        install_btn.connect_clicked(move |_| {
            for d in deps.iter() {
                store.set_mark(d, PkgMark::Install);
            }
            cb(true);
            dlg.destroy();
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
