//! Main application window. Rust translation of ui/window.{h,c} (built
//! directly in code here rather than from a `GtkBuilder` .ui file).

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
    right_paned: gtk::Paned,

    spinner: gtk::Spinner,
    btn_update: gtk::Button,
    btn_reload: gtk::Button,
    btn_mark_upgrades: gtk::Button,
    btn_unmark_all: gtk::Button,
    btn_apply: gtk::Button,
    /// The "N" badge inside `btn_apply`, updated by `update_apply_button`.
    apply_count_pill: gtk::Label,
    menu_button: gtk::MenuButton,
    /// The hamburger popover's page stack (root / view / settings /
    /// shortcuts), populated by `populate_menu_popover`.
    menu_stack: gtk::Stack,
    btn_toggle_sidebar: gtk::Button,
    /// Independent of `btn_toggle_sidebar`'s click-cycle — the View
    /// menu's "Sidebar" and "Minimal Sidebar" switches, kept in sync
    /// with the button and each other via `apply_sidebar_mode`.
    sw_sidebar_visible: gtk::Switch,
    sw_sidebar_minimal: gtk::Switch,
    /// Current rail-mode flag, kept even while the sidebar is hidden —
    /// mirrors `WindowGeometry::sidebar_minimal`. Visibility itself is
    /// read directly from `sidebar.widget().get_visible()`, matching
    /// the existing pattern for `detail_pane_visible`.
    sidebar_minimal: std::cell::Cell<bool>,
    /// Right-side counterpart to `btn_toggle_sidebar`: show/hide the
    /// detail pane. Bound bidirectionally to the View menu's "Detail
    /// Pane" switch.
    btn_toggle_detail_pane: gtk::ToggleButton,
    status_bar: gtk::Box,
    search_entry: gtk::SearchEntry,
    btn_search_name_only: gtk::ToggleButton,
    status_label: gtk::Label,

    /// Wraps the window content so transient notifications show as a
    /// toast instead of overwriting `status_label`. `--features adwaita`
    /// only; `show_toast` falls back to `status_label` otherwise.
    #[cfg(feature = "adwaita")]
    toast_overlay: adw::ToastOverlay,

    /// Mirrors the package list's current selection, for the Delete-key
    /// shortcut.
    selected_pkg: RefCell<Option<Package>>,

    /// Whether to sync repositories at launch — see `WindowGeometry`.
    sync_at_launch: std::cell::Cell<bool>,

    /// Whether "search by name only" starts active at next launch — see
    /// `WindowGeometry`.
    search_name_only_default: std::cell::Cell<bool>,
    auto_close_on_success: std::cell::Cell<bool>,
    /// `right_paned`'s divider position for bottom-dock mode (the pkg
    /// list's height) — kept separately from the live `right_paned`
    /// position because that's overwritten with a *width* while in
    /// right-dock mode; see `apply_panel_orientation`.
    default_detail_pos: std::cell::Cell<i32>,
    /// `main_paned`'s divider position for full (non-rail) sidebar mode —
    /// kept separately from the live `main_paned` position because that's
    /// overwritten with the rail width while minimal; see
    /// `apply_sidebar_mode`. Same technique as `default_detail_pos`.
    default_sidebar_pos: std::cell::Cell<i32>,
}

/// Window size + paned-divider positions, persisted across launches.
/// Hand-rolled `key=value` file rather than pulling in a serialization
/// crate for a handful of fields.
struct WindowGeometry {
    width: i32,
    height: i32,
    sidebar_pos: i32,
    detail_pos: i32,
    /// Whether to sync repositories (a privileged `pkexec` action) at
    /// launch. Defaults to `false` so a fresh install doesn't prompt for
    /// auth before the user has seen a package.
    sync_at_launch: bool,
    /// Whether the header's "search by name only" toggle starts active.
    search_name_only_default: bool,
    /// Collapsed/expanded state of the four sidebar sections, in
    /// `Section::ALL` order.
    section_expanded: [bool; 4],
    /// Shown/hidden state of the four sidebar sections, in `Section::ALL`
    /// order.
    section_visible: [bool; 4],
    detail_pane_visible: bool,
    /// `false` (default) = detail pane docked below the package list,
    /// full width, cards in a 2-row grid. `true` = docked to the right as
    /// a narrow column, cards in a single-column stack. Drives both
    /// `right_paned`'s orientation and `DetailPane::set_horizontal`.
    vertical_panel: bool,
    status_bar_visible: bool,
    /// Whether the sidebar shows repositories no longer configured in
    /// xbps.d.
    stale_repos_visible: bool,
    /// Whether the sidebar is shown at all — previously not persisted
    /// (always started shown); now tracked like `detail_pane_visible`.
    sidebar_visible: bool,
    /// Whether the (visible) sidebar renders as the narrow icon rail
    /// instead of the full labeled layout. Kept even while the sidebar
    /// is hidden, so re-showing it via the View menu's "Sidebar" switch
    /// resumes whichever mode was last active.
    sidebar_minimal: bool,
    auto_close_on_success: bool,
}

