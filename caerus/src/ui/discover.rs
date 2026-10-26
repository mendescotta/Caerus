//! Store-style discovery: section tiles, recommendations, search and an
//! application page, all backed by the locally available packages.
use super::catalog::{available_apps, category_style, CatalogApp, CATEGORIES, FEATURED};
use crate::backend::package::{pkg_format_size, Package, PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::ui::{deps_confirm, remove_confirm};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type OpenPackage = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

/// Front-page recommendations are capped so the page stays a quick overview.
const MAX_RECOMMENDED: usize = 12;
/// Search results are plain widgets, not a recycled list view.
const MAX_RESULTS: usize = 120;
/// Content stops growing past this width, as in GNOME Software.
const CONTENT_WIDTH: i32 = 980;

struct AppPage {
    icon: gtk::Box,
    name: gtk::Label,
    package: gtk::Label,
    chip: gtk::Label,
    primary: gtk::Button,
    note: gtk::Label,
    summary: gtk::Label,
    description: gtk::Label,
    tiles: gtk::FlowBox,
}

struct Inner {
    root: gtk::Stack,
    search: gtk::SearchEntry,
    home: gtk::Box,
    categories: gtk::FlowBox,
    recommended: gtk::FlowBox,
    results_box: gtk::Box,
    results_title: gtk::Label,
    results: gtk::ListBox,
    category_title: gtk::Label,
    category_summary: gtk::Label,
    category_grid: gtk::FlowBox,
    app: AppPage,
    empty: gtk::Label,
    available: RefCell<Vec<(&'static CatalogApp, Package)>>,
    current_app: RefCell<Option<String>>,
    app_return: Cell<&'static str>,
    dirty: Cell<bool>,
    narrow: Cell<bool>,
    open: OpenPackage,
    store: RefCell<Option<PackageStore>>,
    on_marks_changed: RefCell<Option<Box<dyn Fn()>>>,
}

#[derive(Clone)]
pub struct Discover {
    inner: Rc<Inner>,
}

fn heading(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("discover-heading");
    label
}

fn grid(columns: u32) -> gtk::FlowBox {
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(columns);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.set_valign(gtk::Align::Start);
    grid
}

/// A width-limited, centered column inside a vertical scroller.
fn clamped_page(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let clamp = gtk::Box::new(gtk::Orientation::Vertical, 0);
    clamp.set_halign(gtk::Align::Center);
    clamp.set_hexpand(true);
    clamp.add_css_class("discover-clamp");
    clamp.append(child);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&clamp));
    scroller
}

fn back_button(label: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
    content.append(&gtk::Label::new(Some(label)));
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("flat");
    button.set_halign(gtk::Align::Start);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

fn clear_flow(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn symbolic_or_fallback(name: &str) -> &str {
    let present = gtk::gdk::Display::default()
        .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(name));
    if present {
        name
    } else {
        "package-x-generic-symbolic"
    }
}

/// The application's own icon when the theme has one; otherwise a tinted
/// tile with its section's symbol, so uninstalled apps still read as distinct.
fn app_icon(app: &CatalogApp, package: &str, size: i32) -> gtk::Widget {
    let image = super::workflows::application_icon(package);
    let themed = image.gicon().is_some()
        || image
            .icon_name()
            .is_some_and(|name| name != "package-x-generic-symbolic");
    if !themed {
        let stem = app.desktop.trim_end_matches(".desktop");
        let display = gtk::gdk::Display::default();
        if display.is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(stem)) {
            image.set_icon_name(Some(stem));
        } else {
            let (symbol, accent) = category_style(app.category);
            let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            tile.add_css_class("app-icon-tile");
            tile.add_css_class(accent);
            tile.set_size_request(size, size);
            // The glyph expands to fill the tile; stop that reaching the card.
            tile.set_hexpand(false);
            tile.set_vexpand(false);
            tile.set_halign(gtk::Align::Center);
            tile.set_valign(gtk::Align::Center);
            let glyph = gtk::Image::from_icon_name(symbolic_or_fallback(symbol));
            glyph.set_pixel_size(size / 2);
            glyph.set_hexpand(true);
            glyph.set_halign(gtk::Align::Center);
            glyph.set_valign(gtk::Align::Center);
            glyph.set_vexpand(true);
            tile.append(&glyph);
            return tile.upcast();
        }
    }
    image.set_pixel_size(size);
    image.set_valign(gtk::Align::Center);
    image.upcast()
}

