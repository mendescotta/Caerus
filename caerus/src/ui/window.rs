use crate::backend::package::{Package, PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::backend::transaction::Transaction;
use crate::backend::transaction_preview::PreviewOp;
use crate::ui::apply_confirm;
use crate::ui::apply_dialog;
use crate::ui::detail_pane::DetailPane;
use crate::ui::filter_sidebar::FilterSidebar;
use crate::ui::package_list::PackageList;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

struct WindowState {
    window: gtk::ApplicationWindow,
    store: PackageStore,
    session: Transaction,
    sidebar: FilterSidebar,
    pkg_list: PackageList,
    detail_pane: DetailPane,
    header: gtk::HeaderBar,
    root_box: gtk::Box,
    filters_button: gtk::MenuButton,
    repositories_button: gtk::MenuButton,
    filter_row: gtk::Box,
    right_paned: gtk::Paned,
    desktop_tools: gtk::Box,
    btn_toggle_sidebar: gtk::ToggleButton,
    detail_page: gtk::Box,
    workflows: crate::ui::workflows::Workflows,
    pending_bar: gtk::Box,
    pending_label: gtk::Label,
    narrow: std::cell::Cell<bool>,
    wide_header_min: std::cell::Cell<i32>,

    spinner: gtk::Spinner,
    btn_update: gtk::Button,
    btn_reload: gtk::Button,
    btn_mark_upgrades: gtk::Button,
    btn_unmark_all: gtk::Button,
    btn_apply: gtk::Button,
    apply_count_pill: gtk::Label,
    menu_button: gtk::MenuButton,
    menu_stack: gtk::Stack,
    btn_toggle_detail_pane: gtk::ToggleButton,
    status_bar: gtk::Box,
    search_entry: gtk::SearchEntry,
    btn_search_name_only: gtk::ToggleButton,
    status_label: gtk::Label,

    #[cfg(feature = "adwaita")]
    toast_overlay: adw::ToastOverlay,

    selected_pkg: RefCell<Option<Package>>,

    sync_at_launch: std::cell::Cell<bool>,

    search_name_only_default: std::cell::Cell<bool>,
    auto_close_on_success: std::cell::Cell<bool>,
    action_busy: Cell<bool>,
}

struct WindowGeometry {
    width: i32,
    height: i32,
    sync_at_launch: bool,
    search_name_only_default: bool,
    section_expanded: [bool; 4],
    section_visible: [bool; 4],
    detail_pane_visible: bool,
    status_bar_visible: bool,
    stale_repos_visible: bool,
    auto_close_on_success: bool,
    discovery_visible: bool,
}

const SECTION_KEYS: [&str; 4] = ["filters", "repositories", "maintenance", "tools"];

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            width: 1240,
            height: 820,
            sync_at_launch: false,
            search_name_only_default: false,
            section_expanded: [true; 4],
            section_visible: [true; 4],
            detail_pane_visible: true,
            status_bar_visible: true,
            stale_repos_visible: true,
            auto_close_on_success: false,
            discovery_visible: true,
        }
    }
}

const VERTICAL_PANEL_DETAIL_WIDTH: i32 = 334;

fn set_right_paned_position(right_paned: &gtk::Paned, available_width_hint: i32) {
    // A surface resize arrives before the paned gets its new allocation.
    // Use the new content width instead of the previous narrow allocation.
    right_paned.set_position((available_width_hint - VERTICAL_PANEL_DETAIL_WIDTH).max(200));
    // Reparenting can clamp that position to the old allocation. Wait one
    // layout frame, then set it against the actual width and stop ticking.
    let allocated_once = std::cell::Cell::new(false);
    right_paned.add_tick_callback(move |paned, _| {
        if !allocated_once.replace(true) {
            return glib::ControlFlow::Continue;
        }
        paned.set_position((paned.width() - VERTICAL_PANEL_DETAIL_WIDTH).max(200));
        glib::ControlFlow::Break
    });
}

fn state_file_path() -> Option<std::path::PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(config_home.join("caerus").join("window-state.conf"))
}

impl WindowGeometry {
    fn load() -> Self {
        let mut geometry = Self::default();
        let Some(path) = state_file_path() else {
            return geometry;
        };
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return geometry;
        };
        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key == "discovery_visible" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.discovery_visible = b != 0;
                }
                continue;
            }
            if key == "sync_at_launch" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.sync_at_launch = b != 0;
                }
                continue;
            }
            if key == "search_name_only_default" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.search_name_only_default = b != 0;
                }
                continue;
            }
            if key == "auto_close_on_success" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.auto_close_on_success = b != 0;
                }
                continue;
            }
            if let Ok(b) = value.parse::<i32>().map(|b| b != 0) {
                if let Some(name) = key.strip_prefix("expanded_") {
                    if let Some(i) = SECTION_KEYS.iter().position(|k| *k == name) {
                        geometry.section_expanded[i] = b;
                        continue;
                    }
                }
                if let Some(name) = key.strip_prefix("visible_") {
                    if let Some(i) = SECTION_KEYS.iter().position(|k| *k == name) {
                        geometry.section_visible[i] = b;
                        continue;
                    }
                    match name {
                        "detail_pane" => {
                            geometry.detail_pane_visible = b;
                            continue;
                        }
                        "status_bar" => {
                            geometry.status_bar_visible = b;
                            continue;
                        }
                        "stale_repos" => {
                            geometry.stale_repos_visible = b;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            let Ok(n) = value.parse::<i32>() else {
                continue;
            };
            if n <= 0 {
                continue;
            }
            match key {
                "width" => geometry.width = n,
                "height" => geometry.height = n,
                _ => {}
            }
        }
        geometry
    }

    fn save(&self) {
        let Some(path) = state_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut contents = format!(
            "width={}\nheight={}\nsync_at_launch={}\nsearch_name_only_default={}\nauto_close_on_success={}\n",
            self.width,
            self.height,
            i32::from(self.sync_at_launch),
            i32::from(self.search_name_only_default),
            i32::from(self.auto_close_on_success)
        );
        contents.push_str(&format!(
            "discovery_visible={}\n",
            i32::from(self.discovery_visible)
        ));
        for (i, key) in SECTION_KEYS.iter().enumerate() {
            contents.push_str(&format!(
                "expanded_{key}={}\nvisible_{key}={}\n",
                i32::from(self.section_expanded[i]),
                i32::from(self.section_visible[i])
            ));
        }
        contents.push_str(&format!(
            "visible_detail_pane={}\nvisible_status_bar={}\nvisible_stale_repos={}\n",
            i32::from(self.detail_pane_visible),
            i32::from(self.status_bar_visible),
            i32::from(self.stale_repos_visible)
        ));
        let _ = std::fs::write(&path, contents);
    }
}

pub fn build_window(app: &gtk::Application) -> gtk::ApplicationWindow {
    build_window_state(app, WindowGeometry::load(), PackageStore::new(), true)
        .window
        .clone()
}

fn build_window_state(
    app: &gtk::Application,
    geometry: WindowGeometry,
    store: PackageStore,
    load_packages: bool,
) -> Rc<WindowState> {
    let window = gtk::ApplicationWindow::new(app);
    window.set_title(Some("Caerus"));
    window.set_default_size(geometry.width, geometry.height);

    install_css(&window);
    ensure_icon_theme_fallback(&window);

    let header = gtk::HeaderBar::new();

    let btn_toggle_sidebar = gtk::ToggleButton::new();
    btn_toggle_sidebar.set_icon_name("sidebar-show-symbolic");
    btn_toggle_sidebar.set_active(true);
    btn_toggle_sidebar.set_tooltip_text(Some("Show package filters (F9)"));

    let spinner = gtk::Spinner::new();
    let btn_update = gtk::Button::from_icon_name("software-update-available-symbolic");
    btn_update.set_tooltip_text(Some("Sync repositories and reload package list"));
    let btn_reload = gtk::Button::from_icon_name("view-refresh-symbolic");
    btn_reload.set_tooltip_text(Some("Reload local package list without syncing (F5)"));
    let btn_mark_upgrades = gtk::Button::from_icon_name("software-update-urgent-symbolic");
    btn_mark_upgrades.set_tooltip_text(Some("Mark All Updates"));
    let btn_unmark_all = gtk::Button::from_icon_name("edit-clear-all-symbolic");
    btn_unmark_all.set_sensitive(false);
    btn_unmark_all.set_tooltip_text(Some("Unmark All"));
    let mark_state_group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    mark_state_group.add_css_class("linked");
    mark_state_group.append(&btn_mark_upgrades);
    mark_state_group.append(&btn_unmark_all);

    // The 0.7 header toolbar; narrow windows keep only the essentials.
    let desktop_tools = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    desktop_tools.append(&btn_toggle_sidebar);
    header.pack_start(&desktop_tools);
    header.pack_start(&btn_update);
    let desktop_marks = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    desktop_marks.append(&btn_reload);
    desktop_marks.append(&mark_state_group);
    desktop_tools
        .bind_property("visible", &desktop_marks, "visible")
        .sync_create()
        .build();
    header.pack_start(&desktop_marks);
    header.pack_start(&spinner);

    let btn_apply = gtk::Button::new();
    btn_apply.set_sensitive(false);
    btn_apply.add_css_class("suggested-action");
    btn_apply.set_tooltip_text(Some("Review pending changes before applying (Ctrl+Enter)"));
    let apply_btn_content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    apply_btn_content.append(&gtk::Image::from_icon_name("object-select-symbolic"));
    let apply_count_pill = crate::ui::dialog_util::count_pill();
    apply_btn_content.append(&apply_count_pill);
    btn_apply.set_child(Some(&apply_btn_content));

    let btn_search_name_only = gtk::ToggleButton::new();
    btn_search_name_only.set_icon_name("edit-find-symbolic");
    btn_search_name_only
        .set_tooltip_text(Some("Search by name only (default: name + description)"));

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_width_request(180);
    search_entry.set_placeholder_text(Some("Search packages\u{2026}"));

    let btn_toggle_detail_pane = gtk::ToggleButton::new();
    btn_toggle_detail_pane.set_icon_name("sidebar-show-right-symbolic");
    btn_toggle_detail_pane.set_active(geometry.detail_pane_visible);
    btn_toggle_detail_pane.set_tooltip_text(Some("Show/hide the detail panel"));
    header.pack_end(&btn_toggle_detail_pane);

    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Menu"));
    let menu_stack = gtk::Stack::new();
    header.pack_end(&menu_button);

    let search_group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    search_group.add_css_class("linked");
    search_group.append(&search_entry);
    search_group.append(&btn_search_name_only);
    header.pack_end(&search_group);
    header.pack_end(&btn_apply);

    window.set_titlebar(Some(&header));

    let session = Transaction::new();

    let sidebar = FilterSidebar::new();
    let pkg_list = PackageList::new(store.clone());
    let detail_pane = DetailPane::new(store.clone());
    let right_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    right_paned.set_resize_start_child(true);
    right_paned.set_shrink_start_child(false);
    right_paned.set_resize_end_child(false);
    right_paned.set_shrink_end_child(false);
    right_paned.set_start_child(Some(pkg_list.widget()));
    right_paned.set_end_child(Some(detail_pane.widget()));
    set_right_paned_position(
        &right_paned,
        geometry.width - crate::ui::filter_sidebar::FULL_WIDTH,
    );

    let filters_button =
        sidebar.filter_button(crate::ui::filter_sidebar::Section::Filters, "All packages");
    filters_button.set_tooltip_text(Some("Filter by package state or a custom filter (F9)"));
    let repositories_button = sidebar.filter_button(
        crate::ui::filter_sidebar::Section::Repositories,
        "All repositories",
    );
    repositories_button.set_tooltip_text(Some("Filter by repository"));
    // Narrow windows replace the sidebar with these two dropdowns.
    let filter_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    filter_row.add_css_class("narrow-filters");
    filter_row.append(&filters_button);
    filter_row.append(&repositories_button);
    // The sidebar's Maintenance and Tools sections, reachable without it.
    let actions_button = sidebar.actions_menu_button();
    actions_button.set_hexpand(true);
    actions_button.set_halign(gtk::Align::End);
    filter_row.append(&actions_button);
    filter_row.set_visible(false);

    let main_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    main_paned.set_vexpand(true);
    main_paned.set_position(crate::ui::filter_sidebar::FULL_WIDTH);
    main_paned.set_resize_start_child(false);
    main_paned.set_shrink_start_child(false);
    main_paned.set_resize_end_child(true);
    main_paned.set_start_child(Some(sidebar.widget()));
    main_paned.set_end_child(Some(&right_paned));

    let packages_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    packages_page.add_css_class("packages-page");
    packages_page.append(&filter_row);
    packages_page.append(&main_paned);

    let status_label = gtk::Label::new(Some("Loading\u{2026}"));
    status_label.set_xalign(0.0);
    status_label.set_margin_start(8);
    status_label.set_margin_top(3);
    status_label.set_margin_bottom(3);
    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    status_bar.add_css_class("statusbar");
    status_bar.append(&status_label);

    let workflows = crate::ui::workflows::Workflows::new(&packages_page);
    let detail_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let details_back = gtk::Button::with_label("Back to packages");
    details_back.set_halign(gtk::Align::Start);
    details_back.set_margin_start(12);
    details_back.set_margin_top(8);
    details_back.set_margin_bottom(8);
    {
        let stack = workflows.stack.clone();
        details_back.connect_clicked(move |_| stack.set_visible_child_name("packages"));
    }
    detail_page.append(&details_back);
    let pending_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    pending_bar.add_css_class("pending-bar");
    let pending_label = gtk::Label::new(None);
    pending_label.set_xalign(0.0);
    pending_label.set_hexpand(true);
    pending_label.set_wrap(true);
    let clear = gtk::Button::with_label("Clear");
    let review = gtk::Button::with_label("Review changes");
    review.add_css_class("suggested-action");
    pending_bar.append(&pending_label);
    pending_bar.append(&clear);
    pending_bar.append(&review);
    pending_bar.set_visible(false);
    let root_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    header.set_title_widget(Some(&workflows.navigation));
    desktop_marks.append(&workflows.history);
    root_box.append(&workflows.stack);
    root_box.append(&pending_bar);
    root_box.append(&status_bar);

    #[cfg(feature = "adwaita")]
    let toast_overlay = adw::ToastOverlay::new();
    #[cfg(feature = "adwaita")]
    {
        toast_overlay.set_child(Some(&root_box));
        window.set_child(Some(&toast_overlay));
    }
    #[cfg(not(feature = "adwaita"))]
    window.set_child(Some(&root_box));

    let state = Rc::new(WindowState {
        window: window.clone(),
        store,
        session,
        sidebar,
        pkg_list,
        detail_pane,
        header,
        root_box,
        filters_button,
        repositories_button,
        filter_row,
        right_paned,
        desktop_tools,
        btn_toggle_sidebar,
        detail_page,
        workflows,
        pending_bar,
        pending_label,
        narrow: std::cell::Cell::new(false),
        wide_header_min: std::cell::Cell::new(0),
        spinner,
        btn_update,
        btn_reload,
        btn_mark_upgrades,
        btn_unmark_all,
        btn_apply,
        apply_count_pill,
        menu_button,
        menu_stack,
        btn_toggle_detail_pane: btn_toggle_detail_pane.clone(),
        status_bar: status_bar.clone(),
        search_entry,
        btn_search_name_only,
        status_label,
        #[cfg(feature = "adwaita")]
        toast_overlay,
        selected_pkg: RefCell::new(None),
        sync_at_launch: std::cell::Cell::new(geometry.sync_at_launch),
        search_name_only_default: std::cell::Cell::new(geometry.search_name_only_default),
        auto_close_on_success: std::cell::Cell::new(geometry.auto_close_on_success),
        action_busy: Cell::new(false),
    });

    {
        let weak = Rc::downgrade(&state);
        review.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                show_pending_changes(&state);
            }
        });
    }
    {
        let weak = Rc::downgrade(&state);
        clear.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.store.clear_all_marks();
                state.detail_pane.refresh_actions();
                update_status_bar(&state);
            }
        });
    }
    wire_workflows(&state);
    wire_up(&state);
    wire_keyboard_shortcuts(&state);

    for (i, section) in crate::ui::filter_sidebar::Section::ALL
        .into_iter()
        .enumerate()
    {
        state
            .sidebar
            .set_expanded(section, geometry.section_expanded[i]);
        state
            .sidebar
            .section_widget(section)
            .set_visible(i < 2 || geometry.section_visible[i]);
    }
    state
        .detail_pane
        .widget()
        .set_visible(geometry.detail_pane_visible);
    state.status_bar.set_visible(geometry.status_bar_visible);
    state
        .sidebar
        .set_show_stale_repositories(geometry.stale_repos_visible);
    crate::ui::apply_dialog::set_auto_close_on_success(geometry.auto_close_on_success);

    state
        .workflows
        .discover_button
        .set_visible(geometry.discovery_visible);
    populate_menu_popover(&state);

    state
        .btn_search_name_only
        .set_active(geometry.search_name_only_default);

    for section in crate::ui::filter_sidebar::Section::ALL {
        state.sidebar.section_widget(section).set_visible(true);
    }
    // Restore a saved narrow window in its narrow layout before measuring it.
    update_adaptive_layout(&state, geometry.width);
    crate::startup::trace("window widgets and callbacks constructed");
    if load_packages {
        if geometry.sync_at_launch {
            // Publish local data before authentication or network access begins.
            let pending = Rc::new(std::cell::Cell::new(true));
            let weak = Rc::downgrade(&state);
            let sync = Rc::new(move || {
                if pending.replace(false) {
                    let weak = weak.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(state) = weak.upgrade() {
                            trigger_update(&state, true, true);
                        }
                    });
                }
            });
            let after_load = sync.clone();
            state.store.connect_load_finished(move |_| after_load());
            state.store.connect_load_error(move |_| sync());
        }
        trigger_update(&state, false, true);
    }

    state
}

