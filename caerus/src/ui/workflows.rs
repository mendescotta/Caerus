//! Task pages layered over the existing package store and privileged actions.
use crate::backend::package::{Package, PkgState};
use crate::ui::filter_sidebar::SidebarAction;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type PackageCallback = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

pub struct Workflows {
    pub stack: gtk::Stack,
    pub navigation: gtk::Box,
    pub updates_button: gtk::ToggleButton,
    pub discover_button: gtk::ToggleButton,
    pub refresh: gtk::Button,
    pub upgrade: gtk::Button,
    pub history: gtk::Button,
    pub maintenance: Vec<(gtk::Button, SidebarAction)>,
    updates: gtk::ListBox,
    update_status: gtk::Label,
    freshness: gtk::Label,
    discovery: gtk::ListBox,
    update_state: gtk::Stack,
    update_message: gtk::Label,
    open_package: PackageCallback,
    last_sync: RefCell<Option<String>>,
    sync_error: Cell<bool>,
    loading: Cell<bool>,
}

fn title(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("title-2");
    label.update_property(&[gtk::accessible::Property::Label(text)]);
    label
}
fn page() -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(20);
    body.set_margin_bottom(20);
    body.set_margin_start(20);
    body.set_margin_end(20);
    body
}
fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(child));
    scroller
}
fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