fn status_text(pkg: &Package) -> Option<(&'static str, &'static str)> {
    match pkg.mark {
        PkgMark::Install => return Some(("Install queued", "chip-accent")),
        PkgMark::Upgrade => return Some(("Update queued", "chip-accent")),
        PkgMark::Remove | PkgMark::Purge => return Some(("Removal queued", "chip-err")),
        PkgMark::None => {}
    }
    match pkg.state {
        PkgState::NotInstalled => None,
        PkgState::Upgradable => Some(("Update available", "chip-warn")),
        PkgState::OnHold => Some(("Installed · held", "chip-ok")),
        PkgState::Broken => Some(("Broken", "chip-err")),
        PkgState::Installed => Some(("Installed", "chip-ok")),
    }
}

fn status_chip(pkg: &Package) -> Option<gtk::Label> {
    let (text, class) = status_text(pkg)?;
    let chip = gtk::Label::new(Some(text));
    chip.add_css_class("chip");
    chip.add_css_class(class);
    chip.set_halign(gtk::Align::Start);
    Some(chip)
}

impl Discover {
    pub fn new(open: OpenPackage) -> Self {
        let root = gtk::Stack::new();
        root.set_transition_type(gtk::StackTransitionType::Crossfade);
        root.set_hhomogeneous(false);
        root.set_vhomogeneous(false);
        root.add_css_class("discover");

        // Explore: search, sections and recommendations.
        let explore = gtk::Box::new(gtk::Orientation::Vertical, 14);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search applications…"));
        search.update_property(&[gtk::accessible::Property::Label("Search applications")]);
        search.add_css_class("discover-search");
        explore.append(&search);

        let home = gtk::Box::new(gtk::Orientation::Vertical, 14);
        home.append(&heading("Categories"));
        let categories = grid(3);
        categories.add_css_class("category-grid");
        home.append(&categories);
        let recommended_title = heading("Recommended");
        recommended_title.set_margin_top(10);
        home.append(&recommended_title);
        let recommended = grid(3);
        home.append(&recommended);
        let empty = gtk::Label::new(Some("Loading the local application catalog…"));
        empty.set_wrap(true);
        empty.add_css_class("dim-label");
        empty.set_margin_top(24);
        home.append(&empty);
        explore.append(&home);

        let results_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let results_title = heading("");
        results_box.append(&results_title);
        let results = gtk::ListBox::new();
        results.set_selection_mode(gtk::SelectionMode::None);
        results.add_css_class("boxed-list");
        results.add_css_class("discover-results");
        let no_results = gtk::Label::new(Some(
            "No applications match this search. Try another name, or search all packages from the header.",
        ));
        no_results.set_wrap(true);
        no_results.set_margin_top(24);
        no_results.set_margin_bottom(24);
        no_results.add_css_class("dim-label");
        results.set_placeholder(Some(&no_results));
        results_box.append(&results);
        results_box.set_visible(false);
        explore.append(&results_box);
        root.add_named(&clamped_page(&explore), Some("explore"));

        // A single section.
        let category = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let category_back = back_button("Explore");
        category.append(&category_back);
        let category_title = gtk::Label::new(None);
        category_title.set_xalign(0.0);
        category_title.add_css_class("title-1");
        category.append(&category_title);
        let category_summary = gtk::Label::new(None);
        category_summary.set_xalign(0.0);
        category_summary.add_css_class("dim-label");
        category.append(&category_summary);
        let category_grid = grid(3);
        category_grid.set_margin_top(6);
        category.append(&category_grid);
        root.add_named(&clamped_page(&category), Some("category"));

        // One application.
        let app_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let app_back = back_button("Back");
        app_box.append(&app_back);
        let hero = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        hero.add_css_class("app-hero");
        let app_icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        app_icon_box.set_valign(gtk::Align::Center);
        hero.append(&app_icon_box);
        let identity = gtk::Box::new(gtk::Orientation::Vertical, 4);
        identity.set_hexpand(true);
        identity.set_valign(gtk::Align::Center);
        let app_name = gtk::Label::new(None);
        app_name.set_xalign(0.0);
        app_name.set_wrap(true);
        app_name.add_css_class("title-1");
        identity.append(&app_name);
        let app_package = gtk::Label::new(None);
        app_package.set_xalign(0.0);
        app_package.set_selectable(true);
        app_package.add_css_class("dim-label");
        identity.append(&app_package);
        let app_chip = gtk::Label::new(None);
        app_chip.add_css_class("chip");
        app_chip.set_halign(gtk::Align::Start);
        app_chip.set_margin_top(4);
        identity.append(&app_chip);
        hero.append(&identity);
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
        actions.set_valign(gtk::Align::Center);
        let primary = gtk::Button::with_label("Install");
        primary.add_css_class("pill-button");
        actions.append(&primary);
        let details = gtk::Button::with_label("Package details");
        details.add_css_class("flat");
        actions.append(&details);
        hero.append(&actions);
        app_box.append(&hero);
        let note = gtk::Label::new(Some(
            "Queued. Review pending changes from the bar below to apply them.",
        ));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("dim-label");
        app_box.append(&note);
        app_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let summary = gtk::Label::new(None);
        summary.set_xalign(0.0);
        summary.set_wrap(true);
        summary.add_css_class("title-3");
        app_box.append(&summary);
        let description = gtk::Label::new(None);
        description.set_xalign(0.0);
        description.set_wrap(true);
        description.set_selectable(true);
        app_box.append(&description);
        let tiles = grid(4);
        tiles.set_margin_top(6);
        app_box.append(&tiles);
        root.add_named(&clamped_page(&app_box), Some("app"));

        let inner = Rc::new(Inner {
            root,
            search,
            home,
            categories,
            recommended,
            results_box,
            results_title,
            results,
            category_title,
            category_summary,
            category_grid,
            app: AppPage {
                icon: app_icon_box,
                name: app_name,
                package: app_package,
                chip: app_chip,
                primary,
                note,
                summary,
                description,
                tiles,
            },
            empty,
            available: RefCell::new(Vec::new()),
            current_app: RefCell::new(None),
            app_return: Cell::new("explore"),
            dirty: Cell::new(false),
            narrow: Cell::new(false),
            open,
            store: RefCell::new(None),
            on_marks_changed: RefCell::new(None),
        });
        set_content_width(&inner);

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
        category_back.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.root.set_visible_child_name("explore");
            }
        });
        let weak = Rc::downgrade(&inner);
        app_back.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.root.set_visible_child_name(inner.app_return.get());
            }
        });
        let weak = Rc::downgrade(&inner);
        details.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                let name = inner.current_app.borrow().clone();
                if let (Some(name), Some(open)) = (name, inner.open.borrow().as_ref()) {
                    open(&name);
                }
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.app.primary.connect_clicked(move |button| {
            if let Some(inner) = weak.upgrade() {
                primary_action(&inner, button);
            }
        });
        Self { inner }
    }

    pub fn widget(&self) -> &gtk::Stack {
        &self.inner.root
    }

    /// Lets the application page queue changes in the shared store.
    pub fn connect_store(&self, store: PackageStore, on_marks_changed: impl Fn() + 'static) {
        *self.inner.store.borrow_mut() = Some(store);
        *self.inner.on_marks_changed.borrow_mut() = Some(Box::new(on_marks_changed));
    }

    /// Marks can change elsewhere (package table, pending bar); keep the
    /// visible application page and badges in step.
    pub fn refresh_marks(&self) {
        let Some(store) = self.inner.store.borrow().clone() else {
            return;
        };
        let states = store.state_and_mark_snapshot();
        for (_, pkg) in self.inner.available.borrow_mut().iter_mut() {
            if let Some((state, mark)) = states.get(&pkg.name) {
                pkg.state = *state;
                pkg.mark = *mark;
            }
        }
        self.inner.dirty.set(true);
        render_if_visible(&self.inner);
        if self.inner.root.visible_child_name().as_deref() == Some("app") {
            let current = self.inner.current_app.borrow().clone();
            if let Some(name) = current {
                fill_app_page(&self.inner, &name);
            }
        }
    }

    pub fn set_narrow(&self, narrow: bool) {
        if self.inner.narrow.replace(narrow) != narrow {
            set_content_width(&self.inner);
        }
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
        if self.inner.root.visible_child_name().as_deref() == Some("app") {
            let current = self.inner.current_app.borrow().clone();
            if let Some(name) = current {
                fill_app_page(&self.inner, &name);
            }
        }
    }

    #[cfg(test)]
    pub fn show_for_test(&self, category: &str, package: &str) -> bool {
        if package.is_empty() {
            show_category(&self.inner, category);
            return true;
        }
        let known = self
            .inner
            .available
            .borrow()
            .iter()
            .any(|(_, pkg)| pkg.name == package);
        if known {
            show_app(&self.inner, package, "explore");
        }
        known
    }

    #[cfg(test)]
    pub fn available_count(&self) -> usize {
        self.inner.available.borrow().len()
    }

    #[cfg(test)]
    pub fn smoke_test_sections_and_search(&self) {
        assert!(
            self.inner.recommended.first_child().is_none(),
            "hidden Discover must not build app widgets at startup"
        );
        render(&self.inner);
        assert!(self.inner.categories.child_at_index(0).is_some());
        assert!(self.inner.recommended.child_at_index(0).is_some());
        assert!(self.inner.recommended.child_at_index(2).is_none());
        show_category(&self.inner, "Web");
        assert_eq!(
            self.inner.root.visible_child_name().as_deref(),
            Some("category")
        );
        assert!(self.inner.category_grid.child_at_index(0).is_some());
        assert!(self.inner.category_grid.child_at_index(1).is_none());
        show_app(&self.inner, "firefox", "category");
        assert_eq!(self.inner.app.name.text(), "Firefox");
        assert_eq!(self.inner.app.primary.label().as_deref(), Some("Update"));
        self.inner.root.set_visible_child_name("explore");
        self.inner.search.set_text("no-such-application");
        render(&self.inner);
        assert!(self.inner.results_box.is_visible());
        assert!(!self.inner.home.is_visible());
        assert!(self.inner.results.row_at_index(0).is_none());
        self.inner.search.set_text("firefox");
        render(&self.inner);
        assert!(self.inner.results.row_at_index(0).is_some());
        assert!(self.inner.results.row_at_index(1).is_none());
        self.inner.search.set_text("");
        render(&self.inner);
        assert!(self.inner.home.is_visible());
        show_app(&self.inner, "firefox", "explore");
        let packages: Vec<_> = self
            .inner
            .available
            .borrow()
            .iter()
            .map(|(_, pkg)| pkg.clone())
            .collect();
        self.set_packages(&[]);
        assert_eq!(
            self.inner.root.visible_child_name().as_deref(),
            Some("explore")
        );
        assert!(self.inner.current_app.borrow().is_none());
        self.set_packages(&packages);
        self.inner.dirty.set(true);
    }
}

