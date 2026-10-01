use crate::backend::custom_filters::{filter_hides, ActiveFilter};
use crate::backend::package::{
    pkg_format_size, pkg_state_icon, pkg_state_tooltip, FilterMode, Package, PackageObject,
    PkgMark, PkgState, ORPHANED_MAINTAINER,
};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use crate::ui::remove_confirm;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering as CmpOrdering;
use std::rc::Rc;

type PackageSelectedCbs = RefCell<Vec<Box<dyn Fn(Option<Package>)>>>;
type MarksChangedCbs = RefCell<Vec<Box<dyn Fn()>>>;

struct Inner {
    widget: gtk::Box,
    store: PackageStore,
    custom_filter: gtk::CustomFilter,
    current_filter: RefCell<ActiveFilter>,
    current_search: RefCell<String>,
    search_name_only: Cell<bool>,
    current_repo_filter: RefCell<Option<String>>,
    selection: RefCell<Option<gtk::MultiSelection>>,
    column_view: RefCell<Option<gtk::ColumnView>>,
    on_package_selected: PackageSelectedCbs,
    on_marks_changed: MarksChangedCbs,
}

#[derive(Clone)]
pub struct PackageList {
    inner: Rc<Inner>,
}

const fn ord(c: CmpOrdering) -> gtk::Ordering {
    match c {
        CmpOrdering::Less => gtk::Ordering::Smaller,
        CmpOrdering::Equal => gtk::Ordering::Equal,
        CmpOrdering::Greater => gtk::Ordering::Larger,
    }
}

fn pkg_of(obj: &glib::Object) -> PackageObject {
    obj.clone().downcast::<PackageObject>().unwrap()
}

fn cmp_opt_version(a: Option<&str>, b: Option<&str>) -> CmpOrdering {
    match (a, b) {
        (None, None) => CmpOrdering::Equal,
        (None, Some(_)) => CmpOrdering::Less,
        (Some(_), None) => CmpOrdering::Greater,
        (Some(x), Some(y)) => crate::backend::package_store::compare_versions(x, y),
    }
}

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

