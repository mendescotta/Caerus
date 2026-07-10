//! The main package table: a `gtk::ColumnView` over a filter+sort model
//! chain, plus a checkbox column for marking several packages at once.
//! Rust translation of ui/package_list.{h,c}.

use crate::backend::package::{
    pkg_format_size, pkg_state_icon, pkg_state_tooltip, FilterMode, Package, PackageObject,
    PkgMark, PkgState,
};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use crate::ui::remove_confirm;
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
    /// `None` = no repository restriction ("All Repositories").
    current_repo_filter: RefCell<Option<String>>,
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
            current_repo_filter: RefCell::new(None),
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
    pub fn set_repository_filter(&self, repo: Option<String>) {
        *self.inner.current_repo_filter.borrow_mut() = repo;
        self.inner
            .custom_filter
            .changed(gtk::FilterChange::Different);
    }

    /// Distinct, non-empty `repository` values currently in the store,
    /// sorted — used to populate `FilterSidebar`'s repository rows
    /// after each load.
    pub fn available_repositories(&self) -> Vec<String> {
        let mut set = std::collections::HashSet::new();
        let n = self.inner.store.list().n_items();
        for i in 0..n {
            if let Some(obj) = self.inner.store.list().item(i) {
                if let Some(repo) = &pkg_of(&obj).pkg().repository {
                    set.insert(repo.clone());
                }
            }
        }
        let mut out: Vec<String> = set.into_iter().collect();
        out.sort();
        out
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
            if let Some(repo) = inner_f.current_repo_filter.borrow().as_deref() {
                if p.repository.as_deref() != Some(repo) {
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

    let column_view = gtk::ColumnView::new(Some(selection.clone()));
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
    // Also carries a right-click context menu (Mark for Install/
    // Upgrade/Removal/Purge/Unmark) — same identity-by-`item.item()`
    // trick as the checkbox column above, so the menu always acts on
    // whichever package is currently bound to this row, not whichever
    // one was there when the row widget was first created.
    let col_name = make_col(
        "Package",
        200,
        true,
        false,
        {
            let inner = inner.clone();
            let selection = selection.clone();
            move |item| {
                let l = gtk::Label::new(None);
                l.set_xalign(0.0);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                item.set_child(Some(&l));

                let gesture = gtk::GestureClick::new();
                gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
                let li = item.clone();
                let inner = inner.clone();
                let selection = selection.clone();
                gesture.connect_pressed(move |g, _n_press, x, y| {
                    let Some(obj) = li.item().map(|o| pkg_of(&o)) else {
                        return;
                    };
                    selection.set_selected(li.position());
                    let Some(widget) = g.widget() else { return };
                    show_context_menu(&widget, x, y, &inner, &obj);
                });
                l.add_controller(gesture);
            }
        },
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

    // Double-click (or Enter on the selected row) is a quick shortcut
    // for the same toggle the checkbox column and context menu offer —
    // `single-click-activate` defaults to false, so this only fires on
    // an actual double-click/Enter, never a plain single click (which
    // only changes selection, handled above).
    {
        let inner = inner.clone();
        let selection = selection.clone();
        column_view.connect_activate(move |view, position| {
            let Some(obj) = selection.item(position).map(|o| pkg_of(&o)) else {
                return;
            };
            let root = view.root().and_downcast::<gtk::Window>();
            toggle_mark(root, &inner.store, &inner, &obj);
        });
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&column_view));
    inner.widget.append(&scroll);
}

/// Double-click (or Enter) shortcut: clears an existing mark, or
/// applies the obvious one for the package's current state (Install,
/// with the same deps confirmation as everywhere else; Upgrade if one's
/// available; otherwise Remove, unless the package is essential — same
/// guard the checkbox column and detail pane apply).
fn toggle_mark(root: Option<gtk::Window>, store: &PackageStore, inner: &Rc<Inner>, obj: &PackageObject) {
    let (name, state, mark, essential) = {
        let p = obj.pkg();
        (p.name.clone(), p.state, p.mark, p.essential)
    };

    if mark != PkgMark::None {
        set_mark_and_notify(store, inner, &name, PkgMark::None);
        return;
    }

    match state {
        PkgState::NotInstalled => request_install_with_confirm(root, store, inner, &name, |_| {}),
        PkgState::Upgradable => set_mark_and_notify(store, inner, &name, PkgMark::Upgrade),
        _ if !essential => {
            request_remove_with_confirm(root, store, inner, &name, PkgMark::Remove, |_| {})
        }
        _ => {} // essential and already installed: no quick action
    }
}

/// Sets `mark` on `pkgname` and fires every registered
/// `on_marks_changed` listener. Shared by the checkbox column and the
/// right-click context menu.
fn set_mark_and_notify(store: &PackageStore, inner: &Rc<Inner>, pkgname: &str, mark: PkgMark) {
    store.set_mark(pkgname, mark);
    for f in inner.on_marks_changed.borrow().iter() {
        f();
    }
}

/// Marking a not-yet-installed package for install first checks whether
/// it drags in further not-yet-installed dependencies and confirms with
/// the user (see `deps_confirm`). Asynchronous: `on_result` fires with
/// `true` if the mark was actually applied, `false` if the user
/// canceled — the caller decides what, if anything, to do with either
/// outcome (the checkbox column reverts its checkbox; the context menu
/// has nothing to revert).
fn request_install_with_confirm(
    root: Option<gtk::Window>,
    store: &PackageStore,
    inner: &Rc<Inner>,
    pkgname: &str,
    on_result: impl Fn(bool) + 'static,
) {
    let store_for_call = store.clone();
    let store = store.clone();
    let inner = inner.clone();
    let name = pkgname.to_string();
    deps_confirm::confirm_install_deps(root.as_ref(), &store_for_call, pkgname, move |proceed| {
        if proceed {
            set_mark_and_notify(&store, &inner, &name, PkgMark::Install);
        }
        on_result(proceed);
    });
}

/// Marking an installed package for Remove/Purge first checks whether
/// any other still-to-be-installed package depends on it (see
/// `remove_confirm`). Same `on_result(applied)` shape as
/// `request_install_with_confirm` above.
fn request_remove_with_confirm(
    root: Option<gtk::Window>,
    store: &PackageStore,
    inner: &Rc<Inner>,
    pkgname: &str,
    mark: PkgMark,
    on_result: impl Fn(bool) + 'static,
) {
    let store_for_call = store.clone();
    let store = store.clone();
    let inner = inner.clone();
    let name = pkgname.to_string();
    remove_confirm::confirm_remove_impact(root.as_ref(), &store_for_call, pkgname, move |proceed| {
        if proceed {
            set_mark_and_notify(&store, &inner, &name, mark);
        }
        on_result(proceed);
    });
}

/// (button label, target mark, enabled) for a package's current
/// mark/state — same precedence `DetailPane::update_action_buttons`
/// uses: a pending mark shows only Unmark; otherwise the options depend
/// on install state, with Removal/Purge disabled for essential packages.
fn context_menu_items(pkg: &Package) -> Vec<(&'static str, PkgMark, bool)> {
    if pkg.mark != PkgMark::None {
        return vec![("Unmark", PkgMark::None, true)];
    }
    match pkg.state {
        PkgState::NotInstalled => vec![("Mark for Installation", PkgMark::Install, true)],
        PkgState::Upgradable => vec![
            ("Mark for Upgrade", PkgMark::Upgrade, true),
            ("Mark for Removal", PkgMark::Remove, !pkg.essential),
            ("Mark for Purge", PkgMark::Purge, !pkg.essential),
        ],
        _ => vec![
            ("Mark for Removal", PkgMark::Remove, !pkg.essential),
            ("Mark for Purge", PkgMark::Purge, !pkg.essential),
        ],
    }
}

/// Builds and pops up a small right-click menu, anchored at `(x, y)`
/// within `widget`, offering the contextual mark actions for `obj`. A
/// fresh `gtk::Popover` per invocation (rather than one reused instance)
/// keeps this stateless between rows; `connect_closed` unparents it so
/// it doesn't linger once dismissed.
fn show_context_menu(widget: &gtk::Widget, x: f64, y: f64, inner: &Rc<Inner>, obj: &PackageObject) {
    let pkg = obj.pkg().clone();
    let items = context_menu_items(&pkg);

    let popover = gtk::Popover::new();
    popover.set_parent(widget);
    popover.set_has_arrow(true);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| p.unparent());

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);

    let root = widget.root().and_downcast::<gtk::Window>();
    for (label, mark, enabled) in items {
        let btn = gtk::Button::with_label(label);
        btn.set_has_frame(false);
        if let Some(l) = btn.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        btn.set_sensitive(enabled);

        let store = inner.store.clone();
        let inner = inner.clone();
        let name = pkg.name.clone();
        let root = root.clone();
        let popover_weak = popover.downgrade();
        btn.connect_clicked(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.popdown();
            }
            match mark {
                PkgMark::Install => {
                    request_install_with_confirm(root.clone(), &store, &inner, &name, |_| {})
                }
                PkgMark::Remove | PkgMark::Purge => {
                    request_remove_with_confirm(root.clone(), &store, &inner, &name, mark, |_| {})
                }
                _ => set_mark_and_notify(&store, &inner, &name, mark),
            }
        });
        vbox.append(&btn);
    }

    popover.set_child(Some(&vbox));
    popover.popup();
}