fn set_content_width(inner: &Inner) {
    let narrow = inner.narrow.get();
    for page in ["explore", "category", "app"] {
        let clamp = inner
            .root
            .child_by_name(page)
            .and_downcast::<gtk::ScrolledWindow>()
            .and_then(|scroller| scroller.child())
            .and_then(|viewport| viewport.first_child());
        if let Some(clamp) = clamp {
            clamp.set_width_request(if narrow { -1 } else { CONTENT_WIDTH });
            clamp.set_halign(if narrow {
                gtk::Align::Fill
            } else {
                gtk::Align::Center
            });
        }
    }
    let columns = if narrow { 2 } else { 3 };
    inner.categories.set_max_children_per_line(columns);
    inner.categories.set_min_children_per_line(columns);
    for flow in [&inner.recommended, &inner.category_grid] {
        flow.set_max_children_per_line(if narrow { 1 } else { 3 });
        flow.set_min_children_per_line(if narrow { 1 } else { 3 });
    }
    inner
        .app
        .tiles
        .set_max_children_per_line(if narrow { 2 } else { 4 });
    inner
        .app
        .tiles
        .set_min_children_per_line(if narrow { 2 } else { 4 });
}

fn render_if_visible(inner: &Rc<Inner>) {
    if inner.root.is_mapped() && inner.dirty.replace(false) {
        render(inner);
    }
}