fn install_css(window: &gtk::ApplicationWindow) {
    let css = gtk::CssProvider::new();
    css.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(window),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

const USED_SYMBOLIC_ICONS: &[&str] = &[
    "software-update-available-symbolic",
    "software-update-urgent-symbolic",
    "view-refresh-symbolic",
    "sidebar-show-symbolic",
    "sidebar-show-right-symbolic",
    "edit-find-symbolic",
    "open-menu-symbolic",
    "user-trash-symbolic",
    "object-select-symbolic",
    "list-remove-symbolic",
    "edit-delete-symbolic",
    "list-add-symbolic",
    "media-playback-pause-symbolic",
    "dialog-warning-symbolic",
    "view-list-symbolic",
    "starred-symbolic",
    "edit-clear-symbolic",
    "edit-clear-all-symbolic",
    "security-high-symbolic",
    "applications-utilities-symbolic",
    "applications-system-symbolic",
    "application-x-firmware-symbolic",
    "object-flip-horizontal-symbolic",
    "document-open-recent-symbolic",
    "network-server-symbolic",
    "hold-symbolic",
    "unhold-symbolic",
    "repo-lock-symbolic",
    "repo-unlock-symbolic",
    "mark-manual-symbolic",
    "mark-auto-symbolic",
    "download-only-symbolic",
    "reinstall-symbolic",
    "package-x-generic-symbolic",
];

fn ensure_icon_theme_fallback(window: &gtk::ApplicationWindow) {
    let icon_theme = gtk::IconTheme::for_display(&gtk::prelude::WidgetExt::display(window));

    let all_present = USED_SYMBOLIC_ICONS
        .iter()
        .all(|name| icon_theme.has_icon(name));
    if all_present {
        return;
    }

    if let Some(dir) = bundled_icons_dir() {
        icon_theme.add_search_path(dir);
    }
}

fn bundled_icons_dir() -> Option<std::path::PathBuf> {
    let self_exe = std::fs::read_link("/proc/self/exe").ok()?;
    let candidate = self_exe
        .parent()?
        .parent()?
        .parent()?
        .join("caerus")
        .join("data")
        .join("icons");
    candidate.join("hicolor").is_dir().then_some(candidate)
}

fn flat_menu_button(label: &str) -> gtk::Button {
    let btn = gtk::Button::with_label(label);
    btn.set_has_frame(false);
    if let Some(l) = btn.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    btn
}

fn menu_page_header(stack: &gtk::Stack, title: &str) -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let back = gtk::Button::with_label("\u{2039}");
    back.update_property(&[gtk::accessible::Property::Label("Back to main menu")]);
    back.set_has_frame(false);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("root"));
    }
    let title_label = gtk::Label::new(Some(title));
    title_label.add_css_class("heading");
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    header.append(&back);
    header.append(&title_label);
    header
}

fn switch_row_with(switch: &gtk::Switch, label: &str, accel: Option<&str>) -> gtk::Box {
    switch.update_property(&[gtk::accessible::Property::Label(label)]);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_margin_start(8);
    row.set_margin_end(8);
    row.set_margin_top(3);
    row.set_margin_bottom(3);

    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    row.append(&l);

    if let Some(accel) = accel {
        let kbd = gtk::Label::new(Some(accel));
        kbd.add_css_class("keycap");
        row.append(&kbd);
    }

    switch.set_valign(gtk::Align::Center);
    row.append(switch);
    row
}

fn switch_row(label: &str, accel: Option<&str>) -> (gtk::Box, gtk::Switch) {
    let switch = gtk::Switch::new();
    let row = switch_row_with(&switch, label, accel);
    (row, switch)
}

fn toggle_package_filters(state: &Rc<WindowState>) {
    state.workflows.stack.set_visible_child_name("packages");
    if !state.narrow.get() {
        state
            .btn_toggle_sidebar
            .set_active(!state.btn_toggle_sidebar.is_active());
    } else if let Some(popover) = state.filters_button.popover() {
        if popover.is_visible() {
            popover.popdown();
        } else {
            state.filters_button.popup();
        }
    }
}