/// Persistence keys for the per-section booleans, in `Section::ALL`
/// order (must stay in sync with it).
const SECTION_KEYS: [&str; 4] = ["filters", "repositories", "maintenance", "tools"];

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 700,
            sidebar_pos: 200,
            detail_pos: 420,
            sync_at_launch: false,
            search_name_only_default: false,
            section_expanded: [true; 4],
            section_visible: [true; 4],
            detail_pane_visible: true,
            vertical_panel: false,
            status_bar_visible: true,
            stale_repos_visible: true,
            sidebar_visible: true,
            sidebar_minimal: false,
            auto_close_on_success: false,
        }
    }
}

/// Target width of the detail pane when docked to the right (vertical
/// panel mode) — a narrow column, not a 50/50 split.
const VERTICAL_PANEL_DETAIL_WIDTH: i32 = 380;

/// Applies the panel-dock orientation to `right_paned` and matches
/// `detail_pane`'s card layout to it. `right_paned`'s divider position is
/// the *start* child's (`pkg_list`'s) size along the active axis — reusing
/// `detail_pos` (a saved top-pane *height* from bottom-dock mode) as a
/// left-pane *width* in right-dock mode would squeeze the package list to
/// whatever arbitrary pixel value was last saved for a completely
/// different axis, potentially down to a sliver. Right-dock mode instead
/// derives the position from the pane's actual available width, so the
/// package list always gets the lion's share and the detail pane stays a
/// fixed `VERTICAL_PANEL_DETAIL_WIDTH`-wide column — the same
/// doesn't-cover-the-list behavior as the Filters sidebar.
/// `available_width_hint` is used only when `right_paned` isn't realized
/// yet (its `.width()` reads 0 before the window is first shown, e.g. at
/// startup) — pass the best known estimate of the pane's eventual width.
fn apply_panel_orientation(
    right_paned: &gtk::Paned,
    detail_pane: &DetailPane,
    vertical: bool,
    detail_pos: i32,
    available_width_hint: i32,
) {
    right_paned.set_orientation(if vertical {
        gtk::Orientation::Horizontal
    } else {
        gtk::Orientation::Vertical
    });
    if vertical {
        let avail = if right_paned.width() > 0 {
            right_paned.width()
        } else {
            available_width_hint
        };
        right_paned.set_position((avail - VERTICAL_PANEL_DETAIL_WIDTH).max(200));
    } else {
        right_paned.set_position(detail_pos);
    }
    detail_pane.set_horizontal(!vertical);
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
            if key == "vertical_panel" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.vertical_panel = b != 0;
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
                "detail_pos" => geometry.detail_pos = n,
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
            "width={}\nheight={}\nsidebar_pos={}\ndetail_pos={}\nsync_at_launch={}\nsearch_name_only_default={}\nvertical_panel={}\nsidebar_minimal={}\nauto_close_on_success={}\n",
            self.width,
            self.height,
            self.sidebar_pos,
            self.detail_pos,
            i32::from(self.sync_at_launch),
            i32::from(self.search_name_only_default),
            i32::from(self.vertical_panel),
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

    // ── Header bar ──
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
    let btn_mark_upgrades = gtk::Button::with_label("Mark All Upgrades");
    btn_mark_upgrades.add_css_class("flat");
    if let Some(l) = btn_mark_upgrades.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    btn_mark_upgrades.set_tooltip_text(Some(
        "Queue every upgradable package as a pending mark, reviewed and applied via Apply \
         — unlike the app menu's Full System Upgrade, this can be combined with other \
         pending install/remove marks and reviewed before anything runs.",
    ));
    let btn_unmark_all = gtk::Button::with_label("Unmark All");
    btn_unmark_all.add_css_class("flat");
    if let Some(l) = btn_unmark_all.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    btn_unmark_all.set_sensitive(false);
    btn_unmark_all.set_tooltip_text(Some(
        "Clear every pending Install/Upgrade/Remove/Purge mark",
    ));

    header.pack_start(&spinner);
    header.pack_start(&btn_update);
    header.pack_start(&btn_reload);

    let btn_apply = gtk::Button::new();
    btn_apply.set_sensitive(false);
    btn_apply.add_css_class("suggested-action");
    let apply_btn_content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    apply_btn_content.append(&gtk::Label::new(Some("Apply")));
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

    // Right-side counterpart to `btn_toggle_sidebar`: show/hide the
    // detail pane. Packed first among the `pack_end` widgets so it lands
    // at the outermost right edge, past the search bar.
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
    menu_button.set_tooltip_text(Some("Main Menu"));
    let menu_stack = gtk::Stack::new();
    header.pack_end(&menu_button);

    window.set_titlebar(Some(&header));

    // ── Backend ──
    let store = PackageStore::new();
    let session = Transaction::new();

    // ── Body ──
    let sidebar = FilterSidebar::new();
    let pkg_list = PackageList::new(store.clone());
    let detail_pane = DetailPane::new(store.clone());
    {
        let detail_pane_widget = detail_pane.widget().clone();
        btn_toggle_detail_pane.connect_toggled(move |btn| {
            detail_pane_widget.set_visible(btn.is_active());
        });
    }

    // `right_paned`'s orientation is the panel-dock switch: `Vertical`
    // stacks pkg_list/detail_pane top/bottom (default, detail pane docked
    // below, full width); `Horizontal` puts them side by side (detail
    // pane docked to the right, narrow column, like the Filters sidebar —
    // it doesn't cover the package list). Same start/end children either
    // way — only the axis flips.
    let right_paned = gtk::Paned::new(gtk::Orientation::Vertical);
    right_paned.set_resize_start_child(true);
    right_paned.set_shrink_start_child(false);
    right_paned.set_resize_end_child(false);
    right_paned.set_shrink_end_child(false);
    right_paned.set_start_child(Some(pkg_list.widget()));
    right_paned.set_end_child(Some(detail_pane.widget()));
    apply_panel_orientation(
        &right_paned,
        &detail_pane,
        geometry.vertical_panel,
        geometry.detail_pos,
        geometry.width - geometry.sidebar_pos,
    );

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
        right_paned,
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
        // Seeded `false` regardless of `geometry.sidebar_minimal` so the
        // explicit `apply_sidebar_mode` call below (which applies the
        // loaded value) sees it as a real Full->Minimal transition and
        // captures `default_sidebar_pos` correctly.
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
        default_detail_pos: std::cell::Cell::new(geometry.detail_pos),
        default_sidebar_pos: std::cell::Cell::new(geometry.sidebar_pos),
    });

    wire_up(&state);
    wire_keyboard_shortcuts(&state);

    // Must restore section collapse/visibility before building the menu
    // popover — its switches bind to live `visible` with sync_create.
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

    // Must run after wire_up so the toggled handler is already connected.
    state
        .btn_search_name_only
        .set_active(geometry.search_name_only_default);

    // Sync repos at launch silently (no dialog), then reload — unless
    // opted out, in which case this is a plain local reload.
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

