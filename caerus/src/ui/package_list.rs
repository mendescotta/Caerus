//! The main package table: a `gtk::ColumnView` over a filter+sort model
//! chain, plus a checkbox column for marking several packages at once.
//! Rust translation of ui/package_list.{h,c}.

use crate::backend::package::{
    pkg_format_size, pkg_state_icon, pkg_state_tooltip, FilterMode, Package, PackageObject,
    PkgMark, PkgState,
};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering as CmpOrdering;
use std::rc::Rc;

struct Inner {
    widget: gtk::Box,
    store: PackageStore,
    custom_filter: gtk::CustomFilter,
    current_filter: Cell<FilterMode>,
    current_search: RefCell<String>,
    search_name_only: Cell<bool>,
    on_package_selected: RefCell<Vec<Box<dyn Fn(Option<Package>)>>>,
    on_marks_changed: RefCell<Vec<Box<dyn Fn()>>>,
}

#[derive(Clone)]
pub struct PackageList {
    inner: Rc<Inner>,
}

fn ord(c: CmpOrdering) -> gtk::Ordering {
    match c {
        CmpOrdering::Less => gtk::Ordering::Smaller,
        CmpOrdering::Equal => gtk::Ordering::Equal,
        CmpOrdering::Greater => gtk::Ordering::Larger,
    }
}

fn pkg_of(obj: &glib::Object) -> PackageObject {
    obj.clone().downcast::<PackageObject>().unwrap()
}

/// Status rank: lower sorts first — broken, then marked-for-action,
/// then upgradable, then on-hold, then plain installed, then
/// not-installed last. Mirrors `pkg_sort_rank` in the original.
fn pkg_sort_rank(p: &Package) -> i32 {
    if p.state == PkgState::Broken {
        return 0;
    }
    if p.mark != PkgMark::None {
        return 1;
    }
    match p.state {
        PkgState::Upgradable => 2,
        PkgState::OnHold => 3,
        PkgState::Installed => 4,
        _ => 5,
    }
}

fn set_column_sorter(col: &gtk::ColumnViewColumn, cmp: impl Fn(&Package, &Package) -> CmpOrdering + 'static) {
    let sorter = gtk::CustomSorter::new(move |a, b| {
        let pa = pkg_of(a);
        let pb = pkg_of(b);
        let pa = pa.pkg();
        let pb = pb.pkg();
        ord(cmp(&pa, &pb))
    });
    col.set_sorter(Some(&sorter));
}

fn make_col(
    title: &str,
    width: i32,
    resizable: bool,
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
    col.set_resizable(resizable);
    col.set_expand(expand);
    col
}

fn label_cell(item: &gtk::ListItem) {
    let l = gtk::Label::new(None);
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    item.set_child(Some(&l));
}

