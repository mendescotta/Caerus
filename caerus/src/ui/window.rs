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
use std::cell::RefCell;
use std::rc::Rc;

struct WindowState {
    window: gtk::ApplicationWindow,
    store: PackageStore,
    session: Transaction,
    sidebar: FilterSidebar,
    pkg_list: PackageList,
    detail_pane: DetailPane,
    main_paned: gtk::Paned,

    spinner: gtk::Spinner,
    btn_update: gtk::Button,
    btn_reload: gtk::Button,
    btn_mark_upgrades: gtk::Button,
    btn_unmark_all: gtk::Button,
    btn_apply: gtk::Button,
    apply_count_pill: gtk::Label,
    menu_button: gtk::MenuButton,
    menu_stack: gtk::Stack,
    btn_toggle_sidebar: gtk::Button,
    sw_sidebar_visible: gtk::Switch,
    sw_sidebar_minimal: gtk::Switch,
    sidebar_minimal: std::cell::Cell<bool>,
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
    default_sidebar_pos: std::cell::Cell<i32>,
}

struct WindowGeometry {
    width: i32,
    height: i32,
    sidebar_pos: i32,
    sync_at_launch: bool,
    search_name_only_default: bool,
    section_expanded: [bool; 4],
    section_visible: [bool; 4],
    detail_pane_visible: bool,
    status_bar_visible: bool,
    stale_repos_visible: bool,
    sidebar_visible: bool,
    sidebar_minimal: bool,
    auto_close_on_success: bool,
}

const SECTION_KEYS: [&str; 4] = ["filters", "repositories", "maintenance", "tools"];

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 700,
            sidebar_pos: 200,
            sync_at_launch: false,
            search_name_only_default: false,
            section_expanded: [true; 4],
            section_visible: [true; 4],
            detail_pane_visible: true,
            status_bar_visible: true,
            stale_repos_visible: true,
            sidebar_visible: true,
            sidebar_minimal: false,
            auto_close_on_success: false,
        }
    }
}

const VERTICAL_PANEL_DETAIL_WIDTH: i32 = 380;

fn set_right_paned_position(right_paned: &gtk::Paned, available_width_hint: i32) {
    let avail = if right_paned.width() > 0 {
        right_paned.width()
    } else {
        available_width_hint
    };
    right_paned.set_position((avail - VERTICAL_PANEL_DETAIL_WIDTH).max(200));
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
            if key == "sidebar_minimal" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.sidebar_minimal = b != 0;
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
                        "sidebar" => {
                            geometry.sidebar_visible = b;
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
                "sidebar_pos" => geometry.sidebar_pos = n,
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
            "width={}\nheight={}\nsidebar_pos={}\nsync_at_launch={}\nsearch_name_only_default={}\nsidebar_minimal={}\nauto_close_on_success={}\n",
            self.width,
            self.height,
            self.sidebar_pos,
            i32::from(self.sync_at_launch),
            i32::from(self.search_name_only_default),
            i32::from(self.sidebar_minimal),
            i32::from(self.auto_close_on_success)
        );
        for (i, key) in SECTION_KEYS.iter().enumerate() {
            contents.push_str(&format!(
                "expanded_{key}={}\nvisible_{key}={}\n",
                i32::from(self.section_expanded[i]),
                i32::from(self.section_visible[i])
            ));
        }
        contents.push_str(&format!(
            "visible_detail_pane={}\nvisible_status_bar={}\nvisible_stale_repos={}\nvisible_sidebar={}\n",
            i32::from(self.detail_pane_visible),
            i32::from(self.status_bar_visible),
            i32::from(self.stale_repos_visible),
            i32::from(self.sidebar_visible)
        ));
        let _ = std::fs::write(&path, contents);
    }
}