/// Every symbolic icon name used anywhere in the app, checked at startup
/// by `ensure_icon_theme_fallback`.
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

/// GTK resolves icon names against only the active theme plus "hicolor"
/// fallback, never Adwaita as a second fallback — so on a non-GNOME
/// desktop some symbolic names render blank even with adwaita-icon-theme
/// installed. Fixed by bundling copies under `data/icons/hicolor/scalable/`
/// (hicolor's `index.theme` only declares `symbolic/apps`, never
/// `symbolic/<other-context>`, so `scalable/` is required even though
/// every filename ends "-symbolic" — GTK still recolors them correctly
/// there). `install.sh` registers the real system path for an installed
/// build; for a bare `cargo run` this also registers the checkout's own
/// `caerus/data/icons` directly.
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

/// Directory containing a `hicolor/` tree with this app's bundled
/// fallback icons, or `None` if not found (e.g. an installed build where
/// `install.sh` already placed them at the real system icon path).
fn bundled_icons_dir() -> Option<std::path::PathBuf> {
    let self_exe = std::fs::read_link("/proc/self/exe").ok()?;
    // Dev build layout: `<repo>/target/{debug,release}/caerus`.
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
    btn.add_css_class("flat");
    if let Some(l) = btn.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    btn
}

/// A page header for the popover's slide-in pages: a back chevron +
/// bold title, separated from the page content below.
fn menu_page_header(stack: &gtk::Stack, title: &str) -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let back = gtk::Button::with_label("\u{2039}"); // ‹
    back.add_css_class("flat");
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