impl PackageList {
    pub fn new(store: PackageStore) -> Self {
        let custom_filter = gtk::CustomFilter::new(|_| true); // real predicate wired in below
        let inner = Rc::new(Inner {
            widget: gtk::Box::new(gtk::Orientation::Vertical, 0),
            store,
            custom_filter,
            current_filter: Cell::new(FilterMode::All),
            current_search: RefCell::new(String::new()),
            search_name_only: Cell::new(false),
            on_package_selected: RefCell::new(Vec::new()),
            on_marks_changed: RefCell::new(Vec::new()),
        });

        build(inner.clone());

        PackageList { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_package_selected(&self, f: impl Fn(Option<Package>) + 'static) {
        self.inner
            .on_package_selected
            .borrow_mut()
            .push(Box::new(f));
    }
    pub fn connect_marks_changed(&self, f: impl Fn() + 'static) {
        self.inner.on_marks_changed.borrow_mut().push(Box::new(f));
    }

    pub fn set_filter(&self, mode: FilterMode) {
        self.inner.current_filter.set(mode);
        self.inner
            .custom_filter
            .changed(gtk::FilterChange::Different);
    }
    pub fn set_search(&self, query: &str) {
        *self.inner.current_search.borrow_mut() = query.to_string();
        self.inner
            .custom_filter
            .changed(gtk::FilterChange::Different);
    }
    pub fn set_search_mode(&self, name_only: bool) {
        self.inner.search_name_only.set(name_only);
        self.inner
            .custom_filter
            .changed(gtk::FilterChange::Different);
    }
}

fn build(inner: Rc<Inner>) {
    inner.widget.set_vexpand(true);

    // ── Filter predicate ─────────────────────────────────────────────
    {
        let inner_f = inner.clone();
        inner.custom_filter.set_filter_func(move |obj| {
            let obj = pkg_of(obj);
            let p = obj.pkg();

            let query = inner_f.current_search.borrow();
            if !query.is_empty() {
                let q = query.to_lowercase();
                let name_match = p.name.to_lowercase().contains(&q);
                let desc_match =
                    !inner_f.search_name_only.get() && p.short_desc.to_lowercase().contains(&q);
                if !name_match && !desc_match {
                    return false;
                }
            }
            match inner_f.current_filter.get() {
                FilterMode::All => true,
                FilterMode::Installed => {
                    matches!(p.state, PkgState::Installed | PkgState::Upgradable)
                }
                FilterMode::NotInstalled => p.state == PkgState::NotInstalled,
                FilterMode::Upgradable => p.state == PkgState::Upgradable,
                FilterMode::OnHold => p.state == PkgState::OnHold,
                FilterMode::Marked => p.mark != PkgMark::None,
            }
        });
    }

    let filter_model =
        gtk::FilterListModel::new(Some(inner.store.list()), Some(inner.custom_filter.clone()));
    let sort_model = gtk::SortListModel::new(Some(filter_model), None::<gtk::Sorter>);

    let selection = gtk::SingleSelection::new(Some(sort_model.clone()));
    // autoselect defaults to true, which selects index 0 the instant
    // the list goes from empty to populated — i.e. mid-load, before the
    // initial sort has settled — which then swallows the user's actual
    // first click as a no-op "no change" (see the original's own
    // comment on this exact bug). Disabling it means nothing is
    // selected until a real click, which then always fires a fresh,
    // correctly-ordered selection event.
    selection.set_autoselect(false);

    {
        let inner_s = inner.clone();
        selection.connect_selection_changed(move |model, _pos, _n| {
            let sel = model.selected();
            // GTK_INVALID_LIST_POSITION is defined upstream as G_MAXUINT.
            let pkg = if sel == u32::MAX {
                None
            } else {
                model
                    .selected_item()
                    .map(|obj| pkg_of(&obj).pkg().clone())
            };
            for cb in inner_s.on_package_selected.borrow().iter() {
                cb(pkg.clone());
            }
        });
    }

    let column_view = gtk::ColumnView::new(Some(selection));
    column_view.set_show_row_separators(true);
    column_view.set_show_column_separators(true);
    column_view.set_vexpand(true);

    // ── Checkbox column ──────────────────────────────────────────────
    //
    // Lets you mark/unmark several packages directly from the list. A
    // `gtk::ListItem` is recycled across rows as the view scrolls, but
    // the *same* `ListItem` handle keeps being reused for a given
    // screen row, and its `.item()` property always reflects whichever
    // `PackageObject` is currently bound to it. Capturing a clone of
    // the `ListItem` once, in `setup`, and reading `.item()` fresh
    // inside the "toggled" handler therefore always resolves the
    // correct currently-bound package — the Rust equivalent of the
    // original's `g_object_get_data(cb, "list-item")` lookup at click
    // time, without needing to stash anything by hand.
    {
        let store = inner.store.clone();
        let on_marks_changed = inner.clone();
        let col_check = make_col(
            "",
            32,
            false,
            false,
            move |item| {
                item.set_activatable(false);
                let cb = gtk::CheckButton::new();
                cb.set_halign(gtk::Align::Center);

                let li = item.clone();
                let store = store.clone();
                let on_marks_changed = on_marks_changed.clone();
                let handler_id = cb.connect_toggled(move |cb| {
                    let Some(obj) = li.item().map(|o| pkg_of(&o)) else {
                        return;
                    };
                    on_checkbox_toggled(cb, &obj, &store, &on_marks_changed);
                });
                // SAFETY: standard glib idiom for stashing per-widget
                // state that a later, separate closure (bind, below)
                // needs to retrieve — mirrors the original's
                // `g_object_set_data`/`g_object_get_data` use exactly.
                unsafe {
                    cb.set_data("toggle-handler-id", handler_id);
                }
                item.set_child(Some(&cb));
            },
            |item| {
                let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                    return;
                };
                let cb = item.child().and_downcast::<gtk::CheckButton>().unwrap();
                let p = obj.pkg();

                // Block while we set the programmatic state so this
                // rebind doesn't itself fire "toggled" (which would
                // otherwise re-open the deps-confirm dialog on every
                // scroll for an already-marked not-yet-installed row).
                let handler_id = unsafe { cb.data::<glib::SignalHandlerId>("toggle-handler-id") };
                if let Some(id) = handler_id {
                    let id_ref = unsafe { id.as_ref() };
                    cb.block_signal(id_ref);
                    cb.set_active(p.mark != PkgMark::None);
                    cb.set_sensitive(!p.essential);
                    cb.set_tooltip_text(if p.essential {
                        Some("Essential package — cannot be marked for removal")
                    } else {
                        None
                    });
                    cb.unblock_signal(id_ref);
                } else {
                    cb.set_active(p.mark != PkgMark::None);
                }
            },
        );
        column_view.append_column(&col_check);
    }

    // ── Status icon column ───────────────────────────────────────────
    let col_status = make_col(
        "",
        28,
        false,
        false,
        |item| item.set_child(Some(&gtk::Image::new())),
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let img = item.child().and_downcast::<gtk::Image>().unwrap();
            let p = obj.pkg();
            match pkg_state_icon(p.state, p.mark) {
                Some(icon) => {
                    img.set_icon_name(Some(icon));
                    img.set_tooltip_text(Some(pkg_state_tooltip(p.state, p.mark)));
                }
                None => img.clear(),
            }
        },
    );
    set_column_sorter(&col_status, |a, b| {
        let (ra, rb) = (pkg_sort_rank(a), pkg_sort_rank(b));
        if ra != rb {
            ra.cmp(&rb)
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });
    column_view.append_column(&col_status);

    // ── Package name column ──────────────────────────────────────────
    let col_name = make_col(
        "Package",
        200,
        true,
        false,
        label_cell,
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            let p = obj.pkg();
            l.set_text(&p.name);
            if p.mark != PkgMark::None {
                l.add_css_class("pkg-marked");
            } else {
                l.remove_css_class("pkg-marked");
            }
        },
    );
    set_column_sorter(&col_name, |a, b| {
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });
    column_view.append_column(&col_name);