pub fn build_window(app: &gtk::Application) -> gtk::ApplicationWindow {
    let geometry = WindowGeometry::load();

    let window = gtk::ApplicationWindow::new(app);
    window.set_title(Some("Caerus"));
    window.set_default_size(geometry.width, geometry.height);

    install_css(&window);
    ensure_icon_theme_fallback(&window);

    let header = gtk::HeaderBar::new();
    let title_label = gtk::Label::new(Some("Caerus"));
    title_label.add_css_class("title");
    header.set_title_widget(Some(&title_label));

    let btn_toggle_sidebar = gtk::Button::new();
    btn_toggle_sidebar.set_icon_name("sidebar-show-symbolic");
    header.pack_start(&btn_toggle_sidebar);

    let spinner = gtk::Spinner::new();
    let btn_update = gtk::Button::from_icon_name("software-update-available-symbolic");
    btn_update.set_tooltip_text(Some("Sync repositories and reload package list"));
    let btn_reload = gtk::Button::from_icon_name("view-refresh-symbolic");
    btn_reload.set_tooltip_text(Some("Reload local package list without syncing"));
    let btn_mark_upgrades = gtk::Button::from_icon_name("software-update-urgent-symbolic");
    btn_mark_upgrades.set_tooltip_text(Some("Mark All Updates"));
    let btn_unmark_all = gtk::Button::from_icon_name("edit-clear-all-symbolic");
    btn_unmark_all.set_sensitive(false);
    btn_unmark_all.set_tooltip_text(Some("Unmark All"));
    let mark_state_group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    mark_state_group.add_css_class("linked");
    mark_state_group.append(&btn_mark_upgrades);
    mark_state_group.append(&btn_unmark_all);

    header.pack_start(&spinner);
    header.pack_start(&btn_update);
    header.pack_start(&btn_reload);
    header.pack_start(&mark_state_group);

    let btn_apply = gtk::Button::new();
    btn_apply.set_sensitive(false);
    btn_apply.add_css_class("suggested-action");
    btn_apply.set_tooltip_text(Some("Apply"));
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
    search_entry.set_width_request(220);
    search_entry.set_placeholder_text(Some("Search packages\u{2026}"));

    let btn_toggle_detail_pane = gtk::ToggleButton::new();
    btn_toggle_detail_pane.set_icon_name("sidebar-show-right-symbolic");
    btn_toggle_detail_pane.set_active(geometry.detail_pane_visible);
    btn_toggle_detail_pane.set_tooltip_text(Some("Show/hide the detail panel"));
    header.pack_end(&btn_toggle_detail_pane);

    header.pack_end(&search_entry);
    header.pack_end(&btn_search_name_only);
    header.pack_end(&btn_apply);

    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Menu"));
    let menu_stack = gtk::Stack::new();
    header.pack_end(&menu_button);

    window.set_titlebar(Some(&header));

    let store = PackageStore::new();
    let session = Transaction::new();

    let sidebar = FilterSidebar::new();
    let pkg_list = PackageList::new(store.clone());
    let detail_pane = DetailPane::new(store.clone());
    {
        let detail_pane_widget = detail_pane.widget().clone();
        btn_toggle_detail_pane.connect_toggled(move |btn| {
            detail_pane_widget.set_visible(btn.is_active());
        });
    }

    let right_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    right_paned.set_resize_start_child(true);
    right_paned.set_shrink_start_child(false);
    right_paned.set_resize_end_child(false);
    right_paned.set_shrink_end_child(false);
    right_paned.set_start_child(Some(pkg_list.widget()));
    right_paned.set_end_child(Some(detail_pane.widget()));
    set_right_paned_position(&right_paned, geometry.width - geometry.sidebar_pos);

    let main_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    main_paned.set_position(geometry.sidebar_pos);
    main_paned.set_vexpand(true);
    main_paned.set_resize_start_child(false);
    main_paned.set_shrink_start_child(false);
    main_paned.set_resize_end_child(true);
    main_paned.set_start_child(Some(sidebar.widget()));
    main_paned.set_end_child(Some(&right_paned));

    let status_label = gtk::Label::new(Some("Loading\u{2026}"));
    status_label.set_xalign(0.0);
    status_label.set_margin_start(8);
    status_label.set_margin_top(3);
    status_label.set_margin_bottom(3);
    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    status_bar.add_css_class("statusbar");
    status_bar.append(&status_label);

    let root_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root_box.append(&main_paned);
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

    let sw_sidebar_visible = gtk::Switch::new();
    let sw_sidebar_minimal = gtk::Switch::new();

    let state = Rc::new(WindowState {
        window: window.clone(),
        store,
        session,
        sidebar,
        pkg_list,
        detail_pane,
        main_paned,
        spinner,
        btn_update,
        btn_reload,
        btn_mark_upgrades,
        btn_unmark_all,
        btn_apply,
        apply_count_pill,
        menu_button,
        menu_stack,
        btn_toggle_sidebar: btn_toggle_sidebar.clone(),
        sw_sidebar_visible: sw_sidebar_visible.clone(),
        sw_sidebar_minimal: sw_sidebar_minimal.clone(),
        sidebar_minimal: std::cell::Cell::new(false),
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
        default_sidebar_pos: std::cell::Cell::new(geometry.sidebar_pos),
    });

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
            .set_visible(geometry.section_visible[i]);
    }
    state
        .detail_pane
        .widget()
        .set_visible(geometry.detail_pane_visible);
    state.status_bar.set_visible(geometry.status_bar_visible);
    state
        .sidebar
        .set_show_stale_repositories(geometry.stale_repos_visible);
    apply_sidebar_mode(&state, geometry.sidebar_visible, geometry.sidebar_minimal);
    crate::ui::apply_dialog::set_auto_close_on_success(geometry.auto_close_on_success);

    populate_menu_popover(&state);

    state
        .btn_search_name_only
        .set_active(geometry.search_name_only_default);

    trigger_update(&state, geometry.sync_at_launch, true);

    window
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

