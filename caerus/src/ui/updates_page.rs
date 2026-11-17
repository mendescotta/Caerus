//! The Updates page: per-package selection, sizes, a download total and
//! loading / empty / error states.
//!
//! Layout and state model adapted from Shelly's update page
//! (`ui/update_page.ui`, `pages/update_page.zig`, Seafoam-Labs/Shelly-ALPM,
//! GPL-3.0). Its source chips become state chips here (Available, Held back)
//! because Caerus has a single package source per system.
//!
//! The page never starts a transaction itself: it reports an
//! `UpgradeRequest` and `window.rs` routes it through the existing flows
//! (the full system upgrade, or marks plus the pending-changes review).

use crate::backend::package::{pkg_format_size, Package, PackageObject, PkgState};
use crate::ui::widgets::{status_pill, EmptyState, PageHeader, PillKind};
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub type OpenPackage = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

/// What the user asked the Updates page to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpgradeRequest {
    /// Every available update is included: the existing full system upgrade.
    All,
    /// A subset: mark these packages and use the pending-changes review.
    Selected(Vec<String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Chip {
    Available,
    Held,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Freshness {
    NotRefreshed,
    Failed,
    Ok,
}

type IconCache = RefCell<HashMap<String, (Option<gio::Icon>, Option<glib::GString>)>>;
type UpgradeCallback = Rc<RefCell<Option<Box<dyn Fn(UpgradeRequest)>>>>;

struct Inner {
    root: gtk::Box,
    header: PageHeader,
    chip_available: gtk::ToggleButton,
    chip_held: gtk::ToggleButton,
    refresh: gtk::Button,
    state: gtk::Stack,
    empty: EmptyState,
    error: EmptyState,
    card_title: gtk::Label,
    card_sub: gtk::Label,
    action_bar: gtk::Box,
    action_note: gtk::Label,
    upgrade: gtk::Button,
    store: gio::ListStore,
    filter: gtk::CustomFilter,
    chip: Rc<Cell<Chip>>,
    excluded: RefCell<HashSet<String>>,
    open: OpenPackage,
    on_upgrade: UpgradeCallback,
    freshness: Cell<Freshness>,
    last_sync: RefCell<Option<String>>,
    loading: Cell<bool>,
    has_data: Cell<bool>,
    icons: IconCache,
}

#[derive(Clone)]
pub struct UpdatesPage {
    inner: Rc<Inner>,
}

/// Row children in append order; `parts` walks them back out.
struct RowParts {
    check: gtk::CheckButton,
    icon: gtk::Image,
    name: gtk::Label,
    desc: gtk::Label,
    versions: gtk::Label,
    size: gtk::Label,
    held: gtk::Label,
    upgrade: gtk::Button,
}

fn build_row() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("update-row");
    let check = gtk::CheckButton::new();
    check.set_valign(gtk::Align::Center);
    row.append(&check);
    let icon = gtk::Image::from_icon_name("package-x-generic-symbolic");
    icon.set_pixel_size(32);
    icon.add_css_class("update-row-icon");
    row.append(&icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(None);
    name.set_xalign(0.0);
    name.add_css_class("heading");
    let desc = gtk::Label::new(None);
    desc.set_xalign(0.0);
    desc.set_ellipsize(gtk::pango::EllipsizeMode::End);
    desc.add_css_class("dim-label");
    text.append(&name);
    text.append(&desc);
    row.append(&text);
    let versions = gtk::Label::new(None);
    versions.set_valign(gtk::Align::Center);
    versions.add_css_class("monospace");
    versions.add_css_class("update-versions");
    row.append(&versions);
    let size = gtk::Label::new(None);
    size.set_valign(gtk::Align::Center);
    size.set_width_chars(9);
    size.set_xalign(1.0);
    size.add_css_class("dim-label");
    row.append(&size);
    row.append(&status_pill("Held", PillKind::Held));
    let upgrade = gtk::Button::from_icon_name("software-update-available-symbolic");
    upgrade.add_css_class("flat");
    upgrade.set_valign(gtk::Align::Center);
    upgrade.set_tooltip_text(Some("Upgrade only this package"));
    row.append(&upgrade);
    row
}

fn parts(root: &gtk::Box) -> Option<RowParts> {
    let check = root.first_child()?.downcast::<gtk::CheckButton>().ok()?;
    let icon_widget = check.next_sibling()?;
    let icon = icon_widget.clone().downcast::<gtk::Image>().ok()?;
    let text = icon_widget.next_sibling()?;
    let name = text.first_child()?.downcast::<gtk::Label>().ok()?;
    let desc = name.next_sibling()?.downcast::<gtk::Label>().ok()?;
    let versions_widget = text.next_sibling()?;
    let versions = versions_widget.clone().downcast::<gtk::Label>().ok()?;
    let size_widget = versions_widget.next_sibling()?;
    let size = size_widget.clone().downcast::<gtk::Label>().ok()?;
    let held_widget = size_widget.next_sibling()?;
    let held = held_widget.clone().downcast::<gtk::Label>().ok()?;
    let upgrade = held_widget.next_sibling()?.downcast::<gtk::Button>().ok()?;
    Some(RowParts {
        check,
        icon,
        name,
        desc,
        versions,
        size,
        held,
        upgrade,
    })
}

fn package_of(item: &gtk::ListItem) -> Option<Package> {
    item.item()
        .and_downcast::<PackageObject>()
        .map(|object| object.pkg().clone())
}

fn is_available(pkg: &Package) -> bool {
    pkg.state == PkgState::Upgradable && !pkg.is_held_back()
}

/// Text for the action bar button.
pub fn action_label(included: usize, available: usize) -> String {
    if available > 0 && included == available {
        "Upgrade all".to_string()
    } else {
        format!("Upgrade selected ({included})")
    }
}

/// Subtitle of the card: total download of the included rows.
pub fn download_summary(included: usize, bytes: u64) -> String {
    if included == 0 {
        "No updates selected".to_string()
    } else if bytes == 0 {
        "No download required".to_string()
    } else {
        format!("Total download: {}", pkg_format_size(bytes))
    }
}

impl UpdatesPage {
    pub fn new(open: OpenPackage) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.add_css_class("page-body");
        let header = PageHeader::new("Updates", "");
        root.append(header.widget());

        let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        chips.add_css_class("update-chips");
        let chip_available = gtk::ToggleButton::with_label("Available");
        let chip_held = gtk::ToggleButton::with_label("Held back");
        chip_held.set_group(Some(&chip_available));
        chip_available.set_active(true);
        for chip in [&chip_available, &chip_held] {
            chip.add_css_class("update-chip");
            chips.append(chip);
        }
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        chips.append(&spacer);
        let refresh = gtk::Button::with_label("Check again");
        let refresh_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        refresh_content.append(&gtk::Image::from_icon_name("view-refresh-symbolic"));
        refresh_content.append(&gtk::Label::new(Some("Check again")));
        refresh.set_child(Some(&refresh_content));
        refresh.update_property(&[gtk::accessible::Property::Label("Check for updates")]);
        chips.append(&refresh);
        root.append(&chips);

        let store = gio::ListStore::new::<PackageObject>();
        let chip_cell = Rc::new(Cell::new(Chip::Available));
        let filter = {
            let chip = chip_cell.clone();
            gtk::CustomFilter::new(move |object| {
                let Some(object) = object.downcast_ref::<PackageObject>() else {
                    return false;
                };
                let held = object.pkg().is_held_back();
                match chip.get() {
                    Chip::Available => !held,
                    Chip::Held => held,
                }
            })
        };
        let filtered = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let selection = gtk::NoSelection::new(Some(filtered.clone()));
        let list = gtk::ListView::new(Some(selection.clone()), None::<gtk::ListItemFactory>);
        list.set_single_click_activate(true);
        list.add_css_class("updates-list");
        list.update_property(&[gtk::accessible::Property::Label("Available updates")]);

        let state = gtk::Stack::new();
        state.set_vexpand(true);
        state.set_hhomogeneous(false);
        state.set_vhomogeneous(false);

        let loading_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        loading_box.set_valign(gtk::Align::Center);
        loading_box.set_halign(gtk::Align::Center);
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        spinner.set_size_request(32, 32);
        loading_box.append(&spinner);
        let loading_label = gtk::Label::new(Some("Checking for updates…"));
        loading_label.add_css_class("title-3");
        loading_box.append(&loading_label);
        state.add_named(&loading_box, Some("loading"));

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("updates-card");
        card.add_css_class("card");
        let card_title = gtk::Label::new(Some("Available updates"));
        card_title.set_xalign(0.0);
        card_title.add_css_class("heading");
        card_title.add_css_class("updates-card-title");
        let card_sub = gtk::Label::new(None);
        card_sub.set_xalign(0.0);
        card_sub.add_css_class("dim-label");
        card_sub.add_css_class("updates-card-sub");
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&list));
        card.append(&card_title);
        card.append(&card_sub);
        card.append(&scroller);
        state.add_named(&card, Some("list"));

        let empty = EmptyState::new("object-select-symbolic", "Your system is up to date", "");
        state.add_named(empty.widget(), Some("empty"));
        let error = EmptyState::new("dialog-warning-symbolic", "Could not load packages", "");
        let retry = gtk::Button::with_label("Try again");
        retry.add_css_class("pill-button");
        retry.set_halign(gtk::Align::Center);
        error.add_action(&retry);
        state.add_named(error.widget(), Some("error"));
        state.set_visible_child_name("loading");
        root.append(&state);

        let action_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        action_bar.add_css_class("action-bar");
        action_bar.add_css_class("card");
        let action_note = gtk::Label::new(Some("Upgrading asks for administrator authorization."));
        action_note.set_xalign(0.0);
        action_note.set_hexpand(true);
        action_note.set_wrap(true);
        action_note.add_css_class("dim-label");
        action_note.add_css_class("action-bar-note");
        let upgrade = gtk::Button::with_label("Upgrade all");
        upgrade.add_css_class("suggested-action");
        upgrade.add_css_class("pill-button");
        upgrade.set_sensitive(false);
        action_bar.append(&action_note);
        action_bar.append(&upgrade);
        action_bar.set_visible(false);
        root.append(&action_bar);

        let inner = Rc::new(Inner {
            root,
            header,
            chip_available,
            chip_held,
            refresh,
            state,
            empty,
            error,
            card_title,
            card_sub,
            action_bar,
            action_note,
            upgrade,
            store,
            filter,
            chip: chip_cell,
            excluded: RefCell::new(HashSet::new()),
            open,
            on_upgrade: Rc::new(RefCell::new(None)),
            freshness: Cell::new(Freshness::NotRefreshed),
            last_sync: RefCell::new(None),
            loading: Cell::new(false),
            has_data: Cell::new(false),
            icons: RefCell::new(HashMap::new()),
        });
        let page = Self { inner };
        page.wire(&list, &retry);
        page.render();
        page
    }

    fn wire(&self, list: &gtk::ListView, retry: &gtk::Button) {
        let inner = &self.inner;

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(&build_row()));
            }
        });
        {
            let page = Rc::downgrade(inner);
            factory.connect_bind(move |_, item| {
                let (Some(inner), Some(item)) =
                    (page.upgrade(), item.downcast_ref::<gtk::ListItem>())
                else {
                    return;
                };
                Self { inner }.bind_row(item);
            });
        }
        {
            // Wire the per-row widgets once; handlers read the bound item fresh
            // because list items are recycled.
            let page = Rc::downgrade(inner);
            factory.connect_setup(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let Some(row) = item.child().and_downcast::<gtk::Box>() else {
                    return;
                };
                let Some(row_parts) = parts(&row) else {
                    return;
                };
                let weak_item = item.downgrade();
                {
                    let page = page.clone();
                    let weak_item = weak_item.clone();
                    row_parts.check.connect_toggled(move |check| {
                        let (Some(inner), Some(item)) = (page.upgrade(), weak_item.upgrade())
                        else {
                            return;
                        };
                        let Some(pkg) = package_of(&item) else {
                            return;
                        };
                        let included = check.is_active();
                        let changed = {
                            let mut excluded = inner.excluded.borrow_mut();
                            if included {
                                excluded.remove(&pkg.name)
                            } else {
                                excluded.insert(pkg.name)
                            }
                        };
                        if changed {
                            Self { inner }.render();
                        }
                    });
                }
                {
                    let page = page.clone();
                    row_parts.upgrade.connect_clicked(move |_| {
                        let (Some(inner), Some(item)) = (page.upgrade(), weak_item.upgrade())
                        else {
                            return;
                        };
                        if let Some(pkg) = package_of(&item) {
                            Self { inner }.request(UpgradeRequest::Selected(vec![pkg.name]));
                        }
                    });
                }
            });
        }
        list.set_factory(Some(&factory));
        {
            let page = Rc::downgrade(inner);
            list.connect_activate(move |list, position| {
                let Some(inner) = page.upgrade() else {
                    return;
                };
                let name = list
                    .model()
                    .and_then(|model| model.item(position))
                    .and_downcast::<PackageObject>()
                    .map(|object| object.name());
                let open = inner.open.borrow();
                if let (Some(name), Some(callback)) = (name, open.as_ref()) {
                    callback(&name);
                }
            });
        }

        for (button, chip) in [
            (&inner.chip_available, Chip::Available),
            (&inner.chip_held, Chip::Held),
        ] {
            let page = Rc::downgrade(inner);
            button.connect_toggled(move |b| {
                if !b.is_active() {
                    return;
                }
                let Some(inner) = page.upgrade() else {
                    return;
                };
                inner.chip.set(chip);
                inner.filter.changed(gtk::FilterChange::Different);
                Self { inner }.render();
            });
        }
        {
            let page = Rc::downgrade(inner);
            inner.upgrade.connect_clicked(move |_| {
                if let Some(inner) = page.upgrade() {
                    let page = Self { inner };
                    page.request(page.upgrade_request());
                }
            });
        }
        {
            let refresh = inner.refresh.clone();
            retry.connect_clicked(move |_| refresh.emit_clicked());
        }
    }

    fn bind_row(&self, item: &gtk::ListItem) {
        let Some(row) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let (Some(row_parts), Some(pkg)) = (parts(&row), package_of(item)) else {
            return;
        };
        let held = pkg.is_held_back();
        row_parts.name.set_text(&pkg.name);
        row_parts.desc.set_text(&pkg.short_desc);
        row_parts.versions.set_text(&format!(
            "{} → {}",
            pkg.version_installed.as_deref().unwrap_or("—"),
            pkg.version_available.as_deref().unwrap_or("—")
        ));
        row_parts.size.set_text(&if pkg.download_size > 0 {
            pkg_format_size(pkg.download_size)
        } else {
            String::new()
        });
        row_parts.check.set_visible(!held);
        // Compute first: set_active fires the toggled handler, which borrows
        // the exclusion set mutably.
        let included = !self.inner.excluded.borrow().contains(&pkg.name);
        row_parts.check.set_active(included);
        row_parts
            .check
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Include {} in the upgrade",
                pkg.name
            ))]);
        row_parts.held.set_visible(held);
        row_parts.upgrade.set_visible(!held);
        row_parts
            .upgrade
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Upgrade only {}",
                pkg.name
            ))]);
        if held {
            row.add_css_class("update-held");
        } else {
            row.remove_css_class("update-held");
        }
        self.set_icon(&row_parts.icon, &pkg.name);
    }

    fn set_icon(&self, image: &gtk::Image, package: &str) {
        let mut icons = self.inner.icons.borrow_mut();
        let (gicon, name) = icons.entry(package.to_string()).or_insert_with(|| {
            let source = crate::ui::workflows::application_icon(package);
            (source.gicon(), source.icon_name())
        });
        if let Some(gicon) = gicon {
            image.set_from_gicon(gicon);
        } else if let Some(name) = name {
            image.set_icon_name(Some(name));
        }
    }

    fn request(&self, request: UpgradeRequest) {
        if self.inner.loading.get() || !self.inner.has_data.get() {
            return;
        }
        if let Some(callback) = self.inner.on_upgrade.borrow().as_ref() {
            callback(request);
        }
    }

    // ---- data ------------------------------------------------------------

    /// Counts: (available, held back).
    fn counts(&self) -> (usize, usize) {
        let mut available = 0;
        let mut held = 0;
        for i in 0..self.inner.store.n_items() {
            if let Some(object) = self.inner.store.item(i).and_downcast::<PackageObject>() {
                if object.pkg().is_held_back() {
                    held += 1;
                } else {
                    available += 1;
                }
            }
        }
        (available, held)
    }

    /// Names and total download of the included (available, not excluded) rows.
    fn included(&self) -> (Vec<String>, u64) {
        let excluded = self.inner.excluded.borrow();
        let mut names = Vec::new();
        let mut bytes = 0u64;
        for i in 0..self.inner.store.n_items() {
            if let Some(object) = self.inner.store.item(i).and_downcast::<PackageObject>() {
                let pkg = object.pkg();
                if is_available(&pkg) && !excluded.contains(&pkg.name) {
                    names.push(pkg.name.clone());
                    bytes = bytes.saturating_add(pkg.download_size);
                }
            }
        }
        (names, bytes)
    }

    fn upgrade_request(&self) -> UpgradeRequest {
        let (names, _) = self.included();
        let (available, _) = self.counts();
        if available > 0 && names.len() == available {
            UpgradeRequest::All
        } else {
            UpgradeRequest::Selected(names)
        }
    }

    /// The packages offered by the repositories. Held back ones are listed
    /// but never included.
    pub fn set_packages(&self, packages: &[Package]) {
        let mut names = HashSet::new();
        let items: Vec<PackageObject> = packages
            .iter()
            .filter(|p| p.state == PkgState::Upgradable || p.is_held_back())
            .map(|p| {
                if is_available(p) {
                    names.insert(p.name.clone());
                }
                PackageObject::new(p.clone())
            })
            .collect();
        self.inner
            .store
            .splice(0, self.inner.store.n_items(), &items);
        self.inner
            .excluded
            .borrow_mut()
            .retain(|name| names.contains(name));
        self.inner.has_data.set(true);
        self.inner.loading.set(false);
        self.render();
    }

    pub fn set_loading(&self, loading: bool) {
        self.inner.loading.set(loading);
        self.inner.refresh.set_sensitive(!loading);
        self.render();
    }

    pub fn show_error(&self, message: &str) {
        self.inner.loading.set(false);
        self.inner.has_data.set(false);
        self.inner.error.set(
            "dialog-warning-symbolic",
            "Could not load packages",
            &format!("{message}\nUse Check again to retry."),
        );
        self.inner.state.set_visible_child_name("error");
        self.inner.action_bar.set_visible(false);
        self.inner.upgrade.set_sensitive(false);
        self.inner.header.set_subtitle(&self.freshness_text());
    }

    pub fn sync_finished(&self, success: bool) {
        if success {
            self.inner.freshness.set(Freshness::Ok);
            *self.inner.last_sync.borrow_mut() = glib::DateTime::now_local()
                .ok()
                .and_then(|d| d.format("%H:%M").ok())
                .map(|s| s.to_string());
        } else {
            self.inner.freshness.set(Freshness::Failed);
        }
        self.render();
    }

    fn freshness_text(&self) -> String {
        match self.inner.freshness.get() {
            Freshness::NotRefreshed => {
                "Repositories have not been refreshed in this session".into()
            }
            Freshness::Failed => {
                "Repository refresh failed. Displaying cached data; use Check again to retry".into()
            }
            Freshness::Ok => format!(
                "Last refresh {}",
                self.inner
                    .last_sync
                    .borrow()
                    .as_deref()
                    .unwrap_or("just now")
            ),
        }
    }

    /// Recompute everything derived from the model, selection and state.
    fn render(&self) {
        let inner = &self.inner;
        let (available, held) = self.counts();
        inner
            .chip_available
            .set_label(&format!("Available · {available}"));
        inner.chip_held.set_label(&format!("Held back · {held}"));
        let (included, bytes) = self.included();
        let freshness = self.freshness_text();
        inner.header.set_subtitle(&if available > 0 {
            format!(
                "{} of {available} update{} included · {freshness}",
                included.len(),
                if available == 1 { "" } else { "s" }
            )
        } else {
            freshness
        });

        let holding = inner.chip.get() == Chip::Held;
        inner.card_title.set_text(if holding {
            "Held back"
        } else {
            "Available updates"
        });
        inner.card_sub.set_text(&if holding {
            "On hold, so they are excluded from upgrades. Release the hold to update them."
                .to_string()
        } else {
            download_summary(included.len(), bytes)
        });

        inner
            .upgrade
            .set_label(&action_label(included.len(), available));
        let ready = inner.has_data.get() && !inner.loading.get() && !included.is_empty();
        inner.upgrade.set_sensitive(ready);

        let shown = if holding { held } else { available };
        if inner.loading.get() {
            inner.state.set_visible_child_name("loading");
        } else if inner.has_data.get() {
            if shown == 0 {
                let (title, subtitle) = if holding {
                    ("Nothing is held back", "")
                } else {
                    match inner.freshness.get() {
                        Freshness::Failed => (
                            "No updates in cached data",
                            "The latest repository refresh failed.",
                        ),
                        Freshness::NotRefreshed => (
                            "No updates in cached data",
                            "Check again to refresh repositories.",
                        ),
                        Freshness::Ok => ("Your system is up to date", ""),
                    }
                };
                inner.empty.set("object-select-symbolic", title, subtitle);
                inner.state.set_visible_child_name("empty");
            } else {
                inner.state.set_visible_child_name("list");
            }
        }
        let list_visible = inner.state.visible_child_name().as_deref() == Some("list");
        inner.action_bar.set_visible(list_visible && !holding);
        inner.action_note.set_text(
            "Upgrading asks for administrator authorization. Dependencies are reviewed first.",
        );
    }

    // ---- wiring for Workflows -------------------------------------------

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.root
    }

    pub fn refresh_button(&self) -> gtk::Button {
        self.inner.refresh.clone()
    }

    #[cfg(test)]
    pub fn upgrade_button(&self) -> gtk::Button {
        self.inner.upgrade.clone()
    }

    pub fn available_count(&self) -> usize {
        self.counts().0
    }

    pub fn connect_upgrade(&self, callback: impl Fn(UpgradeRequest) + 'static) {
        *self.inner.on_upgrade.borrow_mut() = Some(Box::new(callback));
    }

    #[cfg(test)]
    pub fn probe(&self) -> Probe {
        let (included, _) = self.included();
        Probe {
            subtitle: self.inner.header.subtitle_text(),
            card_sub: self.inner.card_sub.text().to_string(),
            upgrade_label: self
                .inner
                .upgrade
                .label()
                .map(|l| l.to_string())
                .unwrap_or_default(),
            upgrade_sensitive: self.inner.upgrade.is_sensitive(),
            visible_state: self
                .inner
                .state
                .visible_child_name()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            empty_title: self.inner.empty.title_text(),
            empty_subtitle: self.inner.empty.subtitle_text(),
            available_chip: self
                .inner
                .chip_available
                .label()
                .map(|l| l.to_string())
                .unwrap_or_default(),
            held_chip: self
                .inner
                .chip_held
                .label()
                .map(|l| l.to_string())
                .unwrap_or_default(),
            included,
            action_bar_visible: self.inner.action_bar.is_visible(),
        }
    }

    #[cfg(test)]
    pub fn exclude(&self, name: &str) {
        self.inner.excluded.borrow_mut().insert(name.to_string());
        self.render();
    }

    #[cfg(test)]
    pub fn request_for_test(&self) -> UpgradeRequest {
        self.upgrade_request()
    }

    #[cfg(test)]
    pub fn select_chip_for_test(&self, held: bool) {
        if held {
            self.inner.chip_held.set_active(true);
        } else {
            self.inner.chip_available.set_active(true);
        }
    }

    #[cfg(test)]
    pub fn activate_row(&self, name: &str) {
        if let Some(callback) = self.inner.open.borrow().as_ref() {
            callback(name);
        }
    }

    #[cfg(test)]
    pub fn upgrade_one_for_test(&self, name: &str) {
        self.request(UpgradeRequest::Selected(vec![name.to_string()]));
    }
}