impl Workflows {
    pub fn new(packages: &impl IsA<gtk::Widget>) -> Self {
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        stack.add_named(packages, Some("packages"));
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        navigation.set_margin_start(8);
        navigation.set_margin_end(8);
        navigation.set_margin_top(6);
        navigation.set_margin_bottom(6);
        let mut first = None;
        let updates_button = gtk::ToggleButton::with_label("Updates");
        let discover_button = gtk::ToggleButton::with_label("Discover");
        for (name, label) in [
            ("packages", "Packages"),
            ("updates", "Updates"),
            ("discover", "Discover"),
            ("maintenance", "Maintenance"),
        ] {
            let button = match name {
                "updates" => updates_button.clone(),
                "discover" => discover_button.clone(),
                _ => gtk::ToggleButton::with_label(label),
            };
            if let Some(first) = &first {
                button.set_group(Some(first));
            } else {
                first = Some(button.clone());
                button.set_active(true);
            }
            let stack_c = stack.clone();
            button.connect_toggled(move |b| {
                if b.is_active() {
                    stack_c.set_visible_child_name(name);
                }
            });
            // Keep the nav selection synchronized with keyboard/package jumps.
            let weak = button.downgrade();
            stack.connect_visible_child_name_notify(move |s| {
                if let Some(b) = weak.upgrade() {
                    if s.visible_child_name().as_deref() == Some(name) {
                        b.set_active(true);
                    }
                }
            });
            navigation.append(&button);
        }
        stack.connect_visible_child_name_notify(|stack| {
            let weak = stack.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(stack) = weak.upgrade() {
                    if let Some(child) = stack.visible_child() {
                        focus_first(&child);
                    }
                }
            });
        });
        let history = gtk::Button::with_label("History");
        navigation.append(&history);
        let update_page = page();
        update_page.append(&title("Available updates"));
        let update_status = gtk::Label::new(Some("Loading packages…"));
        update_status.set_xalign(0.0);
        update_status.set_wrap(true);
        update_page.append(&update_status);
        let freshness = gtk::Label::new(Some(
            "Repositories have not been refreshed in this session.",
        ));
        freshness.set_xalign(0.0);
        freshness.set_wrap(true);
        freshness.add_css_class("dim-label");
        update_page.append(&freshness);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let refresh = gtk::Button::with_label("Check for updates");
        let upgrade = gtk::Button::with_label("Review system upgrade");
        upgrade.add_css_class("suggested-action");
        upgrade.set_sensitive(false);
        buttons.append(&refresh);
        buttons.append(&upgrade);
        update_page.append(&buttons);
        let update_state = gtk::Stack::new();
        update_state.set_vexpand(true);
        let updates = gtk::ListBox::new();
        updates.set_selection_mode(gtk::SelectionMode::None);
        updates.add_css_class("boxed-list");
        update_state.add_named(&scroll(&updates), Some("list"));
        let update_message = gtk::Label::new(None);
        update_message.set_wrap(true);
        update_message.set_justify(gtk::Justification::Center);
        update_message.set_margin_top(30);
        update_state.add_named(&update_message, Some("message"));
        update_page.append(&update_state);
        stack.add_named(&update_page, Some("updates"));
        let discover_page = page();
        discover_page.append(&title("Discover applications"));
        let explanation = gtk::Label::new(Some("A small bundled selection from your enabled repositories. Availability follows this system’s backend and architecture. Open a package to review and mark changes."));
        explanation.set_xalign(0.0);
        explanation.set_wrap(true);
        explanation.add_css_class("dim-label");
        discover_page.append(&explanation);
        let discovery = gtk::ListBox::new();
        discovery.set_selection_mode(gtk::SelectionMode::None);
        discovery.add_css_class("boxed-list");
        let empty = gtk::Label::new(Some(
            "No catalog applications are available. Refresh repositories or browse all packages.",
        ));
        empty.set_wrap(true);
        empty.set_margin_top(24);
        discovery.set_placeholder(Some(&empty));
        discover_page.append(&scroll(&discovery));
        stack.add_named(&discover_page, Some("discover"));
        let maintenance_page = page();
        maintenance_page.append(&title("Maintenance and tools"));
        let explanation = gtk::Label::new(Some(if cfg!(feature = "apk") {
            "APK manages world constraints. Packages can remain installed when required by another constraint. Void-specific tools are unavailable with this backend."
        } else {
            "Inspect and maintain your system. Package changes run through the privileged helper after review."
        }));
        explanation.set_xalign(0.0);
        explanation.set_wrap(true);
        maintenance_page.append(&explanation);
        let tools = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let mut maintenance = Vec::new();
        for (label, description, action, apk) in [
            (
                "Remove unused dependencies…",
                "Review packages that no longer have dependents.",
                SidebarAction::RemoveOrphans,
                false,
            ),
            (
                "Clean package cache…",
                "Remove obsolete package archives.",
                SidebarAction::CleanCache,
                false,
            ),
            (
                "Verify installed packages…",
                "Check package database consistency.",
                SidebarAction::VerifyDb,
                false,
            ),
            (
                "Reconfigure packages…",
                "Run post-install configuration again.",
                SidebarAction::Reconfigure,
                false,
            ),
            (
                "Remove old kernels…",
                "Review old kernel versions before removing them.",
                SidebarAction::PurgeKernels,
                false,
            ),
            (
                "Find owning package…",
                "Identify which installed package owns a file.",
                SidebarAction::FindOwner,
                true,
            ),
            (
                "Manage alternatives…",
                "Choose the provider for a system command.",
                SidebarAction::Alternatives,
                false,
            ),
            (
                "Manage repositories…",
                "Inspect package sources and their configuration.",
                SidebarAction::ManageRepos,
                true,
            ),
        ] {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let button = gtk::Button::with_label(label);
            button.set_halign(gtk::Align::Start);
            let available = !cfg!(feature = "apk") || apk;
            button.set_sensitive(available);
            let description = if available {
                description
            } else {
                "Unavailable with APK; this is a Void/XBPS operation."
            };
            button.set_tooltip_text(Some(description));
            button.update_property(&[gtk::accessible::Property::Description(description)]);
            let detail = gtk::Label::new(Some(description));
            detail.set_xalign(0.0);
            detail.set_wrap(true);
            detail.add_css_class("dim-label");
            row.append(&button);
            row.append(&detail);
            tools.append(&row);
            maintenance.push((button, action));
        }
        maintenance_page.append(&scroll(&tools));
        stack.add_named(&maintenance_page, Some("maintenance"));
        Self {
            stack,
            navigation,
            updates_button,
            discover_button,
            refresh,
            upgrade,
            history,
            maintenance,
            updates,
            update_status,
            freshness,
            discovery,
            update_state,
            update_message,
            open_package: Rc::new(RefCell::new(None)),
            last_sync: RefCell::new(None),
            sync_error: Cell::new(false),
            loading: Cell::new(false),
        }
    }
    pub fn connect_open_package(&self, callback: impl Fn(&str) + 'static) {
        *self.open_package.borrow_mut() = Some(Box::new(callback));
    }
    pub fn sync_finished(&self, success: bool) {
        self.sync_error.set(!success);
        if success {
            *self.last_sync.borrow_mut() = glib::DateTime::now_local()
                .ok()
                .and_then(|d| d.format("%H:%M:%S").ok())
                .map(|s| s.to_string());
        }
        let text = if success {
            format!(
                "Last successful refresh: {}",
                self.last_sync.borrow().as_deref().unwrap_or("just now")
            )
        } else {
            "Repository refresh failed. Displaying cached data; use Check for updates to retry."
                .into()
        };
        self.freshness.set_text(&text);
    }
    pub fn set_loading(&self, loading: bool) {
        self.loading.set(loading);
        self.refresh.set_sensitive(!loading);
        self.upgrade.set_sensitive(false);
        if loading {
            self.show_message("Loading package information…");
        }
    }
    pub fn load_error(&self, message: &str) {
        self.show_message(&format!(
            "Could not load packages: {message}\nUse Check for updates to retry."
        ));
    }
    fn show_message(&self, message: &str) {
        self.update_message.set_text(message);
        self.update_message
            .update_property(&[gtk::accessible::Property::Description(message)]);
        self.update_state.set_visible_child_name("message");
    }
    pub fn refresh_packages(&self, packages: &[Package]) {
        clear(&self.updates);
        let updates: Vec<_> = packages
            .iter()
            .filter(|p| p.state == PkgState::Upgradable || p.is_held_back())
            .collect();
        let held = updates.iter().filter(|p| p.is_held_back()).count();
        let count = updates.len() - held;
        self.updates_button.set_label(&format!("Updates ({count})"));
        let summary = format!("{count} available updates · {held} held back. Review the complete solver plan before applying.");
        self.update_status.set_text(&summary);
        self.update_status
            .update_property(&[gtk::accessible::Property::Description(&summary)]);
        self.upgrade.set_sensitive(count > 0 && !self.loading.get());
        if updates.is_empty() {
            self.show_message(if self.sync_error.get() {
                "No updates in cached data. The latest repository refresh failed."
            } else if self.last_sync.borrow().is_none() {
                "No updates in cached data. Check for updates to refresh repositories."
            } else {
                "Your system is up to date."
            });
        } else {
            self.update_state.set_visible_child_name("list");
        }
        for p in updates {
            let row = gtk::Button::new();
            row.add_css_class("flat");
            let text = format!(
                "{}   {} → {}{}",
                p.name,
                p.version_installed.as_deref().unwrap_or("—"),
                p.version_available.as_deref().unwrap_or("—"),
                if p.is_held_back() {
                    " · On hold — excluded from upgrades"
                } else {
                    ""
                }
            );
            let label = gtk::Label::new(Some(&text));
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_margin_top(8);
            label.set_margin_bottom(8);
            row.set_child(Some(&label));
            let callback = self.open_package.clone();
            let name = p.name.clone();
            row.connect_clicked(move |_| {
                if let Some(cb) = callback.borrow().as_ref() {
                    cb(&name);
                }
            });
            self.updates.append(&row);
        }
        clear(&self.discovery);
        let mut previous_category = "";
        for app in CATALOG {
            let Some(pkg) = packages
                .iter()
                .find(|p| app.packages.contains(&p.name.as_str()) && p.version_available.is_some())
            else {
                continue;
            };
            if previous_category != app.category {
                let heading = title(app.category);
                heading.set_margin_top(12);
                self.discovery.append(&heading);
                previous_category = app.category;
            }
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.set_margin_top(8);
            row.set_margin_bottom(8);
            row.set_margin_start(8);
            row.set_margin_end(8);
            let icon = application_icon(&pkg.name);
            icon.set_pixel_size(40);
            row.append(&icon);
            let info = gtk::Box::new(gtk::Orientation::Vertical, 3);
            info.set_hexpand(true);
            let name = gtk::Label::new(Some(app.name));
            name.set_xalign(0.0);
            name.add_css_class("heading");
            info.append(&name);
            let description = gtk::Label::new(Some(&format!(
                "{} · {}{}",
                pkg.short_desc,
                pkg.name,
                if pkg.state == PkgState::NotInstalled {
                    ""
                } else {
                    " · Installed"
                }
            )));
            description.set_wrap(true);
            description.set_xalign(0.0);
            description.add_css_class("dim-label");
            info.append(&description);
            row.append(&info);
            let open = gtk::Button::with_label("View package");
            open.set_valign(gtk::Align::Center);
            open.update_property(&[gtk::accessible::Property::Label(&format!(
                "View {} package",
                app.name
            ))]);
            let callback = self.open_package.clone();
            let name = pkg.name.clone();
            open.connect_clicked(move |_| {
                if let Some(cb) = callback.borrow().as_ref() {
                    cb(&name);
                }
            });
            row.append(&open);
            self.discovery.append(&row);
        }
    }
}

