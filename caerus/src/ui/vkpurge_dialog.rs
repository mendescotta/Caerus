//! "Purge Old Kernels" — a thin GUI over `vkpurge`, the standalone Void
//! script (not an xbps tool) that removes kernel files/modules a kernel
//! upgrade left behind once nothing needs them anymore. Listing is a
//! read-only local scan (`vkpurge list`: reads `/boot`, cross-checks
//! `xbps-query -o` ownership and the running `uname -r`, no root
//! needed), so — like `file_owner_dialog`'s `xbps-query -o` — it runs
//! directly from the unprivileged GUI process; only the actual removal
//! (which `vkpurge` itself refuses to run as non-root) goes through
//! `caerus-helper` via `pkexec`.

use crate::backend::transaction::Transaction;
use crate::ui::apply_dialog;
use crate::ui::dialog_util::{close_button, modal_window};
use gio::prelude::*;
use glib::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::process::Command;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct KernelObject {
        pub version: RefCell<String>,
        pub checked: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for KernelObject {
        const NAME: &'static str = "CaerusKernelObject";
        type Type = super::KernelObject;
    }

    impl ObjectImpl for KernelObject {}
}

glib::wrapper! {
    pub struct KernelObject(ObjectSubclass<imp::KernelObject>);
}

impl KernelObject {
    fn new(version: String) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().version.replace(version);
        obj
    }
    fn version(&self) -> String {
        self.imp().version.borrow().clone()
    }
    fn checked(&self) -> bool {
        self.imp().checked.get()
    }
    fn set_checked(&self, v: bool) {
        self.imp().checked.set(v);
    }
}

fn kernel_of(obj: &glib::Object) -> KernelObject {
    obj.clone().downcast::<KernelObject>().unwrap()
}

