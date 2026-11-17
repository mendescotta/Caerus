//! Task pages layered over the existing package store and privileged actions.
use super::catalog::CATALOG;
use super::discover::Discover;
use super::updates_page::{UpdatesPage, UpgradeRequest};
use crate::backend::package::Package;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type PackageCallback = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

pub struct Workflows {
    pub stack: gtk::Stack,
    pub navigation: gtk::Box,
    pub updates_button: gtk::ToggleButton,
    pub discover_button: gtk::ToggleButton,
    pub refresh: gtk::Button,
    pub history: gtk::Button,
    updates: UpdatesPage,
    pub discovery: Discover,
    open_package: PackageCallback,
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
        navigation.add_css_class("workflow-navigation");
        navigation.set_halign(gtk::Align::Center);
        navigation.set_margin_top(4);
        navigation.set_margin_bottom(4);
        let mut first = None;
        let updates_button = gtk::ToggleButton::with_label("Updates");
        let discover_button = gtk::ToggleButton::with_label("Discover");
        for (name, label) in [
            ("packages", "Packages"),
            ("updates", "Updates"),
            ("discover", "Discover"),
        ] {
            let button = match name {
                "updates" => updates_button.clone(),
                "discover" => discover_button.clone(),
                _ => gtk::ToggleButton::with_label(label),
            };
            button.add_css_class("flat");
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
        let history = gtk::Button::from_icon_name("document-open-recent-symbolic");
        history.set_tooltip_text(Some("Transaction history"));
        history.update_property(&[gtk::accessible::Property::Label("Transaction history")]);
        let open_package: PackageCallback = Rc::new(RefCell::new(None));
        let updates = UpdatesPage::new(open_package.clone());
        let refresh = updates.refresh_button();
        stack.add_named(updates.widget(), Some("updates"));
        let discovery = Discover::new(open_package.clone());
        stack.add_named(discovery.widget(), Some("discover"));
        Self {
            stack,
            navigation,
            updates_button,
            discover_button,
            refresh,
            history,
            updates,
            discovery,
            open_package,
        }
    }
    pub fn connect_open_package(&self, callback: impl Fn(&str) + 'static) {
        *self.open_package.borrow_mut() = Some(Box::new(callback));
    }
    pub fn connect_upgrade(&self, callback: impl Fn(UpgradeRequest) + 'static) {
        self.updates.connect_upgrade(callback);
    }
    pub fn sync_finished(&self, success: bool) {
        self.updates.sync_finished(success);
    }
    pub fn set_loading(&self, loading: bool) {
        self.updates.set_loading(loading);
    }
    pub fn load_error(&self, message: &str) {
        self.updates.show_error(message);
    }
    pub fn refresh_packages(&self, packages: &[Package]) {
        self.updates.set_packages(packages);
        self.updates_button
            .set_label(&format!("Updates ({})", self.updates.available_count()));
        self.discovery.set_packages(packages);
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

pub fn application_icon(package: &str) -> gtk::Image {
    if let Some(icon) = CATALOG
        .iter()
        .find(|app| app.packages.contains(&package))
        .filter(|app| !app.desktop.is_empty())
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
    use crate::backend::package::{PackageObject, PkgMark, PkgState};
    use crate::backend::package_store::PackageStore;
    use crate::ui::package_list::PackageList;
    use std::cell::Cell;

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
        let probe = pages.updates.probe();
        assert!(
            probe.subtitle.starts_with("1 of 1 update included"),
            "{probe:?}"
        );
        assert_eq!(probe.available_chip, "Available · 1");
        assert_eq!(probe.held_chip, "Held back · 1");
        assert_eq!(probe.visible_state, "list");
        assert_eq!(probe.upgrade_label, "Upgrade all");
        assert!(probe.upgrade_sensitive);
        assert!(probe.action_bar_visible);
        assert_eq!(pages.updates.request_for_test(), UpgradeRequest::All);
        assert_eq!(pages.discovery.available_count(), 2);
        pages.discovery.smoke_test_sections_and_search();
        let mut with_discovered = packages.clone();
        with_discovered.push(Package {
            short_desc: "A graphical application for drawing diagrams".into(),
            ..package("unlisted-diagram", PkgState::NotInstalled)
        });
        pages.refresh_packages(&with_discovered);
        assert_eq!(pages.discovery.available_count(), 3);
        assert!(pages.discovery.show_for_test("", "unlisted-diagram"));
        pages.refresh_packages(&packages);
        assert!(!pages.discovery.show_for_test("", "unlisted-diagram"));
        pages.updates_button.set_active(true);
        assert_eq!(pages.stack.visible_child_name().as_deref(), Some("updates"));

        // Selection: excluding a row turns "Upgrade all" into a subset.
        let mut many = packages.clone();
        many.push(Package {
            download_size: 3 * 1_048_576,
            ..package("mesa", PkgState::Upgradable)
        });
        many.push(Package {
            download_size: 1_048_576,
            ..package("zlib", PkgState::Upgradable)
        });
        pages.refresh_packages(&many);
        let probe = pages.updates.probe();
        assert!(
            probe.subtitle.starts_with("3 of 3 updates included"),
            "{probe:?}"
        );
        assert_eq!(probe.card_sub, "Total download: 4.0 MiB");
        pages.updates.exclude("mesa");
        let probe = pages.updates.probe();
        assert!(
            probe.subtitle.starts_with("2 of 3 updates included"),
            "{probe:?}"
        );
        assert_eq!(probe.upgrade_label, "Upgrade selected (2)");
        assert_eq!(probe.card_sub, "Total download: 1.0 MiB");
        assert_eq!(probe.included, vec!["firefox", "zlib"]);
        assert_eq!(
            pages.updates.request_for_test(),
            UpgradeRequest::Selected(vec!["firefox".into(), "zlib".into()])
        );
        let requests = Rc::new(RefCell::new(Vec::new()));
        {
            let requests = requests.clone();
            pages.connect_upgrade(move |request| requests.borrow_mut().push(request));
        }
        pages.updates.upgrade_button().emit_clicked();
        pages.updates.upgrade_one_for_test("zlib");
        assert_eq!(
            *requests.borrow(),
            vec![
                UpgradeRequest::Selected(vec!["firefox".into(), "zlib".into()]),
                UpgradeRequest::Selected(vec!["zlib".into()]),
            ]
        );
        // Exclusions for packages that are no longer offered are dropped.
        let mut installed = many.clone();
        for pkg in &mut installed {
            if pkg.name == "mesa" {
                pkg.state = PkgState::Installed;
            }
        }
        pages.refresh_packages(&installed);
        pages.refresh_packages(&many);
        assert_eq!(
            pages.updates.request_for_test(),
            UpgradeRequest::All,
            "a completed update must clear its previous exclusion"
        );
        pages.refresh_packages(&packages);
        assert_eq!(pages.updates.request_for_test(), UpgradeRequest::All);
        pages.updates.select_chip_for_test(true);
        let probe = pages.updates.probe();
        assert!(!probe.action_bar_visible);
        assert_eq!(
            probe.card_sub,
            "On hold, so they are excluded from upgrades. Release the hold to update them."
        );

        let opened = Rc::new(RefCell::new(String::new()));
        {
            let opened = opened.clone();
            pages.connect_open_package(move |name| *opened.borrow_mut() = name.into());
        }
        pages.updates.activate_row("firefox");
        assert_eq!(opened.borrow().as_str(), "firefox");

        // States: failed refresh, empty list, loading, load error.
        pages.updates.select_chip_for_test(false);
        pages.sync_finished(false);
        assert!(pages.updates.probe().subtitle.contains("failed"));
        pages.refresh_packages(&[]);
        let probe = pages.updates.probe();
        assert!(!probe.upgrade_sensitive);
        assert_eq!(probe.visible_state, "empty");
        assert_eq!(probe.empty_title, "No updates in cached data");
        assert!(probe.empty_subtitle.contains("failed"));
        pages.sync_finished(true);
        let probe = pages.updates.probe();
        assert_eq!(probe.empty_title, "Your system is up to date");
        assert!(probe.subtitle.starts_with("Last refresh "), "{probe:?}");
        pages.set_loading(true);
        assert_eq!(pages.updates.probe().visible_state, "loading");
        assert!(!pages.refresh.is_sensitive());
        pages.set_loading(false);
        pages.load_error("database locked");
        let probe = pages.updates.probe();
        assert_eq!(probe.visible_state, "error");
        assert!(!probe.action_bar_visible);
        pages.updates.select_chip_for_test(true);
        pages.updates.select_chip_for_test(false);
        assert!(!pages.updates.probe().upgrade_sensitive);
        let count = requests.borrow().len();
        pages.updates.upgrade_button().emit_clicked();
        assert_eq!(
            requests.borrow().len(),
            count,
            "failed data must not start an upgrade"
        );

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
        let wanted = ["firefox".to_string(), "absent".to_string()]
            .into_iter()
            .collect();
        let subset = store.snapshot_objects_for(&wanted);
        assert_eq!(subset.len(), 1);
        assert!(subset.contains_key("firefox"));
        let notifications = Rc::new(Cell::new(0));
        {
            let notifications = notifications.clone();
            list.connect_package_selected(move |_| notifications.set(notifications.get() + 1));
        }
        list.set_search("FIREFOX");
        assert_eq!(list.visible_counts().0, 1);
        assert!(list.select_package_by_name("firefox"));
        list.assert_activation_toggles_marks();
        list.assert_cached_rows_survive_refresh();
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
        crate::ui::theme::smoke_test_theme_switching();
        #[cfg(not(feature = "adwaita"))]
        crate::ui::toast::smoke_test_toasts();
        crate::ui::updates_page::smoke_test_rows();
        crate::ui::detail_pane::smoke_test_metadata_replies();
        crate::ui::window::smoke_test_compact_layout();
        crate::ui::apply_confirm::smoke_test_incomplete_previews();
        crate::ui::apply_dialog::smoke_test_transaction_history();
    }
}