fn render(inner: &Rc<Inner>) {
    let query = inner.search.text().trim().to_lowercase();
    let searching = !query.is_empty();
    inner.home.set_visible(!searching);
    inner.results_box.set_visible(searching);
    if searching {
        render_results(inner, &query);
    } else {
        render_home(inner);
    }
    if inner.root.visible_child_name().as_deref() == Some("category") {
        let title = inner.category_title.text();
        fill_category(inner, &title);
    }
}

fn render_home(inner: &Rc<Inner>) {
    clear_flow(&inner.categories);
    clear_flow(&inner.recommended);
    let available = inner.available.borrow();
    inner.empty.set_visible(available.is_empty());
    inner.empty.set_text(
        "No catalog applications are available in your enabled repositories. \
         Browse Packages for the complete list.",
    );
    for category in CATEGORIES {
        let count = available
            .iter()
            .filter(|(app, _)| app.category == *category)
            .count();
        if count == 0 {
            continue;
        }
        inner.categories.insert(&category_tile(inner, category), -1);
    }
    let mut picks: Vec<&(&CatalogApp, Package)> = FEATURED
        .iter()
        .filter_map(|name| available.iter().find(|(app, _)| app.name == *name))
        .collect();
    // Top up from each section so a sparse repository still gets a full front page.
    for category in CATEGORIES {
        if picks.len() >= MAX_RECOMMENDED {
            break;
        }
        if let Some(entry) = available.iter().find(|(app, _)| {
            app.category == *category && !picks.iter().any(|(pick, _)| pick.name == app.name)
        }) {
            picks.push(entry);
        }
    }
    for (app, pkg) in picks.into_iter().take(MAX_RECOMMENDED) {
        inner
            .recommended
            .insert(&app_card(inner, app, pkg, "explore"), -1);
    }
}