fn set_column_sorter(
    col: &gtk::ColumnViewColumn,
    cmp: impl Fn(&Package, &Package) -> CmpOrdering + 'static,
) {
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
        let custom_filter = gtk::CustomFilter::new(|_| true);
        let inner = Rc::new(Inner {
            widget: gtk::Box::new(gtk::Orientation::Vertical, 0),
            store,
            custom_filter,
            current_filter: RefCell::new(ActiveFilter::Preset(FilterMode::All)),
            current_search: RefCell::new(String::new()),
            search_name_only: Cell::new(false),
            current_repo_filter: RefCell::new(None),
            selection: RefCell::new(None),
            column_view: RefCell::new(None),
            on_package_selected: RefCell::new(Vec::new()),
            on_marks_changed: RefCell::new(Vec::new()),
        });

        build(inner.clone());

        Self { inner }
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

    pub fn select_all(&self) {
        if let Some(selection) = self.inner.selection.borrow().as_ref() {
            selection.select_all();
        }
    }

    pub fn select_package_by_name(&self, pkgname: &str) -> bool {
        let (Some(selection), Some(column_view)) = (
            self.inner.selection.borrow().clone(),
            self.inner.column_view.borrow().clone(),
        ) else {
            return false;
        };
        let n = selection.n_items();
        for i in 0..n {
            let Some(obj) = selection.item(i) else {
                continue;
            };
            if pkg_of(&obj).name() == pkgname {
                selection.select_item(i, true);
                column_view.scroll_to(i, None, gtk::ListScrollFlags::FOCUS, None);
                return true;
            }
        }
        false
    }

    pub fn request_remove(&self, root: Option<gtk::Window>, pkg: &Package) {
        if mark_applies_to(pkg, PkgMark::Remove) {
            request_remove_with_confirm(
                root,
                &self.inner.store,
                &self.inner,
                &pkg.name,
                PkgMark::Remove,
                |_| {},
            );
        }
    }

    pub fn delete_selected(&self, root: Option<gtk::Window>) {
        let pkgs = {
            let selection = self.inner.selection.borrow();
            let Some(selection) = selection.as_ref() else {
                return;
            };
            selected_packages(selection)
        };
        match pkgs.as_slice() {
            [] => {}
            [pkg] => self.request_remove(root, pkg),
            _ => request_bulk_remove_with_confirm(
                root,
                &self.inner.store,
                &self.inner,
                &pkgs,
                PkgMark::Remove,
            ),
        }
    }

    pub fn set_filter(&self, filter: ActiveFilter) {
        let filter = match filter {
            ActiveFilter::Custom {
                name,
                patterns,
                kind,
            } => ActiveFilter::Custom {
                name,
                kind,
                patterns: patterns.into_iter().map(|p| p.to_lowercase()).collect(),
            },
            preset => preset,
        };
        *self.inner.current_filter.borrow_mut() = filter;
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
    pub fn has_active_search(&self) -> bool {
        !self.inner.current_search.borrow().is_empty()
    }

    pub fn has_active_filters(&self) -> bool {
        self.has_active_search()
            || !matches!(
                &*self.inner.current_filter.borrow(),
                ActiveFilter::Preset(FilterMode::All)
            )
            || self.inner.current_repo_filter.borrow().is_some()
    }

    pub fn visible_counts(&self) -> (u32, u32, u32) {
        let mut installed = 0u32;
        let mut not_installed = 0u32;
        let mut total = 0u32;
        if let Some(selection) = self.inner.selection.borrow().as_ref() {
            let n = selection.n_items();
            for i in 0..n {
                let Some(obj) = selection.item(i) else {
                    continue;
                };
                total += 1;
                match pkg_of(&obj).pkg().state {
                    PkgState::NotInstalled => not_installed += 1,
                    _ => installed += 1,
                }
            }
        }
        (total, installed, not_installed)
    }

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
            match &*inner_f.current_filter.borrow() {
                ActiveFilter::Preset(FilterMode::All) => true,
                ActiveFilter::Preset(FilterMode::Installed) => {
                    matches!(p.state, PkgState::Installed | PkgState::Upgradable)
                }
                ActiveFilter::Preset(FilterMode::NotInstalled) => p.state == PkgState::NotInstalled,
                ActiveFilter::Preset(FilterMode::Upgradable) => p.state == PkgState::Upgradable,
                ActiveFilter::Preset(FilterMode::OnHold) => p.state == PkgState::OnHold,
                ActiveFilter::Preset(FilterMode::Marked) => p.mark != PkgMark::None,
                ActiveFilter::Preset(FilterMode::Orphaned) => p.is_orphan,
                ActiveFilter::Preset(FilterMode::RepoLocked) => p.is_repolocked,
                ActiveFilter::Preset(FilterMode::Unmaintained) => {
                    p.maintainer == ORPHANED_MAINTAINER
                }
                ActiveFilter::Custom { patterns, kind, .. } => {
                    !filter_hides(*kind, patterns, &p.name)
                }
            }
        });
    }

    let filter_model =
        gtk::FilterListModel::new(Some(inner.store.list()), Some(inner.custom_filter.clone()));
    let sort_model = gtk::SortListModel::new(Some(filter_model), None::<gtk::Sorter>);

    let selection = gtk::MultiSelection::new(Some(sort_model.clone()));
    *inner.selection.borrow_mut() = Some(selection.clone());

    {
        let inner_s = inner.clone();
        selection.connect_selection_changed(move |model, _pos, _n| {
            let bitset = model.selection();
            let pkg = if bitset.size() == 1 {
                model
                    .item(bitset.minimum())
                    .map(|obj| pkg_of(&obj).pkg().clone())
            } else {
                None
            };
            for cb in inner_s.on_package_selected.borrow().iter() {
                cb(pkg.clone());
            }
        });
    }

    let column_view = gtk::ColumnView::new(Some(selection.clone()));
    *inner.column_view.borrow_mut() = Some(column_view.clone());
    column_view.set_show_row_separators(true);
    column_view.set_show_column_separators(true);
    column_view.set_vexpand(true);

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
                unsafe {
                    cb.set_data("toggle-handler-id", handler_id);
                }
                item.set_child(Some(&cb));
            },
            |item| {
                let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                    return;
                };

                let Some(cb) = crate::ui::dialog_util::expect_item_child::<gtk::CheckButton>(item)
                else {
                    return;
                };
                let p = obj.pkg();

                let would_remove = p.mark == PkgMark::None
                    && p.state != PkgState::Upgradable
                    && p.state != PkgState::NotInstalled;
                let blocked = p.essential && would_remove;

                let handler_id = unsafe { cb.data::<glib::SignalHandlerId>("toggle-handler-id") };
                if let Some(id) = handler_id {
                    let id_ref = unsafe { id.as_ref() };
                    cb.block_signal(id_ref);
                    cb.set_active(p.mark != PkgMark::None);
                    cb.set_sensitive(!blocked);
                    cb.set_tooltip_text(if blocked {
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

            let Some(img) = crate::ui::dialog_util::expect_item_child::<gtk::Image>(item) else {
                return;
            };
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
        if ra == rb {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            ra.cmp(&rb)
        }
    });
    column_view.append_column(&col_status);

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
                    if li.item().is_none() {
                        return;
                    }
                    let pos = li.position();
                    if !selection.is_selected(pos) || selection.selection().size() <= 1 {
                        selection.select_item(pos, true);
                    }
                    let selected = selected_packages(&selection);
                    let Some(widget) = g.widget() else { return };
                    show_context_menu(&widget, x, y, &inner, selected);
                });
                l.add_controller(gesture);
            }
        },
        |item| {
            let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
                return;
            };

            let Some(l) = crate::ui::dialog_util::expect_item_child::<gtk::Label>(item) else {
                return;
            };
            let p = obj.pkg();
            l.set_text(&p.name);
            if p.mark == PkgMark::None {
                l.remove_css_class("pkg-marked");
            } else {
                l.add_css_class("pkg-marked");
            }
        },
    );
    set_column_sorter(&col_name, |a, b| {
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });
    column_view.append_column(&col_name);

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

            let Some(l) = crate::ui::dialog_util::expect_item_child::<gtk::Label>(item) else {
                return;
            };
            l.set_text(&obj.pkg().short_desc);
        },
    );
    set_column_sorter(&col_desc, |a, b| {
        a.short_desc
            .to_lowercase()
            .cmp(&b.short_desc.to_lowercase())
    });
    column_view.append_column(&col_desc);

    let col_inst = make_col("Installed", 110, true, false, label_cell, |item| {
        let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
            return;
        };

        let Some(l) = crate::ui::dialog_util::expect_item_child::<gtk::Label>(item) else {
            return;
        };
        let p = obj.pkg();
        if let Some(v) = &p.version_installed {
            l.set_text(v);
            l.add_css_class("pkg-installed");
        } else {
            l.set_text("\u{2014}");
            l.remove_css_class("pkg-installed");
        }
    });
    set_column_sorter(&col_inst, |a, b| {
        cmp_opt_version(
            a.version_installed.as_deref(),
            b.version_installed.as_deref(),
        )
    });
    column_view.append_column(&col_inst);

    let col_avail = make_col("Available", 110, true, false, label_cell, |item| {
        let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
            return;
        };

        let Some(l) = crate::ui::dialog_util::expect_item_child::<gtk::Label>(item) else {
            return;
        };
        let p = obj.pkg();
        l.set_text(p.version_available.as_deref().unwrap_or("\u{2014}"));
        if p.state == PkgState::Upgradable {
            l.add_css_class("pkg-upgradable");
        } else {
            l.remove_css_class("pkg-upgradable");
        }
    });
    set_column_sorter(&col_avail, |a, b| {
        cmp_opt_version(
            a.version_available.as_deref(),
            b.version_available.as_deref(),
        )
    });
    column_view.append_column(&col_avail);

    let col_isize = make_col("Installed Size", 110, true, false, label_cell, |item| {
        let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
            return;
        };

        let Some(l) = crate::ui::dialog_util::expect_item_child::<gtk::Label>(item) else {
            return;
        };
        let p = obj.pkg();
        l.set_text(&if p.install_size > 0 {
            pkg_format_size(p.install_size)
        } else {
            "\u{2014}".to_string()
        });
    });
    set_column_sorter(&col_isize, |a, b| a.install_size.cmp(&b.install_size));
    column_view.append_column(&col_isize);

    let col_dsize = make_col("Download Size", 110, true, false, label_cell, |item| {
        let Some(obj) = item.item().map(|o| pkg_of(&o)) else {
            return;
        };

        let Some(l) = crate::ui::dialog_util::expect_item_child::<gtk::Label>(item) else {
            return;
        };
        let p = obj.pkg();
        l.set_text(&if p.download_size > 0 {
            pkg_format_size(p.download_size)
        } else {
            "\u{2014}".to_string()
        });
    });
    set_column_sorter(&col_dsize, |a, b| a.download_size.cmp(&b.download_size));
    column_view.append_column(&col_dsize);

    sort_model.set_sorter(column_view.sorter().as_ref());

    column_view.sort_by_column(Some(&col_name), gtk::SortType::Ascending);

    {
        let inner = inner.clone();
        let selection = selection;
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

    if let Some(sorter) = column_view.sorter() {
        let vadj = scroll.vadjustment();
        sorter.connect_changed(move |_, _| {
            let saved = vadj.value();
            let vadj = vadj.clone();
            glib::source::idle_add_local_once(move || {
                vadj.set_value(saved);
            });
        });
    }
}

