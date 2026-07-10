//! Preset filter sidebar. Rust translation of ui/filter_sidebar.{h,c}
//! (built directly in code here rather than from a GtkBuilder .ui file
//! — see the top-level README for why).
//!
//! Row order must stay in sync with `FilterMode::from_row_index` in
//! backend/package.rs, exactly like the original's comment about
//! `on_preset_selected()`'s use of `gtk_list_box_row_get_index()`.

use crate::backend::package::FilterMode;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

struct Inner {
    widget: gtk::Box,
    on_filter_changed: RefCell<Vec<Box<dyn Fn(FilterMode)>>>,
}

#[derive(Clone)]
pub struct FilterSidebar {
    inner: Rc<Inner>,
}

fn make_row(icon: &str, label: &str) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.set_margin_start(8);
    row_box.set_margin_end(8);
    row_box.set_margin_top(5);
    row_box.set_margin_bottom(5);
    row_box.append(&gtk::Image::from_icon_name(icon));
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    row_box.append(&l);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

impl FilterSidebar {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_width_request(190);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);

        let inner_box = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let filter_header = gtk::Label::new(Some("FILTER"));
        filter_header.set_xalign(0.0);
        filter_header.set_margin_top(8);
        filter_header.set_margin_start(6);
        filter_header.set_margin_bottom(2);
        filter_header.add_css_class("section-header");
        inner_box.append(&filter_header);

        let preset_lb = gtk::ListBox::new();
        preset_lb.set_selection_mode(gtk::SelectionMode::Single);
        preset_lb.add_css_class("navigation-sidebar");

        preset_lb.append(&make_row("view-list-symbolic", "All"));
        preset_lb.append(&make_row("object-select-symbolic", "Installed"));
        preset_lb.append(&make_row("list-remove-symbolic", "Not Installed"));
        preset_lb.append(&make_row(
            "software-update-available-symbolic",
            "Upgradable",
        ));
        preset_lb.append(&make_row("media-playback-pause-symbolic", "On Hold"));
        preset_lb.append(&make_row("emblem-important-symbolic", "Marked"));

        inner_box.append(&preset_lb);
        scroll.set_child(Some(&inner_box));
        widget.append(&scroll);

        let inner = Rc::new(Inner {
            widget,
            on_filter_changed: RefCell::new(Vec::new()),
        });

        {
            let inner_weak = Rc::downgrade(&inner);
            preset_lb.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                let mode = FilterMode::from_row_index(row.index());
                for cb in inner.on_filter_changed.borrow().iter() {
                    cb(mode);
                }
            });
        }

        // Selects "All" visually. NOTE: exactly like the original, this
        // fires "filter-changed" synchronously during construction,
        // before the caller has had a chance to call
        // `connect_filter_changed` — so that first emission is
        // silently dropped. Harmless only because `PackageList`'s own
        // default (`FilterMode::All`) already matches; see the same
        // caveat in the original ui/filter_sidebar.c.
        if let Some(row0) = preset_lb.row_at_index(0) {
            preset_lb.select_row(Some(&row0));
        }

        FilterSidebar { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_filter_changed(&self, f: impl Fn(FilterMode) + 'static) {
        self.inner.on_filter_changed.borrow_mut().push(Box::new(f));
    }
}