fn render_results(inner: &Rc<Inner>, query: &str) {
    clear_list(&inner.results);
    let available = inner.available.borrow();
    let mut matches: Vec<_> = available
        .iter()
        .filter(|(app, pkg)| {
            app.name.to_lowercase().contains(query)
                || pkg.name.to_lowercase().contains(query)
                || pkg.short_desc.to_lowercase().contains(query)
                || app.category.to_lowercase().contains(query)
        })
        .collect();
    // Name hits first, then description-only hits.
    matches.sort_by_key(|(app, pkg)| {
        let name = app.name.to_lowercase();
        if name == query || pkg.name == query {
            0
        } else if name.starts_with(query) {
            1
        } else if name.contains(query) || pkg.name.contains(query) {
            2
        } else {
            3
        }
    });
    inner.results_title.set_text(&results_title(matches.len()));
    for (app, pkg) in matches.into_iter().take(MAX_RESULTS) {
        inner.results.append(&result_row(inner, app, pkg));
    }
}

fn results_title(count: usize) -> String {
    match count {
        0 => "No results".to_string(),
        1 => "1 application".to_string(),
        n if n > MAX_RESULTS => {
            format!("Showing {MAX_RESULTS} of {n} applications — refine your search")
        }
        n => format!("{n} applications"),
    }
}

