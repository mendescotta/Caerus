//! Lazy, searchable discovery sections backed by the locally available packages.
use super::catalog::{available_apps, CatalogApp, CATEGORIES};
use crate::backend::package::{Package, PkgState};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type OpenPackage = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

struct Inner {
    root: gtk::Box,
    search: gtk::SearchEntry,
    sections: gtk::DropDown,
    summary: gtk::Label,
    list: gtk::ListBox,
    empty: gtk::Label,
    available: RefCell<Vec<(&'static CatalogApp, Package)>>,
    dirty: Cell<bool>,
    open: OpenPackage,
}

#[derive(Clone)]
pub struct Discover {
    inner: Rc<Inner>,
}

impl Discover {
    pub fn new(open: OpenPackage) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.set_margin_start(20);
        root.set_margin_end(20);
        root.set_margin_top(12);
        root.set_margin_bottom(12);
        let heading = gtk::Label::new(Some("Discover applications"));
        heading.set_xalign(0.0);
        heading.add_css_class("title-2");
        root.append(&heading);
        let description = gtk::Label::new(Some(
            "Browse a curated selection from your enabled repositories.",
        ));
        description.set_xalign(0.0);
        description.set_wrap(true);
        description.add_css_class("dim-label");
        root.append(&description);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let search = gtk::SearchEntry::new();
        search.set_hexpand(true);
        search.set_placeholder_text(Some("Search applications…"));
        search.update_property(&[gtk::accessible::Property::Label("Search applications")]);
        let names: Vec<_> = std::iter::once("All sections")
            .chain(CATEGORIES.iter().copied())
            .collect();
        let sections = gtk::DropDown::from_strings(&names);
        sections.update_property(&[gtk::accessible::Property::Label("Application section")]);
        sections.set_tooltip_text(Some("Browse application sections"));
        controls.append(&search);
        controls.append(&sections);
        root.append(&controls);
        let summary = gtk::Label::new(None);
        summary.set_xalign(0.0);
        summary.add_css_class("dim-label");
        root.append(&summary);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        let empty = gtk::Label::new(Some("Loading the local application catalog…"));
        empty.set_wrap(true);
        empty.set_margin_top(24);
        empty.set_margin_bottom(24);
        list.set_placeholder(Some(&empty));
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list));
        root.append(&scroll);
        let inner = Rc::new(Inner {
            root,
            search,
            sections,
            summary,
            list,
            empty,
            available: RefCell::new(Vec::new()),
            dirty: Cell::new(false),
            open,
        });
        let weak = Rc::downgrade(&inner);
        inner.root.connect_map(move |_| {
            if let Some(inner) = weak.upgrade() {
                render_if_visible(&inner);
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.search.connect_search_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.dirty.set(true);
                render_if_visible(&inner);
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.sections.connect_selected_notify(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.dirty.set(true);
                render_if_visible(&inner);
            }
        });
        Self { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.root
    }

    pub fn set_packages(&self, packages: &[Package]) {
        *self.inner.available.borrow_mut() = available_apps(packages)
            .into_iter()
            .map(|(app, pkg)| (app, pkg.clone()))
            .collect();
        crate::startup::trace(format_args!(
            "Discover inventory indexed: {} available applications",
            self.inner.available.borrow().len()
        ));
        self.inner.dirty.set(true);
        render_if_visible(&self.inner);
    }

    #[cfg(test)]
    pub fn available_count(&self) -> usize {
        self.inner.available.borrow().len()
    }

    #[cfg(test)]
    pub fn smoke_test_sections_and_search(&self) {
        assert!(
            self.inner.list.row_at_index(0).is_none(),
            "hidden Discover must not build app widgets at startup"
        );
        self.inner.sections.set_selected(1); // Web
        render(&self.inner);
        assert!(self.inner.list.row_at_index(0).is_some());
        assert!(self.inner.list.row_at_index(2).is_none()); // heading + application grid
        self.inner.search.set_text("no-such-application");
        render(&self.inner);
        assert!(self.inner.list.row_at_index(0).is_none());
        self.inner.search.set_text("firefox");
        render(&self.inner);
        assert!(self.inner.list.row_at_index(1).is_some());
        self.inner.sections.set_selected(0);
        self.inner.search.set_text("");
        self.inner.dirty.set(true);
    }
}

