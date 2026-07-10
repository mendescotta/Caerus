//! Detail pane: action buttons + Package Information / Size Statistics
//! / Maintainer & Source frames + side-by-side Dependencies / Reverse
//! Dependencies lists. Rust translation of ui/detail_pane.{h,c} (built
//! directly in code here rather than from a GtkBuilder .ui file).

use crate::backend::package::{Package, PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const DASH: &str = "\u{2014}";

struct Labels {
    name: gtk::Label,
    version: gtk::Label,
    tags: gtk::Label,
    state: gtk::Label,
    desc: gtk::Label,
    installed_size: gtk::Label,
    download_size: gtk::Label,
    maintainer: gtk::Label,
    homepage_box: gtk::Box,
    license: gtk::Label,
    repository: gtk::Label,
    install_date: gtk::Label,
    auto_install: gtk::Label,
}

struct Inner {
    widget: gtk::Box,
    store: PackageStore,
    current_pkgname: RefCell<Option<String>>,
    btn_install: gtk::Button,
    btn_upgrade: gtk::Button,
    btn_remove: gtk::Button,
    btn_unmark: gtk::Button,
    labels: Labels,
    deps_list: gtk::ListBox,
    rdeps_list: gtk::ListBox,
    on_mark_changed: RefCell<Vec<Box<dyn Fn()>>>,
}

#[derive(Clone)]
pub struct DetailPane {
    inner: Rc<Inner>,
}

fn info_row(container: &gtk::Box, label: &str) -> gtk::Label {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let key = gtk::Label::new(Some(label));
    key.set_width_chars(13);
    key.set_xalign(1.0);
    key.add_css_class("dim-label");
    row.append(&key);

    let val = gtk::Label::new(Some(DASH));
    val.set_xalign(0.0);
    val.set_selectable(true);
    val.set_wrap(true);
    val.set_hexpand(true);
    row.append(&val);

    container.append(&row);
    val
}

fn framed(title: &str) -> (gtk::Frame, gtk::Box) {
    let frame = gtk::Frame::new(Some(title));
    frame.set_margin_bottom(6);
    let inner_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    inner_box.set_margin_start(8);
    inner_box.set_margin_end(8);
    inner_box.set_margin_top(6);
    inner_box.set_margin_bottom(8);
    frame.set_child(Some(&inner_box));
    (frame, inner_box)
}

impl DetailPane {
    pub fn new(store: PackageStore) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_height_request(220);
        widget.set_margin_start(12);
        widget.set_margin_end(12);
        widget.set_margin_top(10);
        widget.set_margin_bottom(10);

        // ── Action row ──
        let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        action_row.set_margin_bottom(8);
        let btn_install = gtk::Button::with_label("Install");
        btn_install.set_visible(false);
        btn_install.add_css_class("suggested-action");
        let btn_upgrade = gtk::Button::with_label("Upgrade");
        btn_upgrade.set_visible(false);
        btn_upgrade.add_css_class("suggested-action");
        let btn_remove = gtk::Button::with_label("Remove");
        btn_remove.set_visible(false);
        btn_remove.add_css_class("destructive-action");
        let btn_unmark = gtk::Button::with_label("Unmark");
        btn_unmark.set_visible(false);
        action_row.append(&btn_install);
        action_row.append(&btn_upgrade);
        action_row.append(&btn_remove);
        action_row.append(&btn_unmark);
        widget.append(&action_row);

        // ── Split: metadata column | dependency column ──
        let split = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        split.set_vexpand(true);

        let metadata_scroll = gtk::ScrolledWindow::new();
        metadata_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let metadata_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        metadata_col.set_width_request(320);

        let (frame_info, box_info) = framed("Package Information");
        let name = info_row(&box_info, "Name:");
        let version = info_row(&box_info, "Version:");
        let tags = info_row(&box_info, "Tags:");
        let state = info_row(&box_info, "State:");
        box_info.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let desc = gtk::Label::new(Some("Select a package to view details."));
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(gtk::pango::WrapMode::Word);
        desc.set_selectable(true);
        box_info.append(&desc);
        metadata_col.append(&frame_info);

        let (frame_sizes, box_sizes) = framed("Size Statistics");
        let installed_size = info_row(&box_sizes, "Installed:");
        let download_size = info_row(&box_sizes, "Download:");
        metadata_col.append(&frame_sizes);

        let (frame_maint, box_maint) = framed("Maintainer & Source");
        let maintainer = info_row(&box_maint, "Maintainer:");

        let homepage_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let homepage_key = gtk::Label::new(Some("Homepage:"));
        homepage_key.set_width_chars(13);
        homepage_key.set_xalign(1.0);
        homepage_key.add_css_class("dim-label");
        homepage_row.append(&homepage_key);
        let homepage_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        homepage_box.set_hexpand(true);
        homepage_row.append(&homepage_box);
        box_maint.append(&homepage_row);

        let license = info_row(&box_maint, "License:");
        let repository = info_row(&box_maint, "Repository:");
        let install_date = info_row(&box_maint, "Installed on:");
        let auto_install = info_row(&box_maint, "Auto-installed:");
        metadata_col.append(&frame_maint);

        metadata_scroll.set_child(Some(&metadata_col));
        split.append(&metadata_scroll);
        split.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let dependency_col = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        dependency_col.set_hexpand(true);

        let deps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        deps_col.set_hexpand(true);
        let deps_header = gtk::Label::new(Some("DEPENDENCIES"));
        deps_header.set_xalign(0.0);
        deps_header.add_css_class("section-header");
        deps_col.append(&deps_header);
        let deps_scroll = gtk::ScrolledWindow::new();
        deps_scroll.set_vexpand(true);
        let deps_list = gtk::ListBox::new();
        deps_list.set_selection_mode(gtk::SelectionMode::None);
        let deps_ph = gtk::Label::new(Some("Select a package"));
        deps_ph.add_css_class("dim-label");
        deps_ph.set_margin_top(12);
        deps_list.set_placeholder(Some(&deps_ph));
        deps_scroll.set_child(Some(&deps_list));
        deps_col.append(&deps_scroll);
        dependency_col.append(&deps_col);

        dependency_col.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let rdeps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rdeps_col.set_hexpand(true);
        let rdeps_header = gtk::Label::new(Some("REVERSE DEPENDENCIES"));
        rdeps_header.set_xalign(0.0);
        rdeps_header.add_css_class("section-header");
        rdeps_col.append(&rdeps_header);
        let rdeps_scroll = gtk::ScrolledWindow::new();
        rdeps_scroll.set_vexpand(true);
        let rdeps_list = gtk::ListBox::new();
        rdeps_list.set_selection_mode(gtk::SelectionMode::None);
        let rdeps_ph = gtk::Label::new(Some("No reverse dependencies"));
        rdeps_ph.add_css_class("dim-label");
        rdeps_ph.set_margin_top(12);
        rdeps_list.set_placeholder(Some(&rdeps_ph));
        rdeps_scroll.set_child(Some(&rdeps_list));
        rdeps_col.append(&rdeps_scroll);
        dependency_col.append(&rdeps_col);

        split.append(&dependency_col);
        widget.append(&split);

        let inner = Rc::new(Inner {
            widget,
            store,
            current_pkgname: RefCell::new(None),
            btn_install,
            btn_upgrade,
            btn_remove,
            btn_unmark,
            labels: Labels {
                name,
                version,
                tags,
                state,
                desc,
                installed_size,
                download_size,
                maintainer,
                homepage_box,
                license,
                repository,
                install_date,
                auto_install,
            },
            deps_list,
            rdeps_list,
            on_mark_changed: RefCell::new(Vec::new()),
        });

        wire_buttons(&inner);

        DetailPane { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_mark_changed(&self, f: impl Fn() + 'static) {
        self.inner.on_mark_changed.borrow_mut().push(Box::new(f));
    }

    pub fn show_package(&self, pkg: Option<&Package>) {
        show_package_impl(&self.inner, pkg);
    }
}

fn wire_buttons(inner: &Rc<Inner>) {
    {
        let btn_install = inner.btn_install.clone();
        let inner = inner.clone();
        btn_install.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            let root = inner.widget.root().and_downcast::<gtk::Window>();
            let store = inner.store.clone();
            let inner2 = inner.clone();
            let name2 = name.clone();
            deps_confirm::confirm_install_deps(root.as_ref(), &store, &name, move |proceed| {
                if proceed {
                    inner2.store.set_mark(&name2, PkgMark::Install);
                    update_action_buttons(&inner2, None);
                    for f in inner2.on_mark_changed.borrow().iter() {
                        f();
                    }
                }
            });
        });
    }
    {
        let btn_upgrade = inner.btn_upgrade.clone();
        let inner = inner.clone();
        btn_upgrade.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            inner.store.set_mark(&name, PkgMark::Upgrade);
            update_action_buttons(&inner, None);
            for f in inner.on_mark_changed.borrow().iter() {
                f();
            }
        });
    }
    {
        let btn_remove = inner.btn_remove.clone();
        let inner = inner.clone();
        btn_remove.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            inner.store.set_mark(&name, PkgMark::Remove);
            update_action_buttons(&inner, None);
            for f in inner.on_mark_changed.borrow().iter() {
                f();
            }
        });
    }
    {
        let btn_unmark = inner.btn_unmark.clone();
        let inner = inner.clone();
        btn_unmark.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            inner.store.set_mark(&name, PkgMark::None);
            update_action_buttons(&inner, None);
            for f in inner.on_mark_changed.borrow().iter() {
                f();
            }
        });
    }
}