fn toggle_mark(
    root: Option<gtk::Window>,
    store: &PackageStore,
    inner: &Rc<Inner>,
    obj: &PackageObject,
) {
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
            request_remove_with_confirm(root, store, inner, &name, PkgMark::Remove, |_| {});
        }
        _ => {}
    }
}

fn set_mark_and_notify(store: &PackageStore, inner: &Rc<Inner>, pkgname: &str, mark: PkgMark) {
    store.set_mark(pkgname, mark);
    for f in inner.on_marks_changed.borrow().iter() {
        f();
    }
}

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
    remove_confirm::confirm_remove_impact(
        root.as_ref(),
        &store_for_call,
        pkgname,
        move |proceed| {
            if proceed {
                set_mark_and_notify(&store, &inner, &name, mark);
            }
            on_result(proceed);
        },
    );
}

pub fn mark_applies_to(pkg: &Package, mark: PkgMark) -> bool {
    match mark {
        PkgMark::Install => pkg.state == PkgState::NotInstalled && pkg.mark == PkgMark::None,
        PkgMark::Upgrade => pkg.state == PkgState::Upgradable && pkg.mark == PkgMark::None,
        PkgMark::Remove | PkgMark::Purge => {
            pkg.state != PkgState::NotInstalled && pkg.mark == PkgMark::None && !pkg.essential
        }
        PkgMark::None => pkg.mark != PkgMark::None,
    }
}