fn category_tile(inner: &Rc<Inner>, category: &'static str) -> gtk::Button {
    let (symbol, accent) = category_style(category);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_halign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(symbolic_or_fallback(symbol));
    icon.set_pixel_size(24);
    icon.add_css_class("category-icon");
    icon.add_css_class(accent);
    content.append(&icon);
    let label = gtk::Label::new(Some(category));
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&label);
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("category-tile");
    button.update_property(&[gtk::accessible::Property::Label(category)]);
    let weak = Rc::downgrade(inner);
    button.connect_clicked(move |_| {
        if let Some(inner) = weak.upgrade() {
            show_category(&inner, category);
        }
    });
    button
}

fn app_card(
    inner: &Rc<Inner>,
    app: &'static CatalogApp,
    pkg: &Package,
    from: &'static str,
) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    content.set_halign(gtk::Align::Fill);
    content.set_hexpand(true);
    content.append(&app_icon(app, &pkg.name, 56));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let name = gtk::Label::new(Some(app.name));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("heading");
    text.append(&name);
    let package = gtk::Label::new(Some(&pkg.name));
    package.set_xalign(0.0);
    package.set_ellipsize(gtk::pango::EllipsizeMode::End);
    package.add_css_class("caption");
    package.add_css_class("dim-label");
    text.append(&package);
    let description = gtk::Label::new(Some(&pkg.short_desc));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_lines(2);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    description.set_max_width_chars(28);
    description.set_margin_top(2);
    text.append(&description);
    if let Some(chip) = status_chip(pkg) {
        chip.set_margin_top(4);
        text.append(&chip);
    }
    content.append(&text);
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("app-card");
    button.set_tooltip_text(Some(&format!("{} · {}", pkg.name, pkg.short_desc)));
    button.update_property(&[gtk::accessible::Property::Label(app.name)]);
    let weak = Rc::downgrade(inner);
    let package = pkg.name.clone();
    button.connect_clicked(move |_| {
        if let Some(inner) = weak.upgrade() {
            show_app(&inner, &package, from);
        }
    });
    button
}

fn result_row(inner: &Rc<Inner>, app: &'static CatalogApp, pkg: &Package) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    content.add_css_class("result-row");
    content.append(&app_icon(app, &pkg.name, 44));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(app.name));
    name.set_xalign(0.0);
    name.add_css_class("heading");
    text.append(&name);
    let package = gtk::Label::new(Some(&format!("{} · {}", pkg.name, app.category)));
    package.set_xalign(0.0);
    package.add_css_class("caption");
    package.add_css_class("dim-label");
    text.append(&package);
    let description = gtk::Label::new(Some(&pkg.short_desc));
    description.set_xalign(0.0);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&description);
    content.append(&text);
    if let Some(chip) = status_chip(pkg) {
        chip.set_valign(gtk::Align::Center);
        content.append(&chip);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.set_activatable(true);
    row.update_property(&[gtk::accessible::Property::Label(app.name)]);
    let weak = Rc::downgrade(inner);
    let package = pkg.name.clone();
    let gesture = gtk::GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        if let Some(inner) = weak.upgrade() {
            show_app(&inner, &package, "explore");
        }
    });
    row.add_controller(gesture);
    let weak = Rc::downgrade(inner);
    let package = pkg.name.clone();
    row.connect_activate(move |_| {
        if let Some(inner) = weak.upgrade() {
            show_app(&inner, &package, "explore");
        }
    });
    row
}

fn show_category(inner: &Rc<Inner>, category: &str) {
    inner.category_title.set_text(category);
    fill_category(inner, category);
    inner.root.set_visible_child_name("category");
}

fn fill_category(inner: &Rc<Inner>, category: &str) {
    clear_flow(&inner.category_grid);
    let available = inner.available.borrow();
    let mut count = 0;
    for (app, pkg) in available.iter().filter(|(app, _)| app.category == category) {
        inner
            .category_grid
            .insert(&app_card(inner, app, pkg, "category"), -1);
        count += 1;
    }
    inner.category_summary.set_text(&if count == 1 {
        "1 application available from your repositories".to_string()
    } else {
        format!("{count} applications available from your repositories")
    });
}