/// Re-derives button visibility from the store's live copy of the
/// package (since a caller's `Package` may now be stale after a mark
/// change triggered elsewhere). If `pkg` is `Some`, it's used directly.
fn update_action_buttons(inner: &Rc<Inner>, pkg: Option<&Package>) {
    let owned;
    let pkg: Option<&Package> = if pkg.is_some() {
        pkg
    } else if let Some(name) = inner.current_pkgname.borrow().clone() {
        let n = inner.store.list().n_items();
        let mut found = None;
        for i in 0..n {
            if let Some(obj) = inner.store.list().item(i) {
                let obj = obj
                    .downcast::<crate::backend::package::PackageObject>()
                    .unwrap();
                if obj.name() == name {
                    found = Some(obj.pkg().clone());
                    break;
                }
            }
        }
        owned = found;
        owned.as_ref()
    } else {
        None
    };

    let Some(pkg) = pkg else {
        inner.btn_install.set_visible(false);
        inner.btn_remove.set_visible(false);
        inner.btn_upgrade.set_visible(false);
        inner.btn_unmark.set_visible(false);
        return;
    };

    if pkg.mark != PkgMark::None {
        inner.btn_install.set_visible(false);
        inner.btn_remove.set_visible(false);
        inner.btn_upgrade.set_visible(false);
        inner.btn_unmark.set_visible(true);
        return;
    }
    inner.btn_unmark.set_visible(false);

    if pkg.state == PkgState::NotInstalled {
        inner.btn_install.set_visible(true);
        inner.btn_remove.set_visible(false);
        inner.btn_upgrade.set_visible(false);
    } else {
        inner.btn_install.set_visible(false);
        inner
            .btn_upgrade
            .set_visible(pkg.state == PkgState::Upgradable);
        inner.btn_remove.set_visible(true);
        inner.btn_remove.set_sensitive(!pkg.essential);
        inner.btn_remove.set_tooltip_text(if pkg.essential {
            Some("Essential package — removal disabled")
        } else {
            None
        });
    }
}