/// A switch row for the View/Settings pages: label, optional keycap
/// hint, switch. Builds the row around an existing switch — use this
/// when the switch must be reachable outside `populate_menu_popover`
/// (e.g. driven by a header button too); `switch_row` below is the
/// common case that doesn't need that.
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

/// A switch row that owns its own fresh switch — the common case.
/// Returns the row and the switch for binding.
fn switch_row(label: &str, accel: Option<&str>) -> (gtk::Box, gtk::Switch) {
    let switch = gtk::Switch::new();
    let row = switch_row_with(&switch, label, accel);
    (row, switch)
}

/// Single funnel for every sidebar-mode change (header button, F9, and
/// both View-menu switches all call into this) — applies the widget
/// state and re-syncs the two switches + button tooltip to match.
/// Re-entrant calls into this function (via the switches' own
/// `connect_active_notify`) always converge — each nested call's target
/// state already matches what's being applied, so further nesting stops.
fn apply_sidebar_mode(state: &Rc<WindowState>, visible: bool, minimal: bool) {
    // `width_request` alone won't move an already-positioned `GtkPaned`
    // divider (it's only a minimum), so drive `main_paned`'s position
    // directly — same technique `apply_panel_orientation` uses for the
    // detail pane's docked-right width. Capture the full-mode width
    // before narrowing it, so leaving minimal can restore it.
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

/// The header button's / F9's fixed 3-state cycle: Full -> Minimal ->
/// Hidden -> Full. Reaching Minimal or Full from Hidden any other way
/// (the View-menu switches) is handled separately in
/// `populate_menu_popover` — this cycle only defines what a *click*
/// does.
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

/// Builds the hamburger popover: a `gtk::Stack` of pages — root (View ▸ /
/// Settings ▸ / Keyboard Shortcuts ▸ / About / Quit) plus three slide-in
/// pages whose boolean controls are all switches.
fn populate_menu_popover(state: &Rc<WindowState>) {
    let stack = &state.menu_stack;
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);

    let popover = gtk::Popover::new();
    popover.set_child(Some(stack));
    state.menu_button.set_popover(Some(&popover));

    // Never reopen mid-navigation.
    {
        let stack = stack.clone();
        popover.connect_closed(move |_| stack.set_visible_child_name("root"));
    }

    // ── root page ──
    let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
    root.set_width_request(230);

    let nav_row = |label: &str, target: &'static str| -> gtk::Button {
        let btn = gtk::Button::new();
        btn.add_css_class("flat");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = gtk::Label::new(Some(label));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        let chevron = gtk::Label::new(Some("\u{25b8}")); // ▸
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

    {
        let state2 = state.clone();
        let popover = popover.clone();
        state.btn_mark_upgrades.connect_clicked(move |_| {
            popover.popdown();
            let mut names = std::collections::HashSet::new();
            let list = state2.store.list();
            let n = list.n_items();
            for i in 0..n {
                if let Some(obj) =
                    crate::backend::package_store::package_obj_at(&list, i)
                {
                    let p = obj.pkg();
                    if p.state == PkgState::Upgradable && p.mark == PkgMark::None {
                        names.insert(p.name.clone());
                    }
                }
            }
            state2.store.set_marks(&names, PkgMark::Upgrade);
            update_status_bar(&state2);
        });
    }
    root.append(&state.btn_mark_upgrades);

    {
        let state2 = state.clone();
        let popover = popover.clone();
        state.btn_unmark_all.connect_clicked(move |_| {
            popover.popdown();
            state2.store.clear_all_marks();
            update_status_bar(&state2);
        });
    }
    root.append(&state.btn_unmark_all);

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
    btn_quit.add_css_class("flat");
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
        // Goes through close_request so layout save / helper shutdown
        // is one code path regardless of how the window closes.
        let window = state.window.clone();
        btn_quit.connect_clicked(move |_| window.close());
    }
    root.append(&btn_quit);
    stack.add_named(&root, Some("root"));

    // ── View page ──
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
            // Turning minimal ON also shows the sidebar (it's the only
            // way to reach Minimal directly from Hidden); turning it
            // OFF just drops back to whatever visibility already was.
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

    let (vertical_row, sw_vertical) = switch_row("Vertical Panel", None);
    vertical_row.set_tooltip_text(Some(
        "Dock the detail panel to the right as a narrow column instead of below the \
         package list",
    ));
    sw_vertical.set_active(state.right_paned.orientation() == gtk::Orientation::Horizontal);
    {
        let state = state.clone();
        sw_vertical.connect_active_notify(move |sw| {
            let vertical = sw.is_active();
            apply_panel_orientation(
                &state.right_paned,
                &state.detail_pane,
                vertical,
                state.default_detail_pos.get(),
                state.right_paned.width(),
            );
        });
    }
    view.append(&vertical_row);

    let (status_row, sw_status) = switch_row("Status Bar", None);
    state
        .status_bar
        .bind_property("visible", &sw_status, "active")
        .bidirectional()
        .sync_create()
        .build();
    view.append(&status_row);
    stack.add_named(&view, Some("view"));

    // ── Settings page ── (replaces the former Settings dialog)
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

    // ── Keyboard Shortcuts page ── (essentials; Ctrl+? opens the full
    // overlay dialog)
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