fn context_menu_items(pkgs: &[Package]) -> Vec<(String, PkgMark)> {
    let multi = pkgs.len() > 1;
    let mut items = Vec::new();
    for mark in [
        PkgMark::Install,
        PkgMark::Upgrade,
        PkgMark::Remove,
        PkgMark::Purge,
        PkgMark::None,
    ] {
        let n = pkgs.iter().filter(|p| mark_applies_to(p, mark)).count();
        if n == 0 {
            continue;
        }
        let label = match (mark, multi) {
            (PkgMark::Install, false) => "Mark for Installation".to_string(),
            (PkgMark::Install, true) => format!("Mark {n} for Installation"),
            (PkgMark::Upgrade, false) => "Mark for Upgrade".to_string(),
            (PkgMark::Upgrade, true) => format!("Mark {n} for Upgrade"),
            (PkgMark::Remove, false) => "Mark for Removal".to_string(),
            (PkgMark::Remove, true) => format!("Mark {n} for Removal"),
            (PkgMark::Purge, false) => "Mark for Purge".to_string(),
            (PkgMark::Purge, true) => format!("Mark {n} for Purge"),
            (_, false) => "Unmark".to_string(),
            (_, true) => format!("Unmark {n}"),
        };
        items.push((label, mark));
    }
    items
}