fn populate(lb: &gtk::ListBox, items: Option<Vec<String>>) {
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
    let Some(items) = items else { return };
    for item in items {
        let l = gtk::Label::new(Some(&item));
        l.set_xalign(0.0);
        l.set_selectable(true);
        l.set_margin_start(8);
        l.set_margin_top(4);
        l.set_margin_bottom(4);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        lb.append(&row);
    }
}

fn clear_box_children(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn set_homepage_value(homepage_box: &gtk::Box, url: Option<&str>) {
    clear_box_children(homepage_box);
    match url.filter(|u| !u.is_empty()) {
        Some(url) => {
            let link = gtk::LinkButton::new(url);
            link.set_halign(gtk::Align::Start);
            homepage_box.append(&link);
        }
        None => {
            let lbl = gtk::Label::new(Some(DASH));
            lbl.set_xalign(0.0);
            homepage_box.append(&lbl);
        }
    }
}

fn show_package_impl(inner: &Rc<Inner>, pkg: Option<&Package>) {
    *inner.current_pkgname.borrow_mut() = pkg.map(|p| p.name.clone());

    let l = &inner.labels;

    let Some(pkg) = pkg else {
        l.name.set_text(DASH);
        l.version.set_text(DASH);
        l.tags.set_text(DASH);
        l.state.set_text(DASH);
        l.desc.set_text("Select a package to view details.");
        l.installed_size.set_text(DASH);
        l.download_size.set_text(DASH);
        l.maintainer.set_text(DASH);
        set_homepage_value(&l.homepage_box, None);
        l.license.set_text(DASH);
        l.repository.set_text(DASH);
        l.install_date.set_text(DASH);
        l.auto_install.set_text(DASH);
        populate(&inner.deps_list, None);
        populate(&inner.rdeps_list, None);
        update_action_buttons(inner, None);
        return;
    };

    // ── Package Information ──
    l.name.set_text(&pkg.name);

    let ver = match (&pkg.version_installed, &pkg.version_available) {
        (Some(inst), Some(avail)) if inst != avail => format!("{}  \u{2192}  {}", inst, avail),
        (Some(inst), _) => inst.clone(),
        (None, Some(avail)) => avail.clone(),
        (None, None) => DASH.to_string(),
    };
    l.version.set_text(&ver);

    l.tags.set_text(if !pkg.tags.is_empty() { &pkg.tags } else { DASH });

    let state_text = match pkg.state {
        PkgState::NotInstalled => "Not installed",
        PkgState::Installed => "Installed",
        PkgState::Upgradable => "Upgrade available",
        PkgState::OnHold => "On hold",
        PkgState::Broken => "Broken",
    };
    l.state.set_text(state_text);

    l.desc.set_text(
        pkg.long_desc
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(Some(pkg.short_desc.as_str()))
            .filter(|s| !s.is_empty())
            .unwrap_or("No description available."),
    );

    // ── Size Statistics ──
    l.installed_size.set_text(&crate::backend::package::pkg_format_size(pkg.install_size));

    let mut dsz = if pkg.download_size > 0 {
        Some(crate::backend::package::pkg_format_size(pkg.download_size))
    } else {
        None
    };

    // ── Maintainer & Source (and a possibly more accurate download size) ──
    let extra = inner.store.get_extra_info(&pkg.name);

    if let Some(extra) = &extra {
        if extra.download_size > 0 {
            dsz = Some(crate::backend::package::pkg_format_size(extra.download_size));
        }
    }
    l.download_size.set_text(dsz.as_deref().unwrap_or(DASH));

    l.maintainer.set_text(if !pkg.maintainer.is_empty() {
        &pkg.maintainer
    } else {
        DASH
    });

    set_homepage_value(
        &l.homepage_box,
        extra.as_ref().and_then(|e| e.homepage.as_deref()),
    );

    l.license.set_text(
        extra
            .as_ref()
            .and_then(|e| e.license.as_deref())
            .unwrap_or(DASH),
    );
    l.repository.set_text(
        extra
            .as_ref()
            .and_then(|e| e.repository.as_deref())
            .unwrap_or(DASH),
    );
    l.install_date.set_text(
        extra
            .as_ref()
            .and_then(|e| e.install_date.as_deref())
            .unwrap_or(DASH),
    );

    if let Some(extra) = &extra {
        if extra.has_automatic_install {
            l.auto_install
                .set_text(if extra.automatic_install { "Yes" } else { "No" });
        } else {
            l.auto_install.set_text(DASH);
        }
    } else {
        l.auto_install.set_text(DASH);
    }

    // ── Dependency column ──
    populate(&inner.deps_list, inner.store.get_deps(&pkg.name));
    populate(&inner.rdeps_list, inner.store.get_rdeps(&pkg.name));

    update_action_buttons(inner, Some(pkg));
}