fn populate_menu_popover(state: &Rc<WindowState>) {
    let stack = &state.menu_stack;
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);

    let popover = gtk::Popover::new();
    popover.set_child(Some(stack));
    state.menu_button.set_popover(Some(&popover));

    {
        let stack = stack.clone();
        popover.connect_closed(move |_| stack.set_visible_child_name("root"));
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
    root.set_width_request(230);

    let nav_row = |label: &str, target: &'static str| -> gtk::Button {
        let btn = gtk::Button::new();
        btn.set_has_frame(false);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = gtk::Label::new(Some(label));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        let chevron = gtk::Label::new(Some("\u{25b8}"));
        chevron.add_css_class("dim-label");
        row.append(&l);
        row.append(&chevron);
        btn.set_child(Some(&row));
        let stack = stack.clone();
        btn.connect_clicked(move |_| stack.set_visible_child_name(target));
        btn
    };

    root.append(&nav_row("View", "view"));
    root.append(&nav_row("Settings", "settings"));
    root.append(&nav_row("Keyboard Shortcuts", "shortcuts"));
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let btn_about = flat_menu_button("About Caerus");
    {
        let window = state.window.clone();
        let popover = popover.clone();
        btn_about.connect_clicked(move |_| {
            popover.popdown();
            show_about_dialog(&window);
        });
    }
    root.append(&btn_about);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let btn_quit = gtk::Button::new();
    btn_quit.set_has_frame(false);
    {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = gtk::Label::new(Some("Quit"));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        let kbd = gtk::Label::new(Some("Ctrl+Q"));
        kbd.add_css_class("keycap");
        row.append(&l);
        row.append(&kbd);
        btn_quit.set_child(Some(&row));
    }
    {
        let window = state.window.clone();
        btn_quit.connect_clicked(move |_| window.close());
    }
    root.append(&btn_quit);
    stack.add_named(&root, Some("root"));

    let view = gtk::Box::new(gtk::Orientation::Vertical, 2);
    view.set_width_request(250);
    view.append(&menu_page_header(stack, "View"));

    let (stale_row, sw_stale) = switch_row("Stale Repositories", None);
    stale_row.set_tooltip_text(Some(
        "Show repositories that installed packages came from but that are no longer \
         configured in xbps.d",
    ));
    sw_stale.set_active(state.sidebar.show_stale_repositories());
    {
        let state = state.clone();
        sw_stale.connect_active_notify(move |sw| {
            state.sidebar.set_show_stale_repositories(sw.is_active());
        });
    }
    view.append(&stale_row);

    view.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let (detail_row, sw_detail) = switch_row("Detail Pane", None);
    state
        .btn_toggle_detail_pane
        .bind_property("active", &sw_detail, "active")
        .bidirectional()
        .sync_create()
        .build();
    view.append(&detail_row);

    let (status_row, sw_status) = switch_row("Status Bar", None);
    state
        .status_bar
        .bind_property("visible", &sw_status, "active")
        .bidirectional()
        .sync_create()
        .build();
    view.append(&status_row);
    let (discover_row, sw_discover) = switch_row("Show Discover", None);
    state
        .workflows
        .discover_button
        .bind_property("visible", &sw_discover, "active")
        .bidirectional()
        .sync_create()
        .build();
    view.append(&discover_row);
    stack.add_named(&view, Some("view"));

    let settings = gtk::Box::new(gtk::Orientation::Vertical, 2);
    settings.set_width_request(290);
    settings.append(&menu_page_header(stack, "Settings"));

    let (sync_row, sw_sync) = switch_row("Sync repositories at launch", None);
    sync_row.set_tooltip_text(Some(
        "When enabled, Caerus syncs repository indexes (a privileged action, prompting for \
         your password) automatically every time it starts. Disable this to skip that prompt \
         at launch — you can still sync manually any time via the refresh button.",
    ));
    sw_sync.set_active(state.sync_at_launch.get());
    {
        let state = state.clone();
        sw_sync.connect_active_notify(move |sw| state.sync_at_launch.set(sw.is_active()));
    }
    settings.append(&sync_row);

    let (search_row, sw_search) = switch_row("Search names only by default", None);
    search_row.set_tooltip_text(Some(
        "Controls what the name-only search toggle starts as the next time \
         Caerus launches — doesn't change the current session's search mode.",
    ));
    sw_search.set_active(state.search_name_only_default.get());
    {
        let state = state.clone();
        sw_search.connect_active_notify(move |sw| {
            state.search_name_only_default.set(sw.is_active());
        });
    }
    settings.append(&search_row);

    let (auto_close_row, sw_auto_close) =
        switch_row("Close dialogs automatically on success", None);
    auto_close_row.set_tooltip_text(Some(
        "When enabled, progress dialogs (install, upgrade, remove, purge, \u{2026}) close \
         themselves as soon as they finish successfully, instead of waiting for you to click \
         Close. Dialogs that finish with errors always stay open.",
    ));
    sw_auto_close.set_active(state.auto_close_on_success.get());
    {
        let state = state.clone();
        sw_auto_close.connect_active_notify(move |sw| {
            let enabled = sw.is_active();
            state.auto_close_on_success.set(enabled);
            crate::ui::apply_dialog::set_auto_close_on_success(enabled);
        });
    }
    settings.append(&auto_close_row);
    stack.add_named(&settings, Some("settings"));

    let shortcuts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    shortcuts.set_width_request(260);
    shortcuts.append(&menu_page_header(stack, "Keyboard Shortcuts"));

    let essentials: &[(&str, &str)] = &[
        ("Search", "Ctrl+F"),
        ("Reload Package List", "F5"),
        ("Select All", "Ctrl+A"),
        ("Show package filters", "F9"),
        ("Settings", "Ctrl+,"),
        ("Quit", "Ctrl+Q"),
    ];
    for (desc, key) in essentials {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.set_margin_start(8);
        row.set_margin_end(8);
        row.set_margin_top(2);
        row.set_margin_bottom(2);
        let l = gtk::Label::new(Some(desc));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        let kbd = gtk::Label::new(Some(key));
        kbd.add_css_class("keycap");
        row.append(&l);
        row.append(&kbd);
        shortcuts.append(&row);
    }
    let caption = gtk::Label::new(Some("Essentials only — press Ctrl+? for the full overlay"));
    caption.add_css_class("dim-label");
    caption.set_margin_top(4);
    shortcuts.append(&caption);
    stack.add_named(&shortcuts, Some("shortcuts"));
}

#[cfg(feature = "adwaita")]
fn show_about_dialog(parent: &gtk::ApplicationWindow) {
    let about = adw::AboutWindow::builder()
        .transient_for(parent)
        .modal(true)
        .application_name("Caerus")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(if cfg!(feature = "apk") {
            "A Synaptic-inspired package manager for Chimera Linux, using APK."
        } else {
            "A Synaptic-inspired package manager for Void Linux, built directly on libxbps."
        })
        .website("https://github.com/mendescotta/Caerus")
        .application_icon(crate::APP_ID)
        .license_type(gtk::License::Gpl30)
        .build();
    about.present();
    gtk::prelude::GtkWindowExt::set_focus(&about, None::<&gtk::Widget>);
}

