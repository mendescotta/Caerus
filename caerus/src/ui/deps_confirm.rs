//! If a package has any not-yet-installed run_depends (transitively),
//! shows a confirmation dialog transient for `parent` listing them.
//! Rust translation of ui/deps_confirm.{h,c}.
//!
//! Asynchronous: `cb` may fire after this function returns (a real
//! dialog was shown) or before it returns (the no-deps-missing fast
//! path) — mirroring the original exactly.

use crate::backend::package::PkgMark;
use crate::backend::package_store::PackageStore;
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

    let dlg = gtk::Window::new();
    dlg.set_title(Some("Additional Packages Required"));
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

    let list_text = deps
        .iter()
        .map(|d| format!("\u{2022} {}", d))
        .collect::<Vec<_>>()
        .join("\n");
    let list_label = gtk::Label::new(Some(&list_text));
    list_label.set_xalign(0.0);
    list_label.set_yalign(0.0);
    list_label.set_selectable(true);
    list_label.set_margin_start(4);
    list_label.set_margin_end(4);
    scroll.set_child(Some(&list_label));
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

    dlg.set_child(Some(&outer));
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
        // Window-manager close (title-bar X) counts as Cancel, same as
        // the button.
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    dlg.present();
    // Without this, GTK hands initial keyboard focus to the first
    // focusable widget in the window — the selectable-text deps list
    // above — which shows up as its entire text looking "pre-selected"
    // the instant the dialog opens. Explicitly focusing the default
    // button avoids it.
    install_btn.grab_focus();
}