/// Unchecks `cb` without re-firing its own "toggled" handler — but only
/// if it's still showing the same package `expected_name` was bound to
/// when the async confirmation dialog was opened (list virtualization
/// may have rebound this exact widget to a different row while the
/// modal was up).
fn revert_checkbox_if_still_bound(
    obj_weak: &glib::object::WeakRef<PackageObject>,
    cb_weak: &glib::object::WeakRef<gtk::CheckButton>,
    expected_name: &str,
) {
    let (Some(obj), Some(cb)) = (obj_weak.upgrade(), cb_weak.upgrade()) else {
        return;
    };
    if obj.name() != expected_name {
        return;
    }
    let handler_id = unsafe { cb.data::<glib::SignalHandlerId>("toggle-handler-id") };
    if let Some(id) = handler_id {
        let id_ref = unsafe { id.as_ref() };
        cb.block_signal(id_ref);
        cb.set_active(false);
        cb.unblock_signal(id_ref);
    } else {
        cb.set_active(false);
    }
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

    if !active {
        set_mark_and_notify(store, inner, &name, PkgMark::None);
        return;
    }

    if state == PkgState::Upgradable {
        set_mark_and_notify(store, inner, &name, PkgMark::Upgrade);
        return;
    }

    // Both remaining cases (installing something new, or removing an
    // installed one) go through an async confirmation first — deps for
    // installs, reverse-deps impact for removals — and revert the
    // checkbox on cancel using the same shared helper.
    let root = cb.root().and_downcast::<gtk::Window>();
    let obj_weak = glib::object::ObjectExt::downgrade(obj);
    let cb_weak = glib::object::ObjectExt::downgrade(cb);
    let name_for_revert = name.clone();
    let on_result = move |proceed: bool| {
        if !proceed {
            revert_checkbox_if_still_bound(&obj_weak, &cb_weak, &name_for_revert);
        }
    };
    if state == PkgState::NotInstalled {
        request_install_with_confirm(root, store, inner, &name, on_result);
    } else {
        request_remove_with_confirm(root, store, inner, &name, PkgMark::Remove, on_result);
    }
}