fn apply_sidebar_mode(state: &Rc<WindowState>, visible: bool, minimal: bool) {
    let was_minimal = state.sidebar_minimal.get();
    if minimal && !was_minimal {
        state.default_sidebar_pos.set(state.main_paned.position());
    }

    state.sidebar.widget().set_visible(visible);
    state.sidebar.set_minimal(minimal);
    state.sidebar_minimal.set(minimal);

    if minimal {
        state
            .main_paned
            .set_position(crate::ui::filter_sidebar::RAIL_WIDTH);
    } else if was_minimal {
        state
            .main_paned
            .set_position(state.default_sidebar_pos.get());
    }

    state
        .btn_toggle_sidebar
        .set_tooltip_text(Some(match (visible, minimal) {
            (true, false) => "Show Minimal Sidebar (F9)",
            (true, true) => "Hide Sidebar (F9)",
            (false, _) => "Show Sidebar (F9)",
        }));
    state.sw_sidebar_visible.set_active(visible);
    state.sw_sidebar_minimal.set_active(minimal);
}

fn cycle_sidebar_mode(state: &Rc<WindowState>) {
    let visible = state.sidebar.widget().get_visible();
    let minimal = state.sidebar_minimal.get();
    let (next_visible, next_minimal) = if !visible {
        (true, false)
    } else if !minimal {
        (true, true)
    } else {
        (false, minimal)
    };
    apply_sidebar_mode(state, next_visible, next_minimal);
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

    let sidebar_row = switch_row_with(&state.sw_sidebar_visible, "Sidebar", Some("F9"));
    {
        let sw_sidebar_visible = state.sw_sidebar_visible.clone();
        let state = state.clone();
        sw_sidebar_visible.connect_active_notify(move |sw| {
            apply_sidebar_mode(&state, sw.is_active(), state.sidebar_minimal.get());
        });
    }
    view.append(&sidebar_row);

    let minimal_row = switch_row_with(&state.sw_sidebar_minimal, "Minimal Sidebar", None);
    {
        let sw_sidebar_minimal = state.sw_sidebar_minimal.clone();
        let state = state.clone();
        sw_sidebar_minimal.connect_active_notify(move |sw| {
            let minimal = sw.is_active();
            let visible = state.sidebar.widget().get_visible() || minimal;
            apply_sidebar_mode(&state, visible, minimal);
        });
    }
    view.append(&minimal_row);
    view.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    for section in crate::ui::filter_sidebar::Section::ALL {
        let (row, sw) = switch_row(section.label(), None);
        state
            .sidebar
            .section_widget(section)
            .bind_property("visible", &sw, "active")
            .bidirectional()
            .sync_create()
            .build();
        view.append(&row);
    }

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
    stack.add_named(&view, Some("view"));

    let settings = gtk::Box::new(gtk::Orientation::Vertical, 2);
    settings.set_width_request(290);
    settings.append(&menu_page_header(stack, "Settings"));

    let (sync_row, sw_sync) = switch_row("Sync repositories at launch", None);
    sync_row.set_tooltip_text(Some(
        "When enabled, Caerus syncs repository indexes (a privileged action, prompting for \
         your password) automatically every time it starts. Disable this to skip that prompt \
         at launch — you can still sync manually any time via the header bar's sync button.",
    ));
    sw_sync.set_active(state.sync_at_launch.get());
    {
        let state = state.clone();
        sw_sync.connect_active_notify(move |sw| state.sync_at_launch.set(sw.is_active()));
    }
    settings.append(&sync_row);

    let (search_row, sw_search) = switch_row("Search names only by default", None);
    search_row.set_tooltip_text(Some(
        "Controls what the header bar's name-only search toggle starts as the next time \
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
        ("Toggle Sidebar", "F9"),
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
        .comments("A Synaptic-inspired package manager for Void Linux, built directly on libxbps.")
        .website("https://github.com/mendescotta/Caerus")
        .application_icon(crate::APP_ID)
        .license_type(gtk::License::Gpl30)
        .build();
    about.present();
    gtk::prelude::GtkWindowExt::set_focus(&about, None::<&gtk::Widget>);
}

#[cfg(not(feature = "adwaita"))]
fn show_about_dialog(parent: &gtk::ApplicationWindow) {
    let about = gtk::AboutDialog::new();
    about.set_transient_for(Some(parent));
    about.set_modal(true);
    about.set_program_name(Some("Caerus"));
    about.set_version(Some(env!("CARGO_PKG_VERSION")));
    about.set_comments(Some(
        "A Synaptic-inspired package manager for Void Linux, built directly on libxbps.",
    ));
    about.set_website(Some("https://github.com/mendescotta/Caerus"));
    about.set_logo_icon_name(Some(crate::APP_ID));
    about.set_license_type(gtk::License::Gpl30);
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
        ("Ctrl+F", "Focus search"),
        ("Escape", "Clear search, or close the current dialog"),
        ("F5", "Reload package list"),
        ("F9", "Toggle sidebar"),
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

fn wire_keyboard_shortcuts(state: &Rc<WindowState>) {
    let controller = gtk::EventControllerKey::new();
    let window = state.window.clone();
    let state = state.clone();
    controller.connect_key_pressed(move |_, key, _keycode, modifiers| {
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        match key {
            gtk::gdk::Key::f if ctrl => {
                state.search_entry.grab_focus();
                glib::Propagation::Stop
            }
            gtk::gdk::Key::q if ctrl => {
                state.window.close();
                glib::Propagation::Stop
            }
            gtk::gdk::Key::a if ctrl && !state.search_entry.has_focus() => {
                state.pkg_list.select_all();
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Escape if !state.search_entry.text().is_empty() => {
                state.search_entry.set_text("");
                glib::Propagation::Stop
            }
            gtk::gdk::Key::F5 => {
                trigger_update(&state, false, false);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::F9 => {
                cycle_sidebar_mode(&state);
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
            gtk::gdk::Key::Delete if !state.search_entry.has_focus() => {
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
        store.connect_load_finished(move |_n| {
            set_loading(&state, false);
            update_status_bar(&state);
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
            show_toast(&state, &format!("Error loading packages: {msg}"));
        });
    }

    {
        let sidebar = state.sidebar.clone();
        let state = state.clone();
        sidebar.connect_filter_changed(move |mode| {
            state.pkg_list.set_filter(mode);
            update_status_bar(&state);
        });
    }
    {
        let sidebar = state.sidebar.clone();
        let state = state.clone();
        sidebar.connect_repository_changed(move |repo| {
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
        use crate::ui::filter_sidebar::SidebarAction;
        let state = state.clone();
        state
            .clone()
            .sidebar
            .connect_action(move |action| match action {
                SidebarAction::FullUpgrade => on_full_upgrade_clicked(&state),
                SidebarAction::RemoveOrphans => on_remove_orphans_clicked(&state),
                SidebarAction::CleanCache => {
                    run_maintenance_command(&state, "CLEANCACHE", "Cleaning Package Cache");
                }
                SidebarAction::VerifyDb => {
                    run_maintenance_command(&state, "VERIFY", "Verifying Package Database");
                }
                SidebarAction::Reconfigure => on_reconfigure_all_clicked(&state),
                SidebarAction::PurgeKernels => {
                    crate::ui::vkpurge_dialog::show(
                        Some(state.window.upcast_ref()),
                        &state.session,
                    );
                }
                SidebarAction::FindOwner => {
                    crate::ui::file_owner_dialog::show(Some(state.window.upcast_ref()));
                }
                SidebarAction::Alternatives => {
                    crate::ui::alternatives_dialog::show(
                        Some(state.window.upcast_ref()),
                        &state.session,
                    );
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
            });
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
        let btn_toggle_sidebar = state.btn_toggle_sidebar.clone();
        let state = state.clone();
        btn_toggle_sidebar.connect_clicked(move |_| cycle_sidebar_mode(&state));
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
            on_apply_clicked(&state);
        });
    }
    {
        let search_entry = state.search_entry.clone();
        let state = state.clone();
        search_entry.connect_search_changed(move |e| {
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
            WindowGeometry {
                width: win.width(),
                height: win.height(),
                sidebar_pos: if state.sidebar_minimal.get() {
                    state.default_sidebar_pos.get()
                } else {
                    state.main_paned.position()
                },
                sync_at_launch: state.sync_at_launch.get(),
                search_name_only_default: state.search_name_only_default.get(),
                section_expanded: Section::ALL.map(|s| state.sidebar.is_expanded(s)),
                section_visible: Section::ALL
                    .map(|s| state.sidebar.section_widget(s).get_visible()),
                detail_pane_visible: state.btn_toggle_detail_pane.is_active(),
                status_bar_visible: state.status_bar.get_visible(),
                stale_repos_visible: state.sidebar.show_stale_repositories(),
                sidebar_visible: state.sidebar.widget().get_visible(),
                sidebar_minimal: state.sidebar_minimal.get(),
                auto_close_on_success: state.auto_close_on_success.get(),
            }
            .save();
            state.session.shutdown();
            glib::Propagation::Proceed
        });
    }
}

fn set_loading(state: &Rc<WindowState>, loading: bool) {
    if loading {
        state.spinner.start();
        state.btn_update.set_sensitive(false);
        state.btn_reload.set_sensitive(false);
        state.menu_button.set_sensitive(false);
    } else {
        state.spinner.stop();
        state.btn_update.set_sensitive(true);
        state.btn_reload.set_sensitive(true);
        state.menu_button.set_sensitive(true);
    }
}

fn do_reload(state: &Rc<WindowState>) {
    state.detail_pane.show_package(None);
    *state.selected_pkg.borrow_mut() = None;
    state.store.load_async();
}

fn trigger_update(state: &Rc<WindowState>, sync_first: bool, silent: bool) {
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

fn on_apply_clicked(state: &Rc<WindowState>) {
    let installs = state.store.marked_names(PkgMark::Install);
    let upgrades = state.store.marked_names(PkgMark::Upgrade);
    let removes = state.store.marked_names(PkgMark::Remove);
    let purges = state.store.marked_names(PkgMark::Purge);

    let mut commands = Vec::new();
    if !installs.is_empty() || !upgrades.is_empty() {
        let mut cmd = String::from("INSTALL");
        for n in installs.iter().chain(upgrades.iter()) {
            cmd.push(' ');
            cmd.push_str(n);
        }
        commands.push(cmd);
    }
    if !removes.is_empty() {
        let mut cmd = String::from("REMOVE");
        for n in &removes {
            cmd.push(' ');
            cmd.push_str(n);
        }
        commands.push(cmd);
    }
    if !purges.is_empty() {
        let mut cmd = String::from("PURGE");
        for n in &purges {
            cmd.push(' ');
            cmd.push_str(n);
        }
        commands.push(cmd);
    }

    if commands.is_empty() {
        return;
    }

    let ops: Vec<PreviewOp> = installs
        .iter()
        .map(|n| PreviewOp::Install(n.clone()))
        .chain(upgrades.iter().map(|n| PreviewOp::Update(n.clone())))
        .chain(removes.iter().map(|n| PreviewOp::Remove(n.clone())))
        .chain(purges.iter().map(|n| PreviewOp::Purge(n.clone())))
        .collect();

    let state2 = state.clone();
    state.store.preview_transaction_async(ops, move |preview| {
        let state = state2;
        let state2 = state.clone();
        apply_confirm::confirm(
            Some(state.window.upcast_ref()),
            &installs,
            &upgrades,
            &removes,
            &purges,
            preview,
            move |confirmed| {
                if !confirmed {
                    return;
                }
                let state3 = state2.clone();
                let commands_for_retry = commands.clone();
                apply_dialog::run_recorded(
                    Some(state2.window.upcast_ref()),
                    &state2.session,
                    &commands,
                    "Applying Changes",
                    move |success| {
                        if success {
                            show_toast(&state3, "Changes applied. Reloading\u{2026}");
                            state3.store.clear_all_marks();
                            do_reload(&state3);
                        } else {
                            show_toast(&state3, "Some changes failed — see log.");
                            offer_force_retry(&state3, commands_for_retry.clone());
                        }
                    },
                );
            },
        );
    });
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

fn on_full_upgrade_clicked(state: &Rc<WindowState>) {
    let upgrades = state.store.upgradable_names();
    if upgrades.is_empty() {
        state
            .status_label
            .set_text("Everything is already up to date.");
        return;
    }
    let ops: Vec<PreviewOp> = upgrades
        .iter()
        .map(|n| PreviewOp::Update(n.clone()))
        .collect();

    let state2 = state.clone();
    state.store.preview_transaction_async(ops, move |preview| {
        let state = state2;
        let state2 = state.clone();
        apply_confirm::confirm(
            Some(state.window.upcast_ref()),
            &[],
            &upgrades,
            &[],
            &[],
            preview,
            move |confirmed| {
                if confirmed {
                    run_maintenance_command(&state2, "UPGRADE", "Full System Upgrade");
                }
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

fn force_variant(cmd: &str) -> String {
    for verb in ["INSTALL", "REMOVE", "PURGE"] {
        if let Some(rest) = cmd.strip_prefix(verb) {
            return format!("{verb}_FORCE{rest}");
        }
    }
    cmd.to_string()
}

fn offer_force_retry(state: &Rc<WindowState>, commands: Vec<String>) {
    let (dlg, outer) = crate::ui::dialog_util::modal_window(
        "Retry With Force?",
        Some(state.window.upcast_ref()),
        false,
        (440, -1),
        10,
    );

    let heading = gtk::Label::new(Some(
        "Some changes failed, possibly due to file conflicts or unresolved \
         dependencies. Forcing through these checks can leave the system in \
         an inconsistent state — only do this if you understand why the \
         normal attempt failed.",
    ));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let (btn_box, cancel_btn) = crate::ui::dialog_util::cancel_button_row(4);
    let retry_btn = gtk::Button::with_label("Retry With Force");
    retry_btn.add_css_class("destructive-action");
    btn_box.append(&retry_btn);
    outer.append(&btn_box);
    dlg.set_default_widget(Some(&cancel_btn));

    let give_up = {
        let state = state.clone();
        move || {
            state.store.clear_all_marks();
            do_reload(&state);
        }
    };

    {
        let dlg = dlg.clone();
        let give_up = give_up.clone();
        cancel_btn.connect_clicked(move |_| {
            give_up();
            dlg.destroy();
        });
    }
    {
        let state = state.clone();
        let dlg = dlg.clone();
        retry_btn.connect_clicked(move |_| {
            dlg.destroy();
            let forced: Vec<String> = commands.iter().map(|c| force_variant(c)).collect();
            let state2 = state.clone();
            apply_dialog::run_recorded(
                Some(state.window.upcast_ref()),
                &state.session,
                &forced,
                "Retrying With Force",
                move |success| {
                    show_toast(
                        &state2,
                        if success {
                            "Changes applied. Reloading\u{2026}"
                        } else {
                            "Force retry also failed — see log. Reloading\u{2026}"
                        },
                    );
                    state2.store.clear_all_marks();
                    do_reload(&state2);
                },
            );
        });
    }
    {
        dlg.connect_close_request(move |_| {
            give_up();
            glib::Propagation::Proceed
        });
    }

    crate::ui::dialog_util::present_focused(&dlg, &cancel_btn);
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
    let upgradable = state.store.count_upgradable();
    let marked = state.store.count_marked();

    if state.pkg_list.has_active_filters() {
        let (total, installed, not_installed) = state.pkg_list.visible_counts();
        state.status_label.set_text(&format!(
            "{total} shown — {installed} installed, {not_installed} not installed.  {marked} marked."
        ));
    } else {
        let total = state.store.list().n_items();
        let installed = state.store.count_installed();
        state.status_label.set_text(&format!(
            "{total} packages.  {installed} installed.  {upgradable} upgradable.  {marked} marked."
        ));
    }
    update_apply_button(state, marked);
    update_mark_upgrades_button(state, upgradable);
}

fn update_apply_button(state: &Rc<WindowState>, marked: u32) {
    crate::ui::dialog_util::set_count(
        &state.apply_count_pill,
        (marked > 0).then_some(marked as usize),
    );
    state.btn_apply.set_sensitive(marked > 0);
    state.btn_unmark_all.set_sensitive(marked > 0);
}

fn update_mark_upgrades_button(state: &Rc<WindowState>, upgradable: u32) {
    state.btn_mark_upgrades.set_sensitive(upgradable > 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_variant_adds_suffix_to_install_remove_purge() {
        assert_eq!(force_variant("INSTALL foo bar"), "INSTALL_FORCE foo bar");
        assert_eq!(force_variant("REMOVE foo"), "REMOVE_FORCE foo");
        assert_eq!(
            force_variant("PURGE foo bar baz"),
            "PURGE_FORCE foo bar baz"
        );
    }

    #[test]
    fn force_variant_leaves_commands_without_a_force_verb_unchanged() {
        assert_eq!(force_variant("UPGRADE"), "UPGRADE");
        assert_eq!(force_variant("HOLD foo"), "HOLD foo");
        assert_eq!(force_variant("SYNC"), "SYNC");
        assert_eq!(force_variant("RECONFIGURE_ALL"), "RECONFIGURE_ALL");
    }
}