    // ── Description column ───────────────────────────────────────────
    let col_desc = make_col(
        "Description",
        320,
        true,
        true,
        |item| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            l.add_css_class("dim-label");
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            item.set_child(Some(&l));
        },
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            l.set_text(&obj.pkg().short_desc);
        },
    );
    set_column_sorter(&col_desc, |a, b| {
        a.short_desc.to_lowercase().cmp(&b.short_desc.to_lowercase())
    });
    column_view.append_column(&col_desc);

    // ── Installed version column ─────────────────────────────────────
    let col_inst = make_col(
        "Installed",
        110,
        true,
        false,
        label_cell,
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            let p = obj.pkg();
            match &p.version_installed {
                Some(v) => {
                    l.set_text(v);
                    l.add_css_class("pkg-installed");
                }
                None => {
                    l.set_text("\u{2014}");
                    l.remove_css_class("pkg-installed");
                }
            }
        },
    );
    set_column_sorter(&col_inst, |a, b| {
        a.version_installed
            .clone()
            .unwrap_or_default()
            .cmp(&b.version_installed.clone().unwrap_or_default())
    });
    column_view.append_column(&col_inst);

    // ── Available version column ──────────────────────────────────────
    let col_avail = make_col(
        "Available",
        110,
        true,
        false,
        label_cell,
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            let p = obj.pkg();
            l.set_text(p.version_available.as_deref().unwrap_or("\u{2014}"));
            if p.state == PkgState::Upgradable {
                l.add_css_class("pkg-upgradable");
            } else {
                l.remove_css_class("pkg-upgradable");
            }
        },
    );
    set_column_sorter(&col_avail, |a, b| {
        a.version_available
            .clone()
            .unwrap_or_default()
            .cmp(&b.version_available.clone().unwrap_or_default())
    });
    column_view.append_column(&col_avail);

    // ── Sizes ──────────────────────────────────────────────────────────
    let col_isize = make_col(
        "Installed Size",
        110,
        true,
        false,
        label_cell,
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            let p = obj.pkg();
            l.set_text(&if p.install_size > 0 {
                pkg_format_size(p.install_size)
            } else {
                "\u{2014}".to_string()
            });
        },
    );
    set_column_sorter(&col_isize, |a, b| a.install_size.cmp(&b.install_size));
    column_view.append_column(&col_isize);

    let col_dsize = make_col(
        "Download Size",
        110,
        true,
        false,
        label_cell,
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };
            let l = item.child().and_downcast::<gtk::Label>().unwrap();
            let p = obj.pkg();
            l.set_text(&if p.download_size > 0 {
                pkg_format_size(p.download_size)
            } else {
                "\u{2014}".to_string()
            });
        },
    );
    set_column_sorter(&col_dsize, |a, b| a.download_size.cmp(&b.download_size));
    column_view.append_column(&col_dsize);

    // Now that every column has its own GtkSorter, hand the column
    // view's auto-managed combined sorter to the GtkSortListModel —
    // this is what makes clicking a header actually sort the list.
    sort_model.set_sorter(column_view.sorter().as_ref());

    // Sensible default ordering on first load.
    column_view.sort_by_column(Some(&col_name), gtk::SortType::Ascending);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&column_view));
    inner.widget.append(&scroll);
}