fn render_if_visible(inner: &Rc<Inner>) {
    if inner.root.is_mapped() && inner.dirty.replace(false) {
        render(inner);
    }
}

fn render(inner: &Rc<Inner>) {
    while let Some(child) = inner.list.first_child() {
        inner.list.remove(&child);
    }
    let query = inner.search.text().trim().to_lowercase();
    let section = inner
        .sections
        .selected()
        .checked_sub(1)
        .and_then(|index| CATEGORIES.get(index as usize))
        .copied();
    let available = inner.available.borrow();
    let mut previous_section = "";
    let mut count = 0;
    let mut grid = gtk::FlowBox::new();
    for (app, pkg) in available.iter().filter(|(app, pkg)| {
        section.is_none_or(|name| name == app.category)
            && (query.is_empty()
                || app.name.to_lowercase().contains(&query)
                || pkg.name.to_lowercase().contains(&query)
                || pkg.short_desc.to_lowercase().contains(&query))
    }) {
        if previous_section != app.category {
            let heading = gtk::Label::new(Some(app.category));
            heading.set_xalign(0.0);
            heading.add_css_class("heading");
            heading.set_margin_start(10);
            heading.set_margin_top(10);
            heading.set_margin_bottom(6);
            inner.list.append(&heading);
            grid = gtk::FlowBox::new();
            grid.set_selection_mode(gtk::SelectionMode::None);
            grid.set_homogeneous(true);
            grid.set_min_children_per_line(1);
            grid.set_max_children_per_line(4);
            grid.set_column_spacing(8);
            grid.set_row_spacing(8);
            grid.set_margin_start(6);
            grid.set_margin_end(6);
            grid.set_margin_bottom(8);
            inner.list.append(&grid);
            previous_section = app.category;
        }
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.set_width_request(270);
        row.add_css_class("discover-card");
        row.set_margin_start(10);
        row.set_margin_end(10);
        row.set_margin_top(5);
        row.set_margin_bottom(5);
        let icon = super::workflows::application_icon(&pkg.name);
        icon.set_pixel_size(28);
        row.append(&icon);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let name = gtk::Label::new(Some(app.name));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_max_width_chars(24);
        name.add_css_class("heading");
        text.append(&name);
        let description = gtk::Label::new(Some(&pkg.short_desc));
        description.set_xalign(0.0);
        description.set_max_width_chars(24);
        description.set_ellipsize(gtk::pango::EllipsizeMode::End);
        description.add_css_class("dim-label");
        text.append(&description);
        row.set_tooltip_text(Some(&format!("{} · {}", pkg.name, pkg.short_desc)));
        row.append(&text);
        if pkg.state != PkgState::NotInstalled {
            let installed = gtk::Label::new(Some("Installed"));
            installed.add_css_class("dim-label");
            installed.set_xalign(0.0);
            text.append(&installed);
        }
        let open = gtk::Button::with_label("View");
        open.set_valign(gtk::Align::Center);
        open.update_property(&[gtk::accessible::Property::Label(&format!(
            "View {} package",
            app.name
        ))]);
        let callback = inner.open.clone();
        let name = pkg.name.clone();
        open.connect_clicked(move |_| {
            if let Some(callback) = callback.borrow().as_ref() {
                callback(&name);
            }
        });
        row.append(&open);
        grid.insert(&row, -1);
        count += 1;
    }
    let summary = if section.is_none() && query.is_empty() {
        let sections = CATEGORIES
            .iter()
            .filter(|section| available.iter().any(|(app, _)| &app.category == *section))
            .count();
        format!("{count} applications in {sections} sections")
    } else {
        format!(
            "{count} applications · {} available in the curated catalog",
            available.len()
        )
    };
    inner.summary.set_text(&summary);
    inner.empty.set_text(if available.is_empty() {
        "No catalog applications are available in your enabled repositories. Browse Packages for the complete list."
    } else { "No applications match this section and search. Try another section or clear the search." });
}