fn request_bulk_remove_with_confirm(
    root: Option<gtk::Window>,
    store: &PackageStore,
    inner: &Rc<Inner>,
    pkgs: &[Package],
    mark: PkgMark,
) {
    let names: Vec<String> = pkgs
        .iter()
        .filter(|p| mark_applies_to(p, mark))
        .map(|p| p.name.clone())
        .collect();
    if names.is_empty() {
        return;
    }
    let store_for_call = store.clone();
    let store = store.clone();
    let inner = inner.clone();
    let pkgs = pkgs.to_vec();
    remove_confirm::confirm_bulk_remove_impact(
        root.as_ref(),
        &store_for_call,
        names,
        move |proceed| {
            if proceed {
                apply_bulk_mark(&store, &inner, &pkgs, mark);
            }
        },
    );
}

fn apply_bulk_mark(store: &PackageStore, inner: &Rc<Inner>, pkgs: &[Package], mark: PkgMark) {
    let names: std::collections::HashSet<String> = pkgs
        .iter()
        .filter(|p| mark_applies_to(p, mark))
        .map(|p| p.name.clone())
        .collect();
    store.set_marks(&names, mark);
    for f in inner.on_marks_changed.borrow().iter() {
        f();
    }
}

fn selected_packages(selection: &gtk::MultiSelection) -> Vec<Package> {
    let n = selection.n_items();
    let mut out = Vec::new();
    for i in 0..n {
        if selection.is_selected(i) {
            if let Some(obj) = selection.item(i) {
                out.push(pkg_of(&obj).pkg().clone());
            }
        }
    }
    out
}