fn show_app(inner: &Rc<Inner>, package: &str, from: &'static str) {
    inner.app_return.set(from);
    *inner.current_app.borrow_mut() = Some(package.to_string());
    inner.root.set_visible_child_name("app");
    fill_app_page(inner, package);
    if let Some(scroller) = inner
        .root
        .child_by_name("app")
        .and_downcast::<gtk::ScrolledWindow>()
    {
        scroller.vadjustment().set_value(0.0);
    }
}

fn tile(icon: &str, value: &str, caption: &str) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
    tile.add_css_class("info-tile");
    let image = gtk::Image::from_icon_name(symbolic_or_fallback(icon));
    image.set_pixel_size(20);
    image.add_css_class("info-tile-icon");
    tile.append(&image);
    let value_label = gtk::Label::new(Some(value));
    value_label.add_css_class("heading");
    value_label.set_wrap(true);
    value_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value_label.set_justify(gtk::Justification::Center);
    value_label.set_max_width_chars(18);
    value_label.set_selectable(true);
    tile.append(&value_label);
    let caption_label = gtk::Label::new(Some(caption));
    caption_label.add_css_class("caption");
    caption_label.add_css_class("dim-label");
    tile.append(&caption_label);
    tile
}

fn fill_app_page(inner: &Rc<Inner>, package: &str) {
    let available = inner.available.borrow();
    let Some((app, pkg)) = available.iter().find(|(_, pkg)| pkg.name == package) else {
        // A reload can remove the selected package from enabled repositories;
        // go back where the user came from, as the Back button would.
        inner.current_app.borrow_mut().take();
        drop(available);
        let from = inner.app_return.get();
        inner.root.set_visible_child_name(from);
        // render() refills the category page only while it is visible.
        if from == "category" {
            fill_category(inner, &inner.category_title.text());
        }
        return;
    };
    let mut pkg = pkg.clone();
    if let Some((state, mark)) = inner
        .store
        .borrow()
        .as_ref()
        .and_then(|store| store.state_and_mark(package))
    {
        pkg.state = state;
        pkg.mark = mark;
    }
    let page = &inner.app;
    while let Some(child) = page.icon.first_child() {
        page.icon.remove(&child);
    }
    page.icon.append(&app_icon(app, &pkg.name, 96));
    page.name.set_text(app.name);
    page.package.set_text(&pkg.name);
    for class in ["chip-ok", "chip-warn", "chip-err", "chip-accent"] {
        page.chip.remove_css_class(class);
    }
    match status_text(&pkg) {
        Some((text, class)) => {
            page.chip.set_text(text);
            page.chip.add_css_class(class);
            page.chip.set_visible(true);
        }
        None => page.chip.set_visible(false),
    }
    page.summary.set_text(&pkg.short_desc);
    let long = pkg
        .long_desc
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty() && *text != pkg.short_desc);
    page.description.set_visible(long.is_some());
    page.description.set_text(long.unwrap_or_default());

    let primary = &page.primary;
    for class in ["suggested-action", "destructive-action"] {
        primary.remove_css_class(class);
    }
    primary.set_sensitive(inner.store.borrow().is_some());
    primary.set_tooltip_text(None);
    let (label, class) = match (pkg.mark, pkg.state) {
        (PkgMark::None, PkgState::NotInstalled) => ("Install", Some("suggested-action")),
        (PkgMark::None, PkgState::Upgradable) => ("Update", Some("suggested-action")),
        (PkgMark::None, _) => ("Remove", Some("destructive-action")),
        _ => ("Cancel change", None),
    };
    primary.set_label(label);
    if let Some(class) = class {
        primary.add_css_class(class);
    }
    if removal_blocked(&pkg) {
        primary.set_sensitive(false);
        primary.set_tooltip_text(Some("Essential package — removal disabled"));
    }
    page.note.set_visible(pkg.mark != PkgMark::None);

    clear_flow(&page.tiles);
    let version = match (&pkg.version_installed, &pkg.version_available) {
        (Some(installed), Some(available)) if installed != available => {
            format!("{installed} → {available}")
        }
        (Some(version), _) | (None, Some(version)) => version.clone(),
        (None, None) => "—".into(),
    };
    let (category_icon, _) = category_style(app.category);
    let mut tiles = vec![
        tile("software-update-available-symbolic", &version, "Version"),
        tile(category_icon, app.category, "Category"),
    ];
    if pkg.download_size > 0 {
        tiles.push(tile(
            "download-only-symbolic",
            &pkg_format_size(pkg.download_size),
            "Download size",
        ));
    }
    if pkg.install_size > 0 {
        tiles.push(tile(
            "drive-harddisk-symbolic",
            &pkg_format_size(pkg.install_size),
            "Installed size",
        ));
    }
    if let Some(repository) = pkg.repository.as_deref().filter(|r| !r.is_empty()) {
        tiles.push(tile(
            "network-server-symbolic",
            crate::backend::repo_names::display_repo(repository),
            "Repository",
        ));
    }
    if !pkg.maintainer.is_empty() {
        let maintainer = pkg
            .maintainer
            .split('<')
            .next()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&pkg.maintainer);
        tiles.push(tile("system-users-symbolic", maintainer, "Maintainer"));
    }
    for tile in tiles {
        page.tiles.insert(&tile, -1);
    }
}