#[cfg(not(feature = "adwaita"))]
fn show_about_dialog(parent: &gtk::ApplicationWindow) {
    let (about, content) = crate::ui::dialog_util::modal_window(
        "About Caerus",
        Some(parent.upcast_ref::<gtk::Window>()),
        false,
        (520, -1),
        10,
    );
    content.set_halign(gtk::Align::Center);

    let logo = gtk::Image::from_icon_name(crate::APP_ID);
    logo.set_pixel_size(96);
    logo.set_margin_bottom(2);
    content.append(&logo);

    let name = gtk::Label::new(Some("Caerus"));
    name.add_css_class("title-1");
    content.append(&name);

    let version = gtk::Label::new(Some(&format!("Version {}", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("dim-label");
    content.append(&version);

    let description = gtk::Label::new(Some(if cfg!(feature = "apk") {
        "A Synaptic-inspired package manager for Chimera Linux, using APK."
    } else {
        "A Synaptic-inspired package manager for Void Linux, built directly on libxbps."
    }));
    description.set_wrap(true);
    description.set_justify(gtk::Justification::Center);
    description.set_max_width_chars(58);
    description.set_margin_top(4);
    content.append(&description);

    let website = gtk::LinkButton::with_label(
        "https://github.com/mendescotta/Caerus",
        "Caerus project website",
    );
    content.append(&website);

    let license = gtk::Label::new(Some(
        "Licensed under the GNU General Public License, version 3 or later.",
    ));
    license.add_css_class("dim-label");
    license.set_wrap(true);
    license.set_justify(gtk::Justification::Center);
    license.set_max_width_chars(58);
    content.append(&license);

    crate::ui::dialog_util::close_button(&content, &about, 4);
    about.present();
    gtk::prelude::GtkWindowExt::set_focus(&about, None::<&gtk::Widget>);
}

fn show_shortcuts_dialog(parent: &gtk::ApplicationWindow) {
    let (dlg, outer) = crate::ui::dialog_util::modal_window(
        "Keyboard Shortcuts",
        Some(parent.upcast_ref::<gtk::Window>()),
        false,
        (-1, -1),
        6,
    );

    let shortcuts: &[(&str, &str)] = &[
        ("Ctrl+F", "Focus package search"),
        ("Ctrl+Enter", "Review pending changes"),
        ("Escape", "Clear search, or close the current dialog"),
        ("F5", "Reload package list"),
        ("F9", "Show package filters"),
        ("Delete", "Mark selected package(s) for removal"),
        (
            "Ctrl+A",
            "Select all visible packages (for right-click bulk actions)",
        ),
        ("Ctrl+,", "Open settings"),
        ("Ctrl+Q", "Quit"),
    ];
    for (key, desc) in shortcuts {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        let key_label = gtk::Label::new(Some(key));
        key_label.set_width_chars(10);
        key_label.set_xalign(0.0);
        key_label.add_css_class("heading");
        let desc_label = gtk::Label::new(Some(desc));
        desc_label.set_xalign(0.0);
        desc_label.set_hexpand(true);
        row.append(&key_label);
        row.append(&desc_label);
        outer.append(&row);
    }

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_margin_top(10);
    {
        let dlg2 = dlg.clone();
        close_btn.connect_clicked(move |_| dlg2.destroy());
    }
    outer.append(&close_btn);

    crate::ui::dialog_util::present_focused(&dlg, &close_btn);
}

fn search_has_focus(state: &WindowState) -> bool {
    gtk::prelude::GtkWindowExt::focus(&state.window)
        .is_some_and(|focus| focus == state.search_entry || focus.is_ancestor(&state.search_entry))
}

fn wire_keyboard_shortcuts(state: &Rc<WindowState>) {
    let controller = gtk::EventControllerKey::new();
    let window = state.window.clone();
    let state = state.clone();
    controller.connect_key_pressed(move |_, key, _keycode, modifiers| {
        if state.action_busy.get() {
            return glib::Propagation::Stop;
        }
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        match key {
            gtk::gdk::Key::f if ctrl => {
                state.workflows.stack.set_visible_child_name("packages");
                state.search_entry.grab_focus();
                let entry = state.search_entry.clone();
                glib::idle_add_local_once(move || {
                    entry.grab_focus();
                });
                glib::Propagation::Stop
            }
            gtk::gdk::Key::q if ctrl => {
                state.window.close();
                glib::Propagation::Stop
            }
            gtk::gdk::Key::a
                if ctrl
                    && !search_has_focus(&state)
                    && state.workflows.stack.visible_child_name().as_deref()
                        == Some("packages") =>
            {
                state.pkg_list.select_all();
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Escape if !state.search_entry.text().is_empty() => {
                state.search_entry.set_text("");
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Return if ctrl => {
                show_pending_changes(&state);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Escape
                if state.workflows.stack.visible_child_name().as_deref() == Some("details") =>
            {
                state.workflows.stack.set_visible_child_name("packages");
                glib::Propagation::Stop
            }
            gtk::gdk::Key::F5 => {
                trigger_update(&state, false, false);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::F9 => {
                toggle_package_filters(&state);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::question if ctrl => {
                show_shortcuts_dialog(&state.window);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::comma if ctrl => {
                state.menu_stack.set_visible_child_name("settings");
                state.menu_button.popup();
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Delete
                if !search_has_focus(&state)
                    && state.workflows.stack.visible_child_name().as_deref()
                        == Some("packages") =>
            {
                let root = state.window.clone().upcast::<gtk::Window>();
                state.pkg_list.delete_selected(Some(root));
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(controller);
}

fn wire_up(state: &Rc<WindowState>) {
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_started(move || {
            set_loading(&state, true);
            state
                .status_label
                .set_text("Loading package database\u{2026}");
        });
    }
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_finished(move |n| {
            set_loading(&state, false);
            crate::startup::trace(format_args!(
                "loading completed: {n} packages, {} visible",
                state.pkg_list.visible_counts().0
            ));
            let seen_frame = std::cell::Cell::new(false);
            state.pkg_list.widget().add_tick_callback(move |_, _| {
                if !seen_frame.replace(true) {
                    return glib::ControlFlow::Continue;
                }
                crate::startup::trace("package list frame ready");
                glib::ControlFlow::Break
            });
            update_status_bar(&state);
            refresh_workflow_packages(&state);
            state.sidebar.set_available_repositories(
                state.pkg_list.available_repositories(),
                &crate::ui::repo_manager::configured_repo_urls(),
            );
        });
    }
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_error(move |msg| {
            set_loading(&state, false);
            state.workflows.load_error(msg);
            state.pkg_list.set_error(msg);
            show_toast(&state, &format!("Error loading packages: {msg}"));
        });
    }

    {
        let sidebar = state.sidebar.clone();
        let state = state.clone();
        sidebar.connect_filter_changed(move |mode| {
            crate::ui::filter_sidebar::set_filter_button_label(
                &state.filters_button,
                &crate::ui::filter_sidebar::filter_label(&mode),
            );
            state.filters_button.popdown();
            state.pkg_list.set_filter(mode);
            update_status_bar(&state);
        });
    }
    {
        let sidebar = state.sidebar.clone();
        let state = state.clone();
        sidebar.connect_repository_changed(move |repo| {
            crate::ui::filter_sidebar::set_filter_button_label(
                &state.repositories_button,
                repo.as_deref()
                    .map_or("All repositories", crate::backend::repo_names::display_repo),
            );
            state.repositories_button.popdown();
            state.pkg_list.set_repository_filter(repo);
            update_status_bar(&state);
        });
    }
    {
        let pkg_list = state.pkg_list.clone();
        let state = state.clone();
        pkg_list.connect_package_selected(move |pkg| {
            *state.selected_pkg.borrow_mut() = pkg.clone();
            state.detail_pane.show_package(pkg.as_ref());
            if pkg.is_some() && state.narrow.get() && state.btn_toggle_detail_pane.is_active() {
                state.workflows.stack.set_visible_child_name("details");
            }
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_jump_to_package(move |pkgname| {
            if state.pkg_list.select_package_by_name(&pkgname) {
                return;
            }
            state.search_entry.set_text("");
            state.pkg_list.set_search("");
            state.sidebar.reset_to_all();
            state.pkg_list.select_package_by_name(&pkgname);
        });
    }
    {
        let pkg_list = state.pkg_list.clone();
        let state = state.clone();
        pkg_list.connect_marks_changed(move || {
            update_status_bar(&state);

            let refreshed = {
                let mut selected = state.selected_pkg.borrow_mut();
                if let Some(pkg) = selected.as_mut() {
                    if let Some((pkg_state, mark)) = state.store.state_and_mark(&pkg.name) {
                        pkg.state = pkg_state;
                        pkg.mark = mark;
                    }
                }
                selected.clone()
            };
            if let Some(pkg) = refreshed {
                state.detail_pane.show_package(Some(&pkg));
            }
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_mark_changed(move || {
            update_status_bar(&state);
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_hold_requested(move |pkgname, want_hold| {
            on_hold_requested(&state, &pkgname, want_hold);
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_reinstall_requested(move |pkgname| {
            run_maintenance_command(
                &state,
                &format!("REINSTALL {pkgname}"),
                "Reinstalling Package",
            );
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_reconfigure_requested(move |pkgname| {
            run_maintenance_command(
                &state,
                &format!("RECONFIGURE {pkgname}"),
                "Reconfiguring Package",
            );
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_download_requested(move |pkgname| {
            run_maintenance_command(
                &state,
                &format!("DOWNLOAD {pkgname}"),
                "Downloading Package",
            );
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_repolock_requested(move |pkgname, want_locked| {
            let cmd = if want_locked {
                format!("REPOLOCK {pkgname}")
            } else {
                format!("REPOUNLOCK {pkgname}")
            };
            let title = if want_locked {
                "Repo-Locking Package"
            } else {
                "Releasing Repo-Lock"
            };
            run_maintenance_command(&state, &cmd, title);
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_automatic_requested(move |pkgname, want_automatic| {
            let cmd = if want_automatic {
                format!("MARKAUTO {pkgname}")
            } else {
                format!("MARKMANUAL {pkgname}")
            };
            let title = if want_automatic {
                "Marking Automatic"
            } else {
                "Marking Manual"
            };
            run_maintenance_command(&state, &cmd, title);
        });
    }

    {
        let state = state.clone();
        state
            .clone()
            .sidebar
            .connect_action(move |action| dispatch_action(&state, action));
    }

    {
        let session = state.session.clone();
        let state = state.clone();
        session.connect_disconnected(move |reason| match reason {
            crate::backend::transaction::DisconnectReason::Expected => {}
            crate::backend::transaction::DisconnectReason::Unexpected => {
                show_toast(
                    &state,
                    "Privileged helper disconnected — the next action will re-authenticate.",
                );
            }
            crate::backend::transaction::DisconnectReason::AuthFailed => {
                show_toast(
                    &state,
                    "Could not authenticate as root — is a polkit authentication agent \
                     running for this session? Most desktop environments start one \
                     automatically; a bare window manager setup may need one added to \
                     its startup (e.g. polkit-gnome, lxqt-policykit, polkit-mate).",
                );
            }
        });
    }

    {
        let btn_update = state.btn_update.clone();
        let state = state.clone();
        btn_update.connect_clicked(move |_| {
            trigger_update(&state, true, false);
        });
    }
    {
        let btn_reload = state.btn_reload.clone();
        let state = state.clone();
        btn_reload.connect_clicked(move |_| {
            trigger_update(&state, false, false);
        });
    }
    {
        let btn_mark_upgrades = state.btn_mark_upgrades.clone();
        let state = state.clone();
        btn_mark_upgrades.connect_clicked(move |_| {
            let mut names = std::collections::HashSet::new();
            let list = state.store.list();
            let n = list.n_items();
            for i in 0..n {
                if let Some(obj) = crate::backend::package_store::package_obj_at(&list, i) {
                    let p = obj.pkg();
                    if p.state == PkgState::Upgradable && p.mark == PkgMark::None {
                        names.insert(p.name.clone());
                    }
                }
            }
            state.store.set_marks(&names, PkgMark::Upgrade);
            update_status_bar(&state);
        });
    }
    {
        let btn_unmark_all = state.btn_unmark_all.clone();
        let state = state.clone();
        btn_unmark_all.connect_clicked(move |_| {
            state.store.clear_all_marks();
            update_status_bar(&state);
        });
    }
    {
        let btn_apply = state.btn_apply.clone();
        let state = state.clone();
        btn_apply.connect_clicked(move |_| {
            show_pending_changes(&state);
        });
    }
    {
        let search_entry = state.search_entry.clone();
        let state = state.clone();
        search_entry.connect_search_changed(move |e| {
            if !e.text().trim().is_empty() {
                state.workflows.stack.set_visible_child_name("packages");
            }
            state.pkg_list.set_search(&e.text());
            update_status_bar(&state);
        });
    }
    {
        let btn_search_name_only = state.btn_search_name_only.clone();
        let state = state.clone();
        btn_search_name_only.connect_toggled(move |btn| {
            let name_only = btn.is_active();
            btn.set_tooltip_text(Some(if name_only {
                "Searching by name only (click for name + description)"
            } else {
                "Searching name + description (click for name only)"
            }));
            state.pkg_list.set_search_mode(name_only);
            update_status_bar(&state);
        });
    }

    {
        let window = state.window.clone();
        let state = state.clone();
        window.connect_close_request(move |win| {
            use crate::ui::filter_sidebar::Section;
            if state.action_busy.get() {
                return glib::Propagation::Stop;
            }
            WindowGeometry {
                width: win.width(),
                height: win.height(),
                sync_at_launch: state.sync_at_launch.get(),
                search_name_only_default: state.search_name_only_default.get(),
                section_expanded: Section::ALL.map(|s| state.sidebar.is_expanded(s)),
                section_visible: Section::ALL
                    .map(|s| state.sidebar.section_widget(s).get_visible()),
                detail_pane_visible: state.btn_toggle_detail_pane.is_active(),
                status_bar_visible: state.status_bar.get_visible(),
                stale_repos_visible: state.sidebar.show_stale_repositories(),
                auto_close_on_success: state.auto_close_on_success.get(),
                discovery_visible: state.workflows.discover_button.get_visible(),
            }
            .save();
            state.session.shutdown();
            glib::Propagation::Proceed
        });
    }
}

fn set_loading(state: &Rc<WindowState>, loading: bool) {
    state.workflows.set_loading(loading);
    state.pkg_list.set_loading(loading);
    if loading {
        state.spinner.start();
        state.btn_update.set_sensitive(false);
        state.btn_reload.set_sensitive(false);
        state.menu_button.set_sensitive(false);
        state.btn_apply.set_sensitive(false);
        state.btn_unmark_all.set_sensitive(false);
        state.btn_mark_upgrades.set_sensitive(false);
    } else {
        state.spinner.stop();
        let available = !state.action_busy.get() && !state.store.is_loading();
        state.btn_update.set_sensitive(available);
        state.btn_reload.set_sensitive(available);
        state.menu_button.set_sensitive(available);
    }
}

fn set_action_busy(state: &Rc<WindowState>, busy: bool) {
    state.action_busy.set(busy);
    let available = !busy && !state.store.is_loading();
    state.btn_update.set_sensitive(available);
    state.btn_reload.set_sensitive(available);
    state.menu_button.set_sensitive(available);
    state
        .btn_apply
        .set_sensitive(available && state.store.count_marked() > 0);
    state
        .btn_unmark_all
        .set_sensitive(available && state.store.count_marked() > 0);
    state
        .btn_mark_upgrades
        .set_sensitive(available && !state.store.upgradable_names().is_empty());
    state.pkg_list.widget().set_sensitive(!busy);
    state.detail_pane.widget().set_sensitive(!busy);
    state.sidebar.widget().set_sensitive(!busy);
}

fn do_reload(state: &Rc<WindowState>) {
    state.store.load_async();
}

fn trigger_update(state: &Rc<WindowState>, sync_first: bool, silent: bool) {
    if state.action_busy.get() || state.store.is_loading() {
        return;
    }
    set_loading(state, true);
    if sync_first {
        state.status_label.set_text(if silent {
            "Requesting authentication to sync repositories\u{2026}"
        } else {
            "Syncing repositories\u{2026}"
        });
        let commands = vec!["SYNC".to_string()];
        if silent {
            let state2 = state.clone();
            let commands_for_history = commands.clone();
            state.session.run_batch(commands, move |success| {
                crate::backend::history::record(&commands_for_history, success);
                state2.workflows.sync_finished(success);
                if success {
                    show_toast(&state2, "Repositories synced. Loading package list\u{2026}");
                } else {
                    show_toast(&state2, "Repository sync failed — loading local data.");
                }
                do_reload(&state2);
            });
        } else {
            let state2 = state.clone();
            apply_dialog::run_recorded(
                Some(state.window.upcast_ref()),
                &state.session,
                &commands,
                "Syncing Repositories",
                move |success| {
                    state2.workflows.sync_finished(success);
                    if !success {
                        show_toast(
                            &state2,
                            "Repository sync failed — loading local data anyway.",
                        );
                    }
                    do_reload(&state2);
                },
            );
        }
    } else {
        state
            .status_label
            .set_text("Loading package database\u{2026}");
        do_reload(state);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApplyKind {
    Install,
    Update,
    Remove,
    Purge,
}

struct ApplyStep {
    kind: ApplyKind,
    command: String,
    names: Vec<String>,
    install_count: usize,
}

impl ApplyStep {
    fn new(kind: ApplyKind, names: Vec<String>, install_count: usize) -> Self {
        let verb = match kind {
            ApplyKind::Install => "INSTALL",
            ApplyKind::Update => "UPDATE",
            ApplyKind::Remove => "REMOVE",
            ApplyKind::Purge => "PURGE",
        };
        Self {
            kind,
            command: format!("{verb} {}", names.join(" ")),
            names,
            install_count,
        }
    }

    fn preview_ops(&self) -> Vec<PreviewOp> {
        self.names
            .iter()
            .map(|name| match self.kind {
                // The helper uses xbps-install for both new and installed packages.
                ApplyKind::Install => PreviewOp::Install(name.clone()),
                ApplyKind::Update => PreviewOp::Update(name.clone()),
                ApplyKind::Remove => PreviewOp::Remove(name.clone()),
                ApplyKind::Purge => PreviewOp::Purge(name.clone()),
            })
            .collect()
    }
}

fn apply_steps(
    installs: Vec<String>,
    upgrades: Vec<String>,
    removes: Vec<String>,
    purges: Vec<String>,
) -> VecDeque<ApplyStep> {
    let mut steps = VecDeque::new();
    let install_count = installs.len();
    if cfg!(feature = "apk") {
        if !installs.is_empty() {
            let install_count = installs.len();
            steps.push_back(ApplyStep::new(ApplyKind::Install, installs, install_count));
        }
        if !upgrades.is_empty() {
            steps.push_back(ApplyStep::new(ApplyKind::Update, upgrades, 0));
        }
    } else {
        let install_names: Vec<String> = installs.into_iter().chain(upgrades).collect();
        if !install_names.is_empty() {
            steps.push_back(ApplyStep::new(
                ApplyKind::Install,
                install_names,
                install_count,
            ));
        }
    }
    if !removes.is_empty() {
        steps.push_back(ApplyStep::new(ApplyKind::Remove, removes, 0));
    }
    if !purges.is_empty() {
        steps.push_back(ApplyStep::new(ApplyKind::Purge, purges, 0));
    }
    steps
}

struct ApplyFlow {
    steps: VecDeque<ApplyStep>,
    completed: usize,
    failed: usize,
    stopped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApplyEvent {
    CommandSucceeded,
    CommandFailed,
    ConfirmationCancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApplyDecision {
    Continue,
    Stop,
}

impl ApplyFlow {
    fn next_step(&mut self) -> Option<ApplyStep> {
        if self.stopped {
            return None;
        }
        self.steps.pop_front()
    }

    fn record_event(&mut self, event: ApplyEvent) -> ApplyDecision {
        match event {
            ApplyEvent::CommandSucceeded => {
                self.completed += 1;
                ApplyDecision::Continue
            }
            ApplyEvent::CommandFailed => {
                self.failed += 1;
                self.stopped = true;
                ApplyDecision::Stop
            }
            ApplyEvent::ConfirmationCancelled => {
                self.stopped = true;
                ApplyDecision::Stop
            }
        }
    }
}

fn finish_apply(state: &Rc<WindowState>, flow: &Rc<RefCell<ApplyFlow>>, cancelled: bool) {
    let flow = flow.borrow();
    let message = if cancelled && flow.completed == 0 && flow.failed == 0 {
        "Apply cancelled. Marks are unchanged."
    } else if cancelled {
        "Apply stopped. Completed changes are saved; remaining marks need review."
    } else if flow.failed > 0 {
        "Some commands failed. Completed changes are saved; review remaining marks and logs."
    } else {
        "Changes applied."
    };
    set_action_busy(state, false);
    update_status_bar(state);
    show_toast(state, message);
}

fn run_next_apply_step(state: &Rc<WindowState>, flow: &Rc<RefCell<ApplyFlow>>) {
    let next_step = flow.borrow_mut().next_step();
    let Some(step) = next_step else {
        finish_apply(state, flow, false);
        return;
    };
    let ops = step.preview_ops();
    let state_for_preview = state.clone();
    let flow_for_preview = flow.clone();
    state.store.preview_transaction_async(ops, move |preview| {
        let state_for_confirm = state_for_preview.clone();
        let flow_for_confirm = flow_for_preview.clone();
        let command = step.command.clone();
        let names_for_command = step.names.clone();
        let changes = preview.as_ref().and_then(|p| p.as_ref().ok()).map_or_else(
            String::new,
            crate::backend::transaction_preview::TransactionPreview::changes,
        );
        let (installs, upgrades, removes, purges): (&[String], &[String], &[String], &[String]) =
            match step.kind {
                ApplyKind::Install => (
                    &step.names[..step.install_count],
                    &step.names[step.install_count..],
                    &[],
                    &[],
                ),
                ApplyKind::Update => (&[], &step.names, &[], &[]),
                ApplyKind::Remove => (&[], &[], &step.names, &[]),
                ApplyKind::Purge => (&[], &[], &[], &step.names),
            };
        apply_confirm::confirm(
            Some(state_for_preview.window.upcast_ref()),
            installs,
            upgrades,
            removes,
            purges,
            preview,
            move |confirmed| {
                if !confirmed {
                    flow_for_confirm
                        .borrow_mut()
                        .record_event(ApplyEvent::ConfirmationCancelled);
                    finish_apply(&state_for_confirm, &flow_for_confirm, true);
                    return;
                }
                let state_after_command = state_for_confirm.clone();
                let flow_after_command = flow_for_confirm.clone();
                let names = names_for_command.clone();
                apply_dialog::run_recorded_until_closed(
                    Some(state_for_confirm.window.upcast_ref()),
                    &state_for_confirm.session,
                    std::slice::from_ref(&command),
                    "Applying Changes",
                    changes.clone(),
                    move |success| {
                        let decision = flow_after_command.borrow_mut().record_event(if success {
                            ApplyEvent::CommandSucceeded
                        } else {
                            ApplyEvent::CommandFailed
                        });
                        if success {
                            let names: std::collections::HashSet<_> =
                                names.iter().cloned().collect();
                            state_after_command.store.set_marks(&names, PkgMark::None);
                        }
                        let state_after_reload = state_after_command.clone();
                        let flow_after_reload = flow_after_command.clone();
                        state_after_command.store.load_async_with_result(
                            move |result| match result {
                                Ok(()) => match decision {
                                    ApplyDecision::Continue => {
                                        run_next_apply_step(
                                            &state_after_reload,
                                            &flow_after_reload,
                                        );
                                    }
                                    ApplyDecision::Stop => {
                                        finish_apply(
                                            &state_after_reload,
                                            &flow_after_reload,
                                            false,
                                        );
                                    }
                                },
                                Err(err) => {
                                    finish_apply(&state_after_reload, &flow_after_reload, true);
                                    show_toast(
                                        &state_after_reload,
                                        &format!("Could not reload package state: {err}"),
                                    );
                                }
                            },
                        );
                    },
                );
            },
        );
    });
}

fn on_apply_clicked(state: &Rc<WindowState>) {
    if state.action_busy.get() || state.store.is_loading() {
        return;
    }
    let installs = state.store.marked_names(PkgMark::Install);
    let upgrades = state.store.marked_names(PkgMark::Upgrade);
    let removes = state.store.marked_names(PkgMark::Remove);
    let purges = state.store.marked_names(PkgMark::Purge);

    let steps = apply_steps(installs, upgrades, removes, purges);
    if steps.is_empty() {
        return;
    }
    set_action_busy(state, true);
    let flow = Rc::new(RefCell::new(ApplyFlow {
        steps,
        completed: 0,
        failed: 0,
        stopped: false,
    }));
    run_next_apply_step(state, &flow);
}

fn on_hold_requested(state: &Rc<WindowState>, pkgname: &str, want_hold: bool) {
    let cmd = if want_hold {
        format!("HOLD {pkgname}")
    } else {
        format!("UNHOLD {pkgname}")
    };
    let title = if want_hold {
        "Holding Package"
    } else {
        "Releasing Hold"
    };
    run_maintenance_command(state, &cmd, title);
}

fn held_back_note(held: &[String]) -> String {
    const SHOWN: usize = 5;
    if held.is_empty() {
        return String::new();
    }
    let mut names = held
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if held.len() > SHOWN {
        names.push_str(&format!(", +{} more", held.len() - SHOWN));
    }
    format!(" {} held back: {names}.", held.len())
}

fn on_full_upgrade_clicked(state: &Rc<WindowState>) {
    if state.action_busy.get() || state.store.is_loading() {
        return;
    }
    set_action_busy(state, true);
    let state_after_sync = state.clone();
    apply_dialog::run_recorded_until_closed(
        Some(state.window.upcast_ref()),
        &state.session,
        &["SYNC".to_string()],
        "Syncing Repositories",
        String::new(),
        move |success| {
            if !success {
                set_action_busy(&state_after_sync, false);
                show_toast(
                    &state_after_sync,
                    "Repository sync failed; upgrade was not started.",
                );
                return;
            }
            let state_after_reload = state_after_sync.clone();
            state_after_sync
                .store
                .load_async_with_result(move |result| {
                    if let Err(err) = result {
                        set_action_busy(&state_after_reload, false);
                        show_toast(
                            &state_after_reload,
                            &format!("Could not reload package state: {err}"),
                        );
                        return;
                    }
                    preview_full_upgrade(&state_after_reload);
                });
        },
    );
}

fn preview_full_upgrade(state: &Rc<WindowState>) {
    let upgrades = state.store.upgradable_names();
    let held_note = held_back_note(&state.store.held_back_names());
    state
        .status_label
        .set_text("Checking full upgrade transaction…");
    let state_for_preview = state.clone();
    #[cfg(feature = "apk")]
    let preview_ops = vec![PreviewOp::FullUpgrade];
    #[cfg(not(feature = "apk"))]
    let preview_ops = vec![PreviewOp::UpdateAll];
    state
        .store
        .preview_transaction_async(preview_ops, move |preview| {
            let state_for_confirm = state_for_preview.clone();
            let changes = preview.as_ref().and_then(|p| p.as_ref().ok()).map_or_else(
                String::new,
                crate::backend::transaction_preview::TransactionPreview::changes,
            );
            if matches!(&preview, Some(Ok(p)) if p.items.is_empty()) {
                set_action_busy(&state_for_preview, false);
                state_for_preview
                    .status_label
                    .set_text(&format!("Everything is already up to date.{held_note}"));
                return;
            }
            apply_confirm::confirm(
                Some(state_for_preview.window.upcast_ref()),
                &[],
                &upgrades,
                &[],
                &[],
                preview,
                move |confirmed| {
                    if !confirmed {
                        set_action_busy(&state_for_confirm, false);
                        update_status_bar(&state_for_confirm);
                        return;
                    }
                    let state_after_upgrade = state_for_confirm.clone();
                    apply_dialog::run_recorded_until_closed(
                        Some(state_for_confirm.window.upcast_ref()),
                        &state_for_confirm.session,
                        &["UPGRADE".to_string()],
                        "Full System Upgrade",
                        changes.clone(),
                        move |success| {
                            let state_after_reload = state_after_upgrade.clone();
                            state_after_upgrade
                                .store
                                .load_async_with_result(move |result| {
                                    set_action_busy(&state_after_reload, false);
                                    match result {
                                        Ok(()) if success => {
                                            show_toast(&state_after_reload, "Upgrade complete.");
                                        }
                                        Ok(()) => show_toast(
                                            &state_after_reload,
                                            "Upgrade failed or was incomplete; see log.",
                                        ),
                                        Err(err) => show_toast(
                                            &state_after_reload,
                                            &format!("Could not reload package state: {err}"),
                                        ),
                                    }
                                });
                        },
                    );
                },
            );
        });
}

fn on_remove_orphans_clicked(state: &Rc<WindowState>) {
    let mut orphans = Vec::new();
    let list = state.store.list();
    let n = list.n_items();
    for i in 0..n {
        if let Some(obj) = crate::backend::package_store::package_obj_at(&list, i) {
            if obj.pkg().is_orphan {
                orphans.push(obj.name());
            }
        }
    }
    if orphans.is_empty() {
        show_toast(state, "No orphaned packages to remove.");
        return;
    }
    orphans.sort();

    let (dlg, outer) = crate::ui::dialog_util::modal_window(
        "Remove Orphaned Packages?",
        Some(state.window.upcast_ref()),
        true,
        (420, -1),
        10,
    );

    let n = orphans.len();
    let heading = gtk::Label::new(Some(&format!(
        "This removes {} package{} that nothing else depends on anymore:",
        n,
        if n == 1 { "" } else { "s" },
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(360);
    scroll.set_vexpand(true);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for name in &orphans {
        list.append(&crate::ui::dialog_util::text_list_row(name, false));
    }
    scroll.set_child(Some(&list));
    outer.append(&scroll);

    let (btn_box, cancel_btn) = crate::ui::dialog_util::cancel_button_row(4);
    let remove_btn = gtk::Button::with_label("Remove Orphans");
    remove_btn.add_css_class("destructive-action");
    btn_box.append(&remove_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&cancel_btn));

    {
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| dlg.destroy());
    }
    {
        let state = state.clone();
        let dlg = dlg.clone();
        remove_btn.connect_clicked(move |_| {
            dlg.destroy();
            run_maintenance_command(&state, "ORPHANS", "Removing Orphaned Packages");
        });
    }

    crate::ui::dialog_util::present_focused(&dlg, &cancel_btn);
}

fn on_reconfigure_all_clicked(state: &Rc<WindowState>) {
    let (dlg, outer) = crate::ui::dialog_util::modal_window(
        "Reconfigure All Packages?",
        Some(state.window.upcast_ref()),
        false,
        (440, -1),
        10,
    );

    let heading = gtk::Label::new(Some(
        "This force-reruns the post-install configuration script of every \
         installed package (xbps-reconfigure -fa). It's useful after an \
         interrupted transaction or a libc upgrade, but can take a while \
         on a large system.",
    ));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let (btn_box, cancel_btn) = crate::ui::dialog_util::cancel_button_row(4);
    let go_btn = gtk::Button::with_label("Reconfigure All");
    go_btn.add_css_class("suggested-action");
    btn_box.append(&go_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&go_btn));

    {
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| dlg.destroy());
    }
    {
        let state = state.clone();
        let dlg = dlg.clone();
        go_btn.connect_clicked(move |_| {
            dlg.destroy();
            run_maintenance_command(&state, "RECONFIGURE_ALL", "Reconfiguring All Packages");
        });
    }

    crate::ui::dialog_util::present_focused(&dlg, &go_btn);
}

fn run_maintenance_command(state: &Rc<WindowState>, cmd: &str, title: &str) {
    let state2 = state.clone();
    apply_dialog::run_recorded(
        Some(state.window.upcast_ref()),
        &state.session,
        &[cmd.to_string()],
        title,
        move |success| {
            show_toast(
                &state2,
                if success {
                    "Done. Reloading\u{2026}"
                } else {
                    "Failed — see log. Reloading\u{2026}"
                },
            );
            do_reload(&state2);
        },
    );
}

fn show_toast(state: &Rc<WindowState>, msg: &str) {
    #[cfg(feature = "adwaita")]
    {
        state.toast_overlay.add_toast(adw::Toast::new(msg));
    }
    #[cfg(not(feature = "adwaita"))]
    {
        state.status_label.set_text(msg);
        let state = state.clone();
        glib::source::timeout_add_local_once(std::time::Duration::from_secs(6), move || {
            update_status_bar(&state);
        });
    }
}

fn update_status_bar(state: &Rc<WindowState>) {
    let list = state.store.list();
    let total = list.n_items();
    let mut installed = 0;
    let mut upgradable = 0;
    let mut held_back = 0;
    let mut marks = [0usize; 4];
    for index in 0..total {
        if let Some(object) = crate::backend::package_store::package_obj_at(&list, index) {
            let package = object.pkg();
            installed += u32::from(package.state != PkgState::NotInstalled);
            upgradable += u32::from(package.state == PkgState::Upgradable);
            held_back += usize::from(package.is_held_back());
            match package.mark {
                PkgMark::Install => marks[0] += 1,
                PkgMark::Upgrade => marks[1] += 1,
                PkgMark::Remove => marks[2] += 1,
                PkgMark::Purge => marks[3] += 1,
                PkgMark::None => {}
            }
        }
    }
    let marked = marks.iter().sum::<usize>() as u32;
    let page = state.workflows.stack.visible_child_name();
    if page.as_deref() == Some("updates") {
        state.status_label.set_text(&format!(
            "{upgradable} updates available. {held_back} held back. {marked} marked."
        ));
    } else if page.as_deref() == Some("discover") {
        state.status_label.set_text(&format!(
            "Applications from enabled repositories. {marked} marked."
        ));
    } else if state.pkg_list.has_active_filters() {
        let (total, installed, not_installed) = state.pkg_list.visible_counts();
        state.status_label.set_text(&format!(
            "{total} shown — {installed} installed, {not_installed} not installed.  {marked} marked."
        ));
    } else {
        state.status_label.set_text(&format!(
            "{total} packages.  {installed} installed.  {upgradable} upgradable.  {marked} marked."
        ));
    }
    let [installs, upgrades, removes, purges] = marks;
    let summary = [
        (installs, "install"),
        (upgrades, "upgrade"),
        (removes + purges, "removal"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, noun)| format!("{count} {noun}{}", if count == 1 { "" } else { "s" }))
    .collect::<Vec<_>>()
    .join(" · ");
    state.pending_label.set_markup(&format!(
        "<b>{marked} pending change{}</b>\n<small>{summary}</small>",
        if marked == 1 { "" } else { "s" },
    ));
    state
        .pending_label
        .update_property(&[gtk::accessible::Property::Description(&summary)]);
    state.pending_bar.set_visible(marked > 0);
    if page.as_deref() == Some("discover") {
        state.workflows.discovery.refresh_marks();
    }
    update_apply_button(state, marked);
    update_mark_upgrades_button(state, upgradable);
}

fn update_apply_button(state: &Rc<WindowState>, marked: u32) {
    crate::ui::dialog_util::set_count(
        &state.apply_count_pill,
        (marked > 0).then_some(marked as usize),
    );
    state
        .btn_apply
        .set_sensitive(marked > 0 && !state.action_busy.get() && !state.store.is_loading());
    state
        .btn_unmark_all
        .set_sensitive(marked > 0 && !state.action_busy.get() && !state.store.is_loading());
}

fn update_mark_upgrades_button(state: &Rc<WindowState>, upgradable: u32) {
    state
        .btn_mark_upgrades
        .set_sensitive(upgradable > 0 && !state.action_busy.get() && !state.store.is_loading());
}

fn dispatch_action(state: &Rc<WindowState>, action: crate::ui::filter_sidebar::SidebarAction) {
    use crate::ui::filter_sidebar::SidebarAction;
    match action {
        SidebarAction::FullUpgrade => on_full_upgrade_clicked(state),
        SidebarAction::RemoveOrphans => on_remove_orphans_clicked(state),
        SidebarAction::ForceRemove => {
            let names = state
                .pkg_list
                .selected()
                .into_iter()
                .map(|pkg| pkg.name)
                .collect();
            force_remove(state, names);
        }
        SidebarAction::CleanCache => {
            run_maintenance_command(state, "CLEANCACHE", "Cleaning Package Cache");
        }
        SidebarAction::VerifyDb => {
            run_maintenance_command(state, "VERIFY", "Verifying Package Database");
        }
        SidebarAction::Reconfigure => on_reconfigure_all_clicked(state),
        SidebarAction::PurgeKernels => {
            crate::ui::vkpurge_dialog::show(Some(state.window.upcast_ref()), &state.session);
        }
        SidebarAction::FindOwner => {
            crate::ui::file_owner_dialog::show(Some(state.window.upcast_ref()));
        }
        SidebarAction::Alternatives => {
            crate::ui::alternatives_dialog::show(Some(state.window.upcast_ref()), &state.session);
        }
        SidebarAction::History => {
            crate::ui::history_dialog::show(Some(state.window.upcast_ref()));
        }
        SidebarAction::ManageRepos => {
            let state_for_reload = state.clone();
            crate::ui::repo_manager::show(
                Some(state.window.upcast_ref()),
                &state.session,
                move || do_reload(&state_for_reload),
            );
        }
    }
}

/// `xbps-remove -F` after an explicit warning; never queued with marks.
fn force_remove(state: &Rc<WindowState>, names: Vec<String>) {
    let wanted: std::collections::HashSet<String> = names.into_iter().collect();
    let mut removable = Vec::new();
    let mut essential = Vec::new();
    let list = state.store.list();
    for index in 0..list.n_items() {
        if let Some(object) = crate::backend::package_store::package_obj_at(&list, index) {
            let pkg = object.pkg();
            if !wanted.contains(&pkg.name) || pkg.state == PkgState::NotInstalled {
                continue;
            }
            if pkg.essential {
                essential.push(pkg.name.clone());
            } else {
                removable.push(pkg.name.clone());
            }
        }
    }
    if !essential.is_empty() {
        show_toast(
            state,
            &format!(
                "Essential packages are never force-removed: {}",
                essential.join(", ")
            ),
        );
    }
    if removable.is_empty() {
        if essential.is_empty() {
            show_toast(
                state,
                "Select one or more installed packages in the list to force-remove them.",
            );
        }
        return;
    }
    removable.sort();
    let weak = Rc::downgrade(state);
    let command = format!("REMOVE_FORCE {}", removable.join(" "));
    crate::ui::remove_confirm::confirm_force_remove(
        Some(state.window.upcast_ref()),
        &state.store,
        removable,
        move |proceed| {
            if let Some(state) = weak.upgrade() {
                if proceed {
                    run_maintenance_command(&state, &command, "Force-Removing Packages");
                }
            }
        },
    );
}

fn refresh_workflow_packages(state: &Rc<WindowState>) {
    crate::startup::trace("secondary pages preparation started");
    let list = state.store.list();
    let mut packages: Vec<_> = (0..list.n_items())
        .filter_map(|i| {
            crate::backend::package_store::package_obj_at(&list, i).and_then(|object| {
                let package = object.pkg();
                (package.state == PkgState::Upgradable
                    || package.is_held_back()
                    || crate::ui::catalog::contains_package(&package.name))
                .then(|| package.clone())
            })
        })
        .collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    state.workflows.refresh_packages(&packages);
    crate::startup::trace("secondary pages preparation completed");
}

fn open_package(state: &Rc<WindowState>, name: &str) {
    state.workflows.stack.set_visible_child_name("packages");
    state.search_entry.set_text("");
    state.pkg_list.set_search("");
    state.sidebar.reset_to_all();
    state.btn_toggle_detail_pane.set_active(true);
    if state.pkg_list.select_package_by_name(name) && state.narrow.get() {
        state.workflows.stack.set_visible_child_name("details");
    }
}

fn wire_workflows(state: &Rc<WindowState>) {
    {
        let weak = Rc::downgrade(state);
        state
            .workflows
            .stack
            .connect_visible_child_name_notify(move |_| {
                if let Some(state) = weak.upgrade() {
                    update_status_bar(&state);
                }
            });
    }

    {
        let weak = Rc::downgrade(state);
        state
            .workflows
            .discover_button
            .connect_visible_notify(move |button| {
                if let Some(state) = weak.upgrade() {
                    if !button.get_visible()
                        && state.workflows.stack.visible_child_name().as_deref() == Some("discover")
                    {
                        state.workflows.stack.set_visible_child_name("packages");
                    }
                }
            });
    }

    {
        let weak = Rc::downgrade(state);
        state.btn_toggle_detail_pane.connect_toggled(move |button| {
            if let Some(state) = weak.upgrade() {
                state.detail_pane.widget().set_visible(button.is_active());
                if state.narrow.get() {
                    if button.is_active() && state.selected_pkg.borrow().is_some() {
                        state.workflows.stack.set_visible_child_name("details");
                    } else if state.workflows.stack.visible_child_name().as_deref()
                        == Some("details")
                    {
                        state.workflows.stack.set_visible_child_name("packages");
                    }
                }
            }
        });
    }

    {
        let weak = Rc::downgrade(state);
        state.btn_toggle_sidebar.connect_toggled(move |button| {
            if let Some(state) = weak.upgrade() {
                if !state.narrow.get() {
                    state.sidebar.widget().set_visible(button.is_active());
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        state.workflows.refresh.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                trigger_update(&state, true, false);
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        state.workflows.upgrade.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                on_full_upgrade_clicked(&state);
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        state.workflows.history.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                crate::ui::history_dialog::show(Some(state.window.upcast_ref()));
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        state
            .workflows
            .discovery
            .connect_store(state.store.clone(), move || {
                if let Some(state) = weak.upgrade() {
                    state.detail_pane.refresh_actions();
                    update_status_bar(&state);
                }
            });
    }
    {
        let weak = Rc::downgrade(state);
        state.pkg_list.connect_force_remove(move |names| {
            if let Some(state) = weak.upgrade() {
                force_remove(&state, names);
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        state.workflows.connect_open_package(move |name| {
            if let Some(state) = weak.upgrade() {
                open_package(&state, name);
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        state.pkg_list.connect_reset_filters(move || {
            if let Some(state) = weak.upgrade() {
                state.search_entry.set_text("");
                state.pkg_list.set_search("");
                state.sidebar.reset_to_all();
                update_status_bar(&state);
            }
        });
    }
    let weak = Rc::downgrade(state);
    state.window.connect_realize(move |window| {
        if let Some(surface) = window.surface() {
            let weak = weak.clone();
            surface.connect_width_notify(move |_| queue_adaptive_layout(weak.clone()));
        }
        queue_adaptive_layout(weak.clone());
    });
    // Explicit names cover controls that become icon-only in narrow windows.
    for (button, label) in [
        (&state.btn_update, "Refresh repositories"),
        (&state.btn_reload, "Reload cached package data"),
        (&state.btn_mark_upgrades, "Mark all updates"),
        (&state.btn_unmark_all, "Clear pending changes"),
        (&state.btn_apply, "Review pending changes"),
    ] {
        button.update_property(&[gtk::accessible::Property::Label(label)]);
    }
    state
        .btn_toggle_detail_pane
        .update_property(&[gtk::accessible::Property::Label("Show package details")]);
    state
        .btn_search_name_only
        .update_property(&[gtk::accessible::Property::Label(
            "Search package names only",
        )]);
    state
        .search_entry
        .update_property(&[gtk::accessible::Property::Label("Search packages")]);
}

// Respond to actual surface resizes instead of keeping the frame clock running
// while idle. Defer reparenting until GTK has finished its current allocation.
fn queue_adaptive_layout(weak: std::rc::Weak<WindowState>) {
    glib::idle_add_local_once(move || {
        if let Some(state) = weak.upgrade() {
            let width = state.window.surface().map_or(0, |surface| surface.width());
            if width > 0 {
                update_adaptive_layout(&state, width);
            }
        }
    });
}

const NARROW_BREAKPOINT: i32 = 1000;

fn update_adaptive_layout(state: &Rc<WindowState>, width: i32) {
    // The desktop header sets the window's minimum width, so the window can
    // never shrink below it; switch just above that wall (theme-dependent).
    if !state.narrow.get() {
        state
            .wide_header_min
            .set(state.header.measure(gtk::Orientation::Horizontal, -1).0);
    }
    // `width` is the surface width, which includes client-side shadows.
    let shadow = state
        .window
        .surface()
        .map_or(0, |surface| (surface.width() - state.window.width()).max(0));
    let narrow = width < NARROW_BREAKPOINT.max(state.wide_header_min.get() + shadow + 24);
    state
        .sidebar
        .place_filters(narrow, &state.filters_button, &state.repositories_button);
    state
        .sidebar
        .widget()
        .set_visible(!narrow && state.btn_toggle_sidebar.is_active());
    state.filter_row.set_visible(narrow);
    state.btn_toggle_sidebar.set_visible(!narrow);
    state.desktop_tools.set_visible(!narrow);
    state
        .search_entry
        .set_width_request(if narrow { 140 } else { 180 });
    state.pkg_list.set_narrow(narrow);
    state.workflows.discovery.set_narrow(narrow);
    if state.narrow.replace(narrow) != narrow {
        let detail = state.detail_pane.widget();
        // The header holds navigation on desktop; narrow windows use a bottom bar.
        if narrow {
            state.header.set_title_widget(Some(&gtk::Label::new(None)));
            state.root_box.append(&state.workflows.navigation);
        } else {
            state.root_box.remove(&state.workflows.navigation);
            state
                .header
                .set_title_widget(Some(&state.workflows.navigation));
        }
        if narrow {
            state.right_paned.set_end_child(None::<&gtk::Widget>);
            state.detail_page.append(detail);
            state
                .workflows
                .stack
                .add_named(&state.detail_page, Some("details"));
        } else {
            if state.workflows.stack.visible_child_name().as_deref() == Some("details") {
                state.workflows.stack.set_visible_child_name("packages");
            }
            state.detail_page.remove(detail);
            state.workflows.stack.remove(&state.detail_page);
            state.right_paned.set_end_child(Some(detail));
            let sidebar = if state.sidebar.widget().is_visible() {
                crate::ui::filter_sidebar::FULL_WIDTH
            } else {
                0
            };
            set_right_paned_position(&state.right_paned, width - sidebar);
        }
    }
}

fn show_pending_changes(state: &Rc<WindowState>) {
    use crate::ui::dialog_util::{modal_window, present_focused};
    let (dialog, content) = modal_window(
        "Pending Changes",
        Some(state.window.upcast_ref()),
        true,
        (600, 460),
        12,
    );
    let note = gtk::Label::new(Some(if cfg!(feature = "apk") {
        "Review installs first, then upgrades, then removals. Remaining choices stay queued for the next review. The next screen includes dependency and version changes."
    } else {
        "Review installs and upgrades first, then removals. Removals with unused dependencies are reviewed last. Remaining choices stay queued. The next screen includes dependency and version changes."
    }));
    note.set_wrap(true);
    note.set_xalign(0.0);
    content.append(&note);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for mark in [
        PkgMark::Install,
        PkgMark::Upgrade,
        PkgMark::Remove,
        PkgMark::Purge,
    ] {
        let mut names = state.store.marked_names(mark);
        names.sort();
        for name in names {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label = gtk::Label::new(Some(&format!(
                "{name} · {}",
                match mark {
                    PkgMark::Install => "Install",
                    PkgMark::Upgrade => "Upgrade",
                    PkgMark::Remove => "Remove",
                    PkgMark::Purge => "Remove with unused dependencies",
                    PkgMark::None => "",
                }
            )));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_wrap(true);
            let remove = gtk::Button::with_label("Unmark");
            remove.update_property(&[gtk::accessible::Property::Label(&format!("Unmark {name}"))]);
            row.append(&label);
            row.append(&remove);
            list.append(&row);
            let weak = Rc::downgrade(state);
            let row_weak = row.downgrade();
            remove.connect_clicked(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.store.set_mark(&name, PkgMark::None);
                    update_status_bar(&state);
                    state.detail_pane.refresh_actions();
                    if let Some(row) = row_weak.upgrade() {
                        row.set_sensitive(false);
                        row.set_visible(false);
                    }
                }
            });
        }
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));
    content.append(&scroll);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Keep editing");
    let review = gtk::Button::with_label("Review transaction");
    review.add_css_class("suggested-action");
    actions.append(&cancel);
    actions.append(&review);
    content.append(&actions);
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let weak = Rc::downgrade(state);
        let dialog = dialog.clone();
        review.connect_clicked(move |_| {
            dialog.destroy();
            if let Some(state) = weak.upgrade() {
                on_apply_clicked(&state);
            }
        });
    }
    present_focused(&dialog, &review);
}

/// Runs inside the existing single-thread GTK smoke test with an unprivileged
/// fixture store. No load or transaction is started for this window.
#[cfg(test)]
pub fn smoke_test_compact_layout() {
    use crate::backend::package::PackageObject;

    fn settle() {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(160);
        while std::time::Instant::now() < deadline {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn capture(window: &gtk::ApplicationWindow, name: &str) {
        let Some(directory) = std::env::var_os("CAERUS_SCREENSHOT_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("create requested screenshot directory");
        let snapshot = gtk::Snapshot::new();
        let paintable = gtk::WidgetPaintable::new(Some(window));
        window.queue_draw();
        settle();
        paintable.snapshot(
            &snapshot,
            f64::from(window.width()),
            f64::from(window.height()),
        );
        let node = snapshot
            .to_node()
            .expect("mapped fixture window has a render node");
        window
            .renderer()
            .expect("mapped window has a renderer")
            .render_texture(&node, None)
            .save_to_png(directory.join(format!("{name}.png")))
            .expect("write fixture screenshot");
    }

    let app = gtk::Application::new(
        Some("org.voidlinux.caerus.CompactTest"),
        gio::ApplicationFlags::NON_UNIQUE,
    );
    app.register(gio::Cancellable::NONE)
        .expect("register fixture application");
    let store = PackageStore::new();
    let packages = [
        (
            "firefox",
            "Mozilla Firefox web browser",
            PkgState::Installed,
        ),
        (
            "firefox-esr",
            "Firefox extended support release",
            PkgState::NotInstalled,
        ),
        (
            "firefox-esr-i18n",
            "Language packs for Firefox ESR",
            PkgState::NotInstalled,
        ),
        (
            "firefox-esr-i18n-de",
            "German language pack",
            PkgState::NotInstalled,
        ),
        (
            "firefox-esr-i18n-fr",
            "French language pack",
            PkgState::NotInstalled,
        ),
        (
            "firefox-i18n",
            "Language packs for Firefox",
            PkgState::NotInstalled,
        ),
        (
            "firefox-i18n-en-GB",
            "British English language pack",
            PkgState::NotInstalled,
        ),
        (
            "fd",
            "A simple, fast alternative to find",
            PkgState::NotInstalled,
        ),
        (
            "ripgrep",
            "Search tool that respects ignore files",
            PkgState::NotInstalled,
        ),
        ("nano", "Small, friendly text editor", PkgState::Installed),
    ]
    .into_iter()
    .map(|(name, description, state)| {
        PackageObject::new(Package {
            name: name.into(),
            short_desc: description.into(),
            state,
            version_available: Some("158.0_1".into()),
            version_installed: (state == PkgState::Installed).then(|| "158.0_1".into()),
            repository: Some("https://repo-default.voidlinux.org/current".into()),
            install_size: 311_741_645,
            download_size: 96_783_565,
            ..Package::default()
        })
    })
    .collect::<Vec<_>>();
    store.list().splice(0, 0, &packages);
    let state = build_window_state(
        &app,
        WindowGeometry {
            width: 1440,
            height: 940,
            ..WindowGeometry::default()
        },
        store.clone(),
        false,
    );
    state.sidebar.set_available_repositories(
        vec!["https://repo-default.voidlinux.org/current".into()],
        &Default::default(),
    );
    state.search_entry.set_text("firefox");
    state.pkg_list.set_search("firefox");
    store.set_mark("ripgrep", PkgMark::Install);
    store.set_mark("fd", PkgMark::Install);
    store.set_mark("nano", PkgMark::Remove);
    update_status_bar(&state);
    state.window.present();
    settle();
    assert!(!state.narrow.get());
    assert!(
        state.sidebar.widget().width() <= 260,
        "sidebar must leave room for content"
    );
    assert!(
        state.detail_pane.widget().get_visible(),
        "the detail pane follows its toggle, as in 0.7"
    );
    assert!(state.pkg_list.select_package_by_name("firefox"));
    settle();
    assert!(state.pending_bar.is_visible());
    assert!(state.btn_apply.is_sensitive());
    assert_eq!(state.pkg_list.visible_counts().0, 7);
    assert_eq!(
        state.detail_pane.widget().parent(),
        Some(state.right_paned.clone().upcast())
    );
    state.search_entry.grab_focus();
    assert!(
        search_has_focus(&state),
        "search delegates keyboard focus to its text child"
    );
    state.pkg_list.assert_compact_layout();
    capture(&state.window, "compact-wide");

    // Desktop filters live in the restored sidebar and remain live after resizing.
    assert!(state.sidebar.widget().is_visible());
    assert_eq!(
        state
            .sidebar
            .section_widget(crate::ui::filter_sidebar::Section::Filters)
            .parent(),
        state
            .sidebar
            .section_widget(crate::ui::filter_sidebar::Section::Maintenance)
            .parent()
    );
    let filters = state.sidebar.preset_list_for_test();
    filters.select_row(filters.row_at_index(1).as_ref());
    assert_eq!(state.pkg_list.visible_counts().0, 1);
    assert_eq!(
        state
            .filters_button
            .child()
            .and_downcast::<gtk::Label>()
            .as_ref()
            .map(gtk::Label::text)
            .as_deref(),
        Some("Installed")
    );
    state.sidebar.reset_to_all();
    assert_eq!(
        state
            .filters_button
            .child()
            .and_downcast::<gtk::Label>()
            .as_ref()
            .map(gtk::Label::text)
            .as_deref(),
        Some("All packages")
    );
    assert_eq!(state.pkg_list.visible_counts().0, 7);

    // A long repository URL must not force a wider window.
    crate::ui::filter_sidebar::set_filter_button_label(
        &state.repositories_button,
        "https://a-repository-with-a-long-hostname.example.org/extra/packages/x86_64/current",
    );
    // Exercise real resize allocation as well as the resulting reparenting.
    // Cross the breakpoint before requesting the final narrow size: native
    // window managers first honor the current header's minimum width.
    state.window.set_default_size(900, 760);
    settle();
    assert!(
        state.window.measure(gtk::Orientation::Horizontal, -1).0 <= 640,
        "the narrow layout must permit a 640px window: header {}",
        state.header.measure(gtk::Orientation::Horizontal, -1).0,
    );
    state.window.set_default_size(640, 760);
    settle();
    assert!(state.narrow.get());
    assert!(state.filter_row.is_visible());
    assert!(!state.sidebar.widget().is_visible());
    assert!(
        state.window.width() <= 660,
        "content should fit narrow width: window {}, header {}, filters {}, navigation {}, table {}",
        state.window.width(), state.header.width(), state.filter_row.width(),
        state.workflows.navigation.width(), state.right_paned.width(),
    );
    crate::ui::filter_sidebar::set_filter_button_label(
        &state.repositories_button,
        "All repositories",
    );
    assert_eq!(
        state.detail_pane.widget().parent(),
        Some(state.detail_page.clone().upcast())
    );
    assert!(state.pkg_list.select_package_by_name("firefox-esr"));
    settle();
    assert_eq!(
        state.workflows.stack.visible_child_name().as_deref(),
        Some("details")
    );
    capture(&state.window, "compact-narrow-details");
    state.workflows.stack.set_visible_child_name("packages");
    settle();
    capture(&state.window, "compact-narrow");
    assert_eq!(
        store.count_marked(),
        3,
        "resizing and selection must preserve the queue"
    );

    state.window.set_default_size(1440, 940);
    settle();
    assert!(!state.narrow.get());
    assert_eq!(
        state.detail_pane.widget().parent(),
        Some(state.right_paned.clone().upcast())
    );
    assert_eq!(store.count_marked(), 3);
    assert!(
        state.detail_pane.widget().width() >= 260,
        "returning to wide layout must restore room for package details: inspector {}, paned {}, position {}",
        state.detail_pane.widget().width(), state.right_paned.width(), state.right_paned.position(),
    );
    assert!(state.pkg_list.select_package_by_name("firefox"));
    #[cfg(feature = "adwaita")]
    let previous_scheme = {
        let manager = adw::StyleManager::default();
        let previous = manager.color_scheme();
        manager.set_color_scheme(adw::ColorScheme::ForceDark);
        previous
    };
    #[cfg(not(feature = "adwaita"))]
    let previous_scheme = {
        let settings = gtk::Settings::default().expect("GTK settings available on test display");
        let previous = settings.is_gtk_application_prefer_dark_theme();
        settings.set_gtk_application_prefer_dark_theme(true);
        previous
    };
    settle();
    capture(&state.window, "compact-dark");
    #[cfg(feature = "adwaita")]
    adw::StyleManager::default().set_color_scheme(previous_scheme);
    #[cfg(not(feature = "adwaita"))]
    gtk::Settings::default()
        .unwrap()
        .set_gtk_application_prefer_dark_theme(previous_scheme);
    // Destroy directly: the test never writes the user's saved geometry.
    state.window.destroy();

    // A saved narrow size must fit immediately, before a desktop allocation.
    let saved_narrow = build_window_state(
        &app,
        WindowGeometry {
            width: 640,
            height: 760,
            ..WindowGeometry::default()
        },
        PackageStore::new(),
        false,
    );
    saved_narrow.window.present();
    settle();
    assert!(saved_narrow.narrow.get());
    assert!(
        saved_narrow.window.width() <= 660,
        "restored narrow windows must fit"
    );
    saved_narrow.window.destroy();

    // Opt in only on a machine with repository metadata: no sync or helper is used.
    if std::env::var_os("CAERUS_TEST_LOCAL_STARTUP").is_some() {
        let store = PackageStore::new();
        let completed = Rc::new(std::cell::Cell::new(None));
        let observed = completed.clone();
        store.connect_load_finished(move |count| observed.set(Some(count)));
        let state = build_window_state(
            &app,
            WindowGeometry {
                width: 1440,
                height: 940,
                ..WindowGeometry::default()
            },
            store.clone(),
            true,
        );
        state.window.present();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while completed.get().is_none() && std::time::Instant::now() < deadline {
            settle();
        }
        let count = completed
            .get()
            .expect("cached repository data should load without a refresh");
        assert!(
            count > 0,
            "local repository metadata is required for this opt-in test"
        );
        assert_eq!(state.pkg_list.visible_counts().0, count);
        assert!(!state.sync_at_launch.get());
        state.pkg_list.assert_cached_rows_survive_refresh();
        capture(&state.window, "cached-startup");
        if state.pkg_list.select_package_by_name("firefox") {
            settle();
            capture(&state.window, "compact-inventory");
        }
        completed.set(None);
        store.load_async();
        assert_eq!(
            state.pkg_list.visible_counts().0,
            count,
            "reload must retain existing packages"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while completed.get().is_none() && std::time::Instant::now() < deadline {
            settle();
        }
        assert_eq!(
            completed.get(),
            Some(count),
            "a local reload should not be needed to discover more packages"
        );
        state.workflows.discover_button.set_active(true);
        settle();
        capture(&state.window, "discover-sections");
        state.workflows.discovery.show_for_test("Graphics", "");
        settle();
        capture(&state.window, "discover-category");
        if state.workflows.discovery.show_for_test("", "gimp") {
            settle();
            capture(&state.window, "discover-app");
        }
        state.window.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_step_flow() -> ApplyFlow {
        ApplyFlow {
            steps: apply_steps(
                vec!["install-me".into()],
                vec![],
                vec!["remove-me".into()],
                vec!["purge-me".into()],
            ),
            completed: 0,
            failed: 0,
            stopped: false,
        }
    }

    #[test]
    fn successful_apply_step_allows_next_group() {
        let mut flow = three_step_flow();
        assert_eq!(flow.next_step().unwrap().command, "INSTALL install-me");
        assert_eq!(
            flow.record_event(ApplyEvent::CommandSucceeded),
            ApplyDecision::Continue
        );
        assert_eq!(flow.next_step().unwrap().command, "REMOVE remove-me");
        assert_eq!(flow.completed, 1);
        assert_eq!(flow.failed, 0);
    }

    #[test]
    fn failed_apply_step_stops_before_remaining_groups() {
        let mut flow = three_step_flow();
        assert_eq!(flow.next_step().unwrap().command, "INSTALL install-me");
        assert_eq!(
            flow.record_event(ApplyEvent::CommandFailed),
            ApplyDecision::Stop
        );
        assert!(flow.next_step().is_none());
        assert_eq!(flow.steps.len(), 2);
        assert_eq!(flow.completed, 0);
        assert_eq!(flow.failed, 1);
    }

    #[test]
    fn cancelled_apply_stops_with_remaining_groups_untouched() {
        let mut flow = three_step_flow();
        assert_eq!(flow.next_step().unwrap().command, "INSTALL install-me");
        assert_eq!(
            flow.record_event(ApplyEvent::ConfirmationCancelled),
            ApplyDecision::Stop
        );
        assert!(flow.next_step().is_none());
        assert_eq!(flow.steps.len(), 2);
        assert_eq!(flow.completed, 0);
        assert_eq!(flow.failed, 0);
    }

    #[test]
    fn apply_groups_match_separate_helper_transactions() {
        let mut steps = apply_steps(
            vec!["new".into()],
            vec!["old".into()],
            vec!["remove-me".into()],
            vec!["purge-me".into()],
        );
        let install = steps.pop_front().unwrap();
        if cfg!(feature = "apk") {
            assert_eq!(install.command, "INSTALL new");
            assert_eq!(install.install_count, 1);
            assert!(
                matches!(install.preview_ops().as_slice(), [PreviewOp::Install(name)] if name == "new")
            );

            let update = steps.pop_front().unwrap();
            assert_eq!(update.command, "UPDATE old");
            assert!(matches!(
                update.preview_ops().as_slice(),
                [PreviewOp::Update(name)] if name == "old"
            ));
        } else {
            assert_eq!(install.command, "INSTALL new old");
            assert_eq!(install.install_count, 1);
            assert!(matches!(install.preview_ops().as_slice(),
                [PreviewOp::Install(a), PreviewOp::Install(b)] if a == "new" && b == "old"));
        }

        let remove = steps.pop_front().unwrap();
        assert_eq!(remove.command, "REMOVE remove-me");
        assert!(
            matches!(remove.preview_ops().as_slice(), [PreviewOp::Remove(name)] if name == "remove-me")
        );

        let purge = steps.pop_front().unwrap();
        assert_eq!(purge.command, "PURGE purge-me");
        assert!(
            matches!(purge.preview_ops().as_slice(), [PreviewOp::Purge(name)] if name == "purge-me")
        );
        assert!(steps.is_empty());
    }

    #[test]
    fn apply_omits_empty_groups() {
        let steps = apply_steps(vec![], vec![], vec!["remove-me".into()], vec![]);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps.front().unwrap().command, "REMOVE remove-me");
    }

    #[test]
    fn held_back_note_lists_and_truncates() {
        assert_eq!(held_back_note(&[]), "");
        assert_eq!(
            held_back_note(&["a".to_string(), "b".to_string()]),
            " 2 held back: a, b."
        );
        let many: Vec<String> = (0..7).map(|i| format!("p{i}")).collect();
        assert_eq!(
            held_back_note(&many),
            " 7 held back: p0, p1, p2, p3, p4, +2 more."
        );
    }
}