// libadwaita's AboutWindow gets a proper CSD titlebar matching the rest
// of the app; the plain-GTK4 fallback below is otherwise identical.
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
    // GTK focuses the first focusable widget on present (a selectable
    // label here), which renders as pre-selected text; clear it.
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

/// Global shortcuts, active anywhere in the window (not just when a
/// specific widget has focus): Ctrl+F to search, Escape to clear it,
/// F5 to reload, Delete to mark the selected package for removal,
/// Ctrl+Q to quit.
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
            // Guard: don't hijack "select all text" while typing a search.
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
            // Ctrl+? — the full shortcuts overlay.
            gtk::gdk::Key::question if ctrl => {
                show_shortcuts_dialog(&state.window);
                glib::Propagation::Stop
            }
            // Ctrl+, — open the hamburger directly on its Settings page.
            gtk::gdk::Key::comma if ctrl => {
                state.menu_stack.set_visible_child_name("settings");
                state.menu_button.popup();
                glib::Propagation::Stop
            }
            // Same guard as Ctrl+A above.
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
    // ── Store signals ──
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

    // ── Sidebar / list / detail wiring ──
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
        // Jumps the main list to a clicked Dependencies row's package.
        // `select_package_by_name` fires `connect_package_selected`
        // above, so the detail pane updates for free.
        detail_pane.connect_jump_to_package(move |pkgname| {
            if state.pkg_list.select_package_by_name(&pkgname) {
                return;
            }
            // Not visible under current search/filter/repo — clear via
            // the sidebar (not pkg_list directly) so its highlighted row
            // stays in sync — and retry once.
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

            // Refresh the detail pane if the mark changed via a route
            // other than its own buttons (checkbox column, context menu).
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

    // ── Sidebar action rows (MAINTENANCE / TOOLS / Manage Repositories) ──
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

    // ── Session disconnect ──
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

    // ── Buttons ──
    {
        let btn_update = state.btn_update.clone();
        let state = state.clone();
        btn_update.connect_clicked(move |_| {
            trigger_update(&state, true, false); // sync + reload, with dialog
        });
    }
    {
        let btn_reload = state.btn_reload.clone();
        let state = state.clone();
        btn_reload.connect_clicked(move |_| {
            trigger_update(&state, false, false); // local reload only, no dialog
        });
    }
    {
        let btn_toggle_sidebar = state.btn_toggle_sidebar.clone();
        let state = state.clone();
        btn_toggle_sidebar.connect_clicked(move |_| cycle_sidebar_mode(&state));
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

    // ── Shutdown: persist the window/paned layout and tell the
    // privileged helper to exit when the window closes ──
    {
        let window = state.window.clone();
        let state = state.clone();
        window.connect_close_request(move |win| {
            use crate::ui::filter_sidebar::Section;
            WindowGeometry {
                width: win.width(),
                height: win.height(),
                // `main_paned.position()` is only a meaningful full-width
                // sidebar width outside rail mode — while minimal it's the
                // rail width instead (see `apply_sidebar_mode`), so fall
                // back to the remembered full-mode width in that case.
                sidebar_pos: if state.sidebar_minimal.get() {
                    state.default_sidebar_pos.get()
                } else {
                    state.main_paned.position()
                },
                // `right_paned.position()` is only a meaningful bottom-dock
                // height while actually in that orientation — in
                // right-dock mode it's a width instead (see
                // `apply_panel_orientation`), so fall back to whatever the
                // bottom-dock height was before switching.
                detail_pos: if state.right_paned.orientation() == gtk::Orientation::Vertical {
                    state.right_paned.position()
                } else {
                    state.default_detail_pos.get()
                },
                sync_at_launch: state.sync_at_launch.get(),
                search_name_only_default: state.search_name_only_default.get(),
                section_expanded: Section::ALL.map(|s| state.sidebar.is_expanded(s)),
                section_visible: Section::ALL
                    .map(|s| state.sidebar.section_widget(s).get_visible()),
                detail_pane_visible: state.btn_toggle_detail_pane.is_active(),
                vertical_panel: state.right_paned.orientation() == gtk::Orientation::Horizontal,
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
        // Menu actions share the same `Transaction` session, so disable
        // the whole menu button to prevent queuing a second batch
        // while one (including a silent at-launch sync) is in flight.
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
    // Otherwise a post-reload action could act on a stale Package snapshot.
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

    // Dry-run happens on the worker thread; the confirm dialog opens once
    // it reports back, so Apply never freezes the main loop.
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

/// Hold/unhold is applied right away rather than queued as a pending
/// mark; it needs no dependency resolution or batching.
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

/// "xbps-install -Su" via the helper's UPGRADE command — independent of
/// whatever the user has separately marked. Previews the set via a real
/// dry-run built from the app's currently-known-upgradable names; the
/// actual command lets xbps resolve its own set, which may differ
/// slightly (e.g. deps pulled in along the way).
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

/// Confirms before `xbps-remove -o`. The list shown is the app's own
/// `is_orphan` set from the last reload, barring changes made outside
/// caerus since then.
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

/// Confirms before `xbps-reconfigure -fa`: not destructive, but a heavy
/// system-wide action worth a deliberate second click.
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

/// Runs a single privileged protocol command outside the normal
/// mark/Apply batch (hold/unhold, orphan removal, cache cleanup, ...).
/// Shows the same progress dialog as a regular Apply, then reloads.
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

/// Maps a queued INSTALL/REMOVE/PURGE line to its force-override verb —
/// see the matching `*_FORCE` handlers in `caerus-helper`. Other
/// commands pass through unchanged.
fn force_variant(cmd: &str) -> String {
    for verb in ["INSTALL", "REMOVE", "PURGE"] {
        if let Some(rest) = cmd.strip_prefix(verb) {
            return format!("{verb}_FORCE{rest}");
        }
    }
    cmd.to_string()
}

/// Shown when an Apply batch fails, offering a forced retry (file
/// conflicts/unresolved deps a plain retry can't fix). Declining falls
/// back to clear-marks-and-reload.
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

/// Shows a transient, self-dismissing notification, as opposed to
/// `update_status_bar`'s persistent package-count summary.
fn show_toast(state: &Rc<WindowState>, msg: &str) {
    #[cfg(feature = "adwaita")]
    {
        state.toast_overlay.add_toast(adw::Toast::new(msg));
    }
    #[cfg(not(feature = "adwaita"))]
    {
        state.status_label.set_text(msg);
        // No AdwToast here, so restore the persistent summary after a
        // few seconds instead of leaving the transient message shown.
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
        // Show counts for what's on screen, not whole-database totals.
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