fn on_checkbox_toggled(
    cb: &gtk::CheckButton,
    obj: &PackageObject,
    store: &PackageStore,
    inner: &Rc<Inner>,
) {
    let (name, state, active) = {
        let p = obj.pkg();
        (p.name.clone(), p.state, cb.is_active())
    };

    if active {
        if state == PkgState::NotInstalled {
            // Installing something new — check whether it drags in any
            // not-yet-installed dependencies and confirm with the user
            // before marking anything. Async: the mark (or lack of
            // one) is applied in the callback, not here. If the user
            // cancels, revert the checkbox — but only if it's still
            // showing the same package (list virtualization may have
            // rebound this exact widget to a different row while the
            // modal dialog was open).
            let root = cb.root().and_downcast::<gtk::Window>();
            let store_for_call = store.clone();
            let store = store.clone();
            let obj_weak = glib::object::ObjectExt::downgrade(obj);
            let cb_weak = glib::object::ObjectExt::downgrade(cb);
            let name_for_dialog = name.clone();
            let inner = inner.clone();
            deps_confirm::confirm_install_deps(root.as_ref(), &store_for_call, &name, move |proceed| {
                if proceed {
                    store.set_mark(&name_for_dialog, PkgMark::Install);
                    for f in inner.on_marks_changed.borrow().iter() {
                        f();
                    }
                } else if let (Some(obj), Some(cb)) = (obj_weak.upgrade(), cb_weak.upgrade()) {
                    if obj.name() == name_for_dialog {
                        let handler_id =
                            unsafe { cb.data::<glib::SignalHandlerId>("toggle-handler-id") };
                        if let Some(id) = handler_id {
                            let id_ref = unsafe { id.as_ref() };
                            cb.block_signal(id_ref);
                            cb.set_active(false);
                            cb.unblock_signal(id_ref);
                        } else {
                            cb.set_active(false);
                        }
                    }
                }
            });
            return;
        }
        let mark = if state == PkgState::Upgradable {
            PkgMark::Upgrade
        } else {
            PkgMark::Remove
        };
        store.set_mark(&name, mark);
    } else {
        store.set_mark(&name, PkgMark::None);
    }

    for f in inner.on_marks_changed.borrow().iter() {
        f();
    }
}