fn focus_first(widget: &gtk::Widget) -> bool {
    if !widget.is_visible() || !widget.is_sensitive() {
        return false;
    }
    if widget.is_visible()
        && widget.is_sensitive()
        && !widget.is::<gtk::Label>()
        && widget.is_focusable()
        && widget.grab_focus()
    {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if focus_first(&current) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

struct CatalogApp {
    category: &'static str,
    name: &'static str,
    packages: &'static [&'static str],
    desktop: &'static str,
}
const CATALOG: &[CatalogApp] = &[
    CatalogApp {
        category: "Internet",
        name: "Firefox",
        packages: &["firefox"],
        desktop: "firefox.desktop",
    },
    CatalogApp {
        category: "Internet",
        name: "Chromium",
        packages: &["chromium"],
        desktop: "chromium.desktop",
    },
    CatalogApp {
        category: "Internet",
        name: "Thunderbird",
        packages: &["thunderbird"],
        desktop: "thunderbird.desktop",
    },
    CatalogApp {
        category: "Office",
        name: "LibreOffice",
        packages: &["libreoffice", "libreoffice-writer"],
        desktop: "libreoffice-writer.desktop",
    },
    CatalogApp {
        category: "Graphics",
        name: "GIMP",
        packages: &["gimp"],
        desktop: "gimp.desktop",
    },
    CatalogApp {
        category: "Graphics",
        name: "Inkscape",
        packages: &["inkscape"],
        desktop: "org.inkscape.Inkscape.desktop",
    },
    CatalogApp {
        category: "Media",
        name: "VLC",
        packages: &["vlc"],
        desktop: "vlc.desktop",
    },
    CatalogApp {
        category: "Media",
        name: "Audacity",
        packages: &["audacity"],
        desktop: "audacity.desktop",
    },
    CatalogApp {
        category: "Development",
        name: "Geany",
        packages: &["geany"],
        desktop: "geany.desktop",
    },
    CatalogApp {
        category: "Development",
        name: "GNOME Builder",
        packages: &["gnome-builder"],
        desktop: "org.gnome.Builder.desktop",
    },
];

pub fn application_icon(package: &str) -> gtk::Image {
    if let Some(icon) = CATALOG
        .iter()
        .find(|app| app.packages.contains(&package))
        .and_then(|app| gio::DesktopAppInfo::new(app.desktop))
        .and_then(|app| app.icon())
    {
        return gtk::Image::from_gicon(&icon);
    }
    let image = gtk::Image::from_icon_name("package-x-generic-symbolic");
    if let Some(display) = gtk::gdk::Display::default() {
        if gtk::IconTheme::for_display(&display).has_icon(package) {
            image.set_icon_name(Some(package));
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::package::{PackageObject, PkgMark};
    use crate::backend::package_store::PackageStore;
    use crate::ui::package_list::PackageList;

    #[test]
    #[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
    fn task_pages_and_package_search_follow_available_backend_data() {
        gtk::init().expect("GUI smoke test requires a display");
        #[cfg(feature = "adwaita")]
        adw::init().expect("GUI smoke test requires libadwaita");
        let package = |name: &str, state| Package {
            name: name.into(),
            state,
            short_desc: "An application".into(),
            version_installed: Some("1.0_1".into()),
            version_available: Some("2.0_1".into()),
            ..Package::default()
        };
        let packages = vec![
            package("firefox", PkgState::Upgradable),
            package("geany", PkgState::OnHold),
            Package {
                version_available: None,
                ..package("gimp", PkgState::Installed)
            },
        ];
        let pages = Workflows::new(&gtk::Box::new(gtk::Orientation::Vertical, 0));
        pages.set_loading(false);
        pages.refresh_packages(&packages);
        assert_eq!(pages.updates_button.label().as_deref(), Some("Updates (1)"));
        assert!(pages.update_status.text().contains("1 held back"));
        assert!(pages.upgrade.is_sensitive());
        assert!(pages.discovery.row_at_index(3).is_some());
        assert!(
            pages.discovery.row_at_index(4).is_none(),
            "unavailable apps must not enter discovery"
        );
        pages.updates_button.set_active(true);
        assert_eq!(pages.stack.visible_child_name().as_deref(), Some("updates"));
        let opened = Rc::new(RefCell::new(String::new()));
        {
            let opened = opened.clone();
            pages.connect_open_package(move |name| *opened.borrow_mut() = name.into());
        }
        let row = pages.updates.row_at_index(0).unwrap();
        row.child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(opened.borrow().as_str(), "firefox");
        pages.sync_finished(false);
        assert!(pages.freshness.text().contains("failed"));
        pages.refresh_packages(&[]);
        assert!(!pages.upgrade.is_sensitive());
        assert_eq!(
            pages.update_state.visible_child_name().as_deref(),
            Some("message")
        );
        assert!(pages.update_message.text().contains("cached"));

        let store = PackageStore::new();
        let list = PackageList::new(store.clone());
        store.list().splice(
            0,
            0,
            &packages
                .into_iter()
                .map(PackageObject::new)
                .collect::<Vec<_>>(),
        );
        let notifications = Rc::new(Cell::new(0));
        {
            let notifications = notifications.clone();
            list.connect_package_selected(move |_| notifications.set(notifications.get() + 1));
        }
        list.set_search("FIREFOX");
        assert_eq!(list.visible_counts().0, 1);
        assert!(list.select_package_by_name("firefox"));
        let before_duplicate = notifications.get();
        list.set_search(" firefox ");
        assert_eq!(
            notifications.get(),
            before_duplicate,
            "delayed duplicate searches must preserve package jumps"
        );
        store.set_mark("firefox", PkgMark::Upgrade);
        assert_eq!(store.count_marked(), 1);
        list.set_search("definitely-no-matching-package");
        assert_eq!(list.visible_counts().0, 0);
        list.set_search("");
        assert_eq!(list.visible_counts().0, 3);
        crate::ui::apply_confirm::smoke_test_incomplete_previews();
        crate::ui::apply_dialog::smoke_test_transaction_history();
    }
}