#[cfg(test)]
#[derive(Debug)]
pub struct Probe {
    pub subtitle: String,
    pub card_sub: String,
    pub upgrade_label: String,
    pub upgrade_sensitive: bool,
    pub visible_state: String,
    pub empty_title: String,
    pub empty_subtitle: String,
    pub available_chip: String,
    pub held_chip: String,
    pub included: Vec<String>,
    pub action_bar_visible: bool,
}

#[cfg(test)]
fn labels(widget: &gtk::Widget, out: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        if widget.is_mapped() {
            out.push(label.text().to_string());
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        labels(&current, out);
        child = current.next_sibling();
    }
}

#[cfg(test)]
pub fn smoke_test_rows() {
    let pkg = |name: &str, state, size| Package {
        name: name.into(),
        state,
        short_desc: format!("{name} description"),
        version_installed: Some("1.0_1".into()),
        version_available: Some("2.0_1".into()),
        download_size: size,
        ..Package::default()
    };
    let page = UpdatesPage::new(Rc::new(RefCell::new(None)));
    let window = gtk::Window::new();
    window.set_default_size(900, 600);
    window.set_child(Some(page.widget()));
    window.present();
    page.set_packages(&[
        pkg("firefox", PkgState::Upgradable, 78 * 1_048_576),
        pkg("mesa", PkgState::Upgradable, 0),
        pkg("zfs", PkgState::OnHold, 0),
    ]);
    let context = glib::MainContext::default();
    for _ in 0..40 {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut texts = Vec::new();
    labels(page.widget().upcast_ref(), &mut texts);
    assert!(texts.iter().any(|t| t == "firefox"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "mesa"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "1.0_1 → 2.0_1"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "78.0 MiB"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t == "zfs"),
        "held rows stay on their own chip"
    );
    page.select_chip_for_test(true);
    for _ in 0..40 {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut held = Vec::new();
    labels(page.widget().upcast_ref(), &mut held);
    assert!(held.iter().any(|t| t == "zfs"), "{held:?}");
    assert!(held.iter().any(|t| t == "Held"), "{held:?}");
    window.destroy();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_label_switches_between_all_and_selected() {
        assert_eq!(action_label(14, 14), "Upgrade all");
        assert_eq!(action_label(12, 14), "Upgrade selected (12)");
        assert_eq!(action_label(0, 14), "Upgrade selected (0)");
        assert_eq!(action_label(0, 0), "Upgrade selected (0)");
    }

    #[test]
    fn download_summary_covers_empty_zero_and_sized_selections() {
        assert_eq!(download_summary(0, 0), "No updates selected");
        assert_eq!(download_summary(3, 0), "No download required");
        assert_eq!(
            download_summary(2, 2 * 1_048_576),
            "Total download: 2.0 MiB"
        );
    }
}