fn removal_blocked(pkg: &Package) -> bool {
    pkg.essential
        && pkg.mark == PkgMark::None
        && !matches!(pkg.state, PkgState::NotInstalled | PkgState::Upgradable)
}

fn primary_action(inner: &Rc<Inner>, button: &gtk::Button) {
    let Some(name) = inner.current_app.borrow().clone() else {
        return;
    };
    let Some(store) = inner.store.borrow().clone() else {
        return;
    };
    let Some((state, mark)) = store.state_and_mark(&name) else {
        return;
    };
    let finish = {
        let weak = Rc::downgrade(inner);
        let store = store.clone();
        let name = name.clone();
        move |mark: PkgMark| {
            store.set_mark(&name, mark);
            if let Some(inner) = weak.upgrade() {
                if let Some(callback) = inner.on_marks_changed.borrow().as_ref() {
                    callback();
                }
                // A confirmation may finish after navigation to another app.
                let current = inner.current_app.borrow().clone();
                if let Some(current) = current {
                    fill_app_page(&inner, &current);
                }
            }
        }
    };
    let root = button.root().and_downcast::<gtk::Window>();
    match (mark, state) {
        (PkgMark::None, PkgState::NotInstalled) => {
            deps_confirm::confirm_install_deps(root.as_ref(), &store, &name, move |proceed| {
                if proceed {
                    finish(PkgMark::Install);
                }
            });
        }
        (PkgMark::None, PkgState::Upgradable) => finish(PkgMark::Upgrade),
        (PkgMark::None, _) => {
            remove_confirm::confirm_remove_impact(root.as_ref(), &store, &name, move |proceed| {
                if proceed {
                    finish(PkgMark::Remove);
                }
            });
        }
        _ => finish(PkgMark::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn essential_apps_allow_updates_and_cancellation_but_not_removal() {
        let mut pkg = Package {
            essential: true,
            state: PkgState::Upgradable,
            ..Package::default()
        };
        assert!(!removal_blocked(&pkg));
        pkg.state = PkgState::Installed;
        assert!(removal_blocked(&pkg));
        pkg.mark = PkgMark::Upgrade;
        assert!(!removal_blocked(&pkg));
        pkg.mark = PkgMark::None;
        pkg.essential = false;
        assert!(!removal_blocked(&pkg));
    }

    #[test]
    fn truncated_search_results_explain_how_to_find_remaining_apps() {
        assert_eq!(results_title(0), "No results");
        assert_eq!(results_title(1), "1 application");
        assert_eq!(results_title(MAX_RESULTS), "120 applications");
        assert_eq!(
            results_title(MAX_RESULTS + 1),
            "Showing 120 of 121 applications — refine your search"
        );
    }
}