fn show_context_menu(
    widget: &gtk::Widget,
    x: f64,
    y: f64,
    inner: &Rc<Inner>,
    selected: Vec<Package>,
) {
    if selected.is_empty() {
        return;
    }
    let items = context_menu_items(&selected);
    if items.is_empty() {
        return;
    }

    let popover = gtk::Popover::new();
    popover.set_parent(widget);
    popover.set_has_arrow(true);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(gtk::prelude::WidgetExt::unparent);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);

    let root = widget.root().and_downcast::<gtk::Window>();
    let selected = Rc::new(selected);
    for (label, mark) in items {
        let btn = gtk::Button::with_label(&label);
        btn.set_has_frame(false);
        if let Some(l) = btn.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }

        let store = inner.store.clone();
        let inner = inner.clone();
        let root = root.clone();
        let selected = selected.clone();
        let popover_weak = popover.downgrade();
        btn.connect_clicked(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.popdown();
            }
            if selected.len() == 1 {
                let name = selected[0].name.clone();
                match mark {
                    PkgMark::Install => {
                        request_install_with_confirm(root.clone(), &store, &inner, &name, |_| {});
                    }
                    PkgMark::Remove | PkgMark::Purge => request_remove_with_confirm(
                        root.clone(),
                        &store,
                        &inner,
                        &name,
                        mark,
                        |_| {},
                    ),
                    _ => set_mark_and_notify(&store, &inner, &name, mark),
                }
            } else {
                match mark {
                    PkgMark::Remove | PkgMark::Purge => {
                        request_bulk_remove_with_confirm(
                            root.clone(),
                            &store,
                            &inner,
                            &selected,
                            mark,
                        );
                    }
                    _ => apply_bulk_mark(&store, &inner, &selected, mark),
                }
            }
        });
        vbox.append(&btn);
    }

    popover.set_child(Some(&vbox));
    popover.popup();
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, state: PkgState, mark: PkgMark, essential: bool) -> Package {
        Package {
            name: name.to_string(),
            state,
            mark,
            essential,
            ..Default::default()
        }
    }

    #[test]
    fn install_applies_only_to_unmarked_not_installed() {
        let p = pkg("a", PkgState::NotInstalled, PkgMark::None, false);
        assert!(mark_applies_to(&p, PkgMark::Install));
        let installed = pkg("a", PkgState::Installed, PkgMark::None, false);
        assert!(!mark_applies_to(&installed, PkgMark::Install));
        let already = pkg("a", PkgState::NotInstalled, PkgMark::Install, false);
        assert!(!mark_applies_to(&already, PkgMark::Install));
    }

    #[test]
    fn upgrade_applies_only_to_unmarked_upgradable() {
        assert!(mark_applies_to(
            &pkg("a", PkgState::Upgradable, PkgMark::None, false),
            PkgMark::Upgrade
        ));
        assert!(!mark_applies_to(
            &pkg("a", PkgState::Installed, PkgMark::None, false),
            PkgMark::Upgrade
        ));
        assert!(!mark_applies_to(
            &pkg("a", PkgState::Upgradable, PkgMark::Upgrade, false),
            PkgMark::Upgrade
        ));
    }

    #[test]
    fn remove_and_purge_spare_essential_and_not_installed() {
        for mark in [PkgMark::Remove, PkgMark::Purge] {
            assert!(mark_applies_to(
                &pkg("a", PkgState::Installed, PkgMark::None, false),
                mark
            ));
            for state in [PkgState::Upgradable, PkgState::OnHold, PkgState::Broken] {
                assert!(mark_applies_to(
                    &pkg("a", state, PkgMark::None, false),
                    mark
                ));
            }
            assert!(!mark_applies_to(
                &pkg("a", PkgState::NotInstalled, PkgMark::None, false),
                mark
            ));
            assert!(!mark_applies_to(
                &pkg("a", PkgState::Installed, PkgMark::None, true),
                mark
            ));
            assert!(!mark_applies_to(
                &pkg("a", PkgState::Installed, PkgMark::Remove, false),
                mark
            ));
        }
    }

    #[test]
    fn unmark_applies_only_to_marked() {
        assert!(mark_applies_to(
            &pkg("a", PkgState::Installed, PkgMark::Remove, false),
            PkgMark::None
        ));
        assert!(!mark_applies_to(
            &pkg("a", PkgState::Installed, PkgMark::None, false),
            PkgMark::None
        ));
    }

    #[test]
    fn single_selection_menu_uses_singular_labels() {
        let items = context_menu_items(&[pkg("a", PkgState::Installed, PkgMark::None, false)]);
        let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Mark for Removal", "Mark for Purge"]);
    }

    #[test]
    fn multi_selection_menu_counts_only_applicable_packages() {
        let sel = [
            pkg("a", PkgState::NotInstalled, PkgMark::None, false),
            pkg("b", PkgState::NotInstalled, PkgMark::None, false),
            pkg("c", PkgState::Installed, PkgMark::None, false),
        ];
        let items = context_menu_items(&sel);
        let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Mark 2 for Installation",
                "Mark 1 for Removal",
                "Mark 1 for Purge"
            ]
        );
    }

    #[test]
    fn menu_omits_marks_that_apply_to_nothing() {
        let items = context_menu_items(&[pkg("a", PkgState::Installed, PkgMark::None, true)]);
        assert!(items.is_empty());

        let items = context_menu_items(&[
            pkg("a", PkgState::Installed, PkgMark::Remove, false),
            pkg("b", PkgState::NotInstalled, PkgMark::Install, false),
        ]);
        let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Unmark 2"]);
    }
}