/// Parses `vkpurge list` output; the subprocess itself runs off the
/// main thread via `run_command_async` (see `refresh`).
fn parse_kernel_list(result: Result<std::process::Output, String>) -> Result<Vec<String>, String> {
    let output = result.map_err(|e| format!("failed to run vkpurge: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(if err.trim().is_empty() {
            "vkpurge list failed".to_string()
        } else {
            err.trim().to_string()
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn make_col(
    title: &str,
    width: i32,
    expand: bool,
    setup: impl Fn(&gtk::ListItem) + 'static,
    bind: impl Fn(&gtk::ListItem) + 'static,
) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| setup(item.downcast_ref::<gtk::ListItem>().unwrap()));
    factory.connect_bind(move |_, item| bind(item.downcast_ref::<gtk::ListItem>().unwrap()));
    let col = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    if width > 0 {
        col.set_fixed_width(width);
    }
    col.set_expand(expand);
    col
}

fn refresh(list_store: &gio::ListStore, status_label: &gtk::Label) {
    list_store.remove_all();
    status_label.set_text("Listing removable kernels\u{2026}");
    status_label.set_visible(true);

    let mut cmd = Command::new("vkpurge");
    cmd.arg("list");
    let list_store = list_store.clone();
    let status_label = status_label.clone();
    crate::ui::dialog_util::run_command_async(cmd, move |result| match parse_kernel_list(result) {
        Ok(versions) if versions.is_empty() => {
            status_label.set_text("No removable kernel versions found.");
            status_label.set_visible(true);
        }
        Ok(versions) => {
            status_label.set_visible(false);
            for v in versions {
                list_store.append(&KernelObject::new(v));
            }
        }
        Err(e) => {
            status_label.set_text(&format!("Could not list kernels: {e}"));
            status_label.set_visible(true);
        }
    });
}

pub fn show(parent: Option<&gtk::Window>, session: &Transaction) {
    let (dlg, outer) = modal_window("Purge Old Kernels", parent, true, (460, 420), 8);

    let hint = gtk::Label::new(Some(
        "Kernel versions no longer owned by any installed package and not \
         currently running — safe to remove. Select which ones to purge.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    outer.append(&hint);

    let status_label = gtk::Label::new(None);
    status_label.set_xalign(0.0);
    status_label.set_margin_top(8);
    status_label.set_margin_bottom(8);
    status_label.set_visible(false);
    outer.append(&status_label);

    let list_store = gio::ListStore::new::<KernelObject>();

    let col_check = make_col(
        "",
        32,
        false,
        |item| {
            item.set_activatable(false);
            let cb = gtk::CheckButton::new();
            cb.set_halign(gtk::Align::Center);
            let li = item.clone();
            cb.connect_toggled(move |cb| {
                if let Some(obj) = li.item().map(|o| kernel_of(&o)) {
                    obj.set_checked(cb.is_active());
                }
            });
            item.set_child(Some(&cb));
        },
        |item| {
            let Some(obj) = item.item().map(|o| kernel_of(&o)) else {
                return;
            };
            let cb = item.child().and_downcast::<gtk::CheckButton>().unwrap();
            cb.set_active(obj.checked());
        },
    );
    let col_version = make_col(
        "Kernel Version",
        200,
        true,
        |item| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            item.set_child(Some(&l));
        },
        |item| {
            let Some(obj) = item.item().map(|o| kernel_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            l.set_text(&obj.version());
        },
    );

    let selection = gtk::NoSelection::new(Some(list_store.clone()));
    let column_view = gtk::ColumnView::new(Some(selection));
    column_view.append_column(&col_check);
    column_view.append_column(&col_version);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&column_view));
    scroll.set_vexpand(true);
    scroll.set_min_content_height(220);
    outer.append(&scroll);

    refresh(&list_store, &status_label);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(8);

    let reload_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
    reload_btn.set_tooltip_text(Some("Re-scan for removable kernel versions"));
    let select_all_btn = gtk::Button::with_label("Select All");
    let purge_btn = gtk::Button::with_label("Purge Selected");
    purge_btn.add_css_class("destructive-action");
    btn_box.append(&reload_btn);
    btn_box.append(&select_all_btn);
    btn_box.append(&purge_btn);
    outer.append(&btn_box);
    close_button(&outer, &dlg, 0);

    {
        let list_store = list_store.clone();
        let status_label = status_label.clone();
        reload_btn.connect_clicked(move |_| {
            refresh(&list_store, &status_label);
        });
    }

    {
        let list_store = list_store.clone();
        select_all_btn.connect_clicked(move |_| {
            let n = list_store.n_items();
            for i in 0..n {
                if let Some(obj) = list_store.item(i) {
                    kernel_of(&obj).set_checked(true);
                }
            }
            // Forces every currently-bound row to re-query its item and
            // refresh its checkbox — mutating the backing objects alone
            // doesn't repaint already-bound rows.
            list_store.items_changed(0, n, n);
        });
    }

    {
        let list_store = list_store;
        let session = session.clone();
        let dlg_for_purge = dlg.clone();
        let status_label = status_label;
        purge_btn.connect_clicked(move |_| {
            let mut versions = Vec::new();
            let n = list_store.n_items();
            for i in 0..n {
                if let Some(obj) = list_store.item(i) {
                    let obj = kernel_of(&obj);
                    if obj.checked() {
                        versions.push(obj.version());
                    }
                }
            }
            if versions.is_empty() {
                return;
            }
            let cmd = format!("VKPURGE {}", versions.join(" "));
            let cmd_for_history = cmd.clone();
            let list_store = list_store.clone();
            let status_label = status_label.clone();
            apply_dialog::run(
                Some(dlg_for_purge.upcast_ref()),
                &session,
                &[cmd],
                "Purging Old Kernels",
                move |success| {
                    crate::backend::history::record(
                        std::slice::from_ref(&cmd_for_history),
                        success,
                    );
                    // Refresh in place rather than closing — lets the
                    // user see what's left (or the error) without
                    // reopening the dialog.
                    refresh(&list_store, &status_label);
                },
            );
        });
    }

    dlg.present();
}
