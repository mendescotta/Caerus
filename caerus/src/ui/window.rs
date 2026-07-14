//! Main application window. Rust translation of ui/window.{h,c} (built
//! directly in code here rather than from a GtkBuilder .ui file).

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
    menu_button: gtk::MenuButton,
    menu_buttons: AppMenuButtons,
    search_entry: gtk::SearchEntry,
    btn_search_name_only: gtk::ToggleButton,
    status_label: gtk::Label,

    /// Wraps the whole window content so transient, self-dismissing
    /// notifications (sync failed, changes applied, ...) can show as a
    /// toast instead of overwriting `status_label`'s persistent package
    /// count — see `show_toast`. Only exists when built with
    /// `--features adwaita`; the plain-GTK4 build has no equivalent
    /// widget, `show_toast` just falls back to `status_label` there.
    #[cfg(feature = "adwaita")]
    toast_overlay: adw::ToastOverlay,

    /// Mirrors the package list's current selection, kept here purely
    /// so the Delete-key shortcut has something to act on without
    /// having to poke a getter through `DetailPane`.
    selected_pkg: RefCell<Option<Package>>,

    /// Whether to sync repositories at launch — see `WindowGeometry`'s
    /// field of the same name. Only read/written by the "Sync
    /// Repositories at Launch" menu checkbox and the close-request
    /// handler that persists it; not consulted again after startup.
    sync_at_launch: std::cell::Cell<bool>,
}

/// Window size + paned-divider positions, persisted across launches so
/// a user's chosen layout survives a restart. Deliberately a tiny
/// hand-rolled `key=value` file rather than pulling in a serialization
/// crate for four integers.
struct WindowGeometry {
    width: i32,
    height: i32,
    sidebar_pos: i32,
    detail_pos: i32,
    /// Whether to sync repositories (a privileged `pkexec` action) at
    /// launch, before the user has clicked anything. Defaults to `false`
    /// — a fresh install shouldn't greet a first-time user with an
    /// unexplained authentication prompt before they've seen a single
    /// package; exposed as a checkable "Sync Repositories at Launch" item
    /// in the app menu for anyone who'd rather have it back.
    sync_at_launch: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        WindowGeometry {
            width: 1100,
            height: 700,
            sidebar_pos: 200,
            detail_pos: 420,
            sync_at_launch: false,
        }
    }
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
        let mut geometry = WindowGeometry::default();
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
        let contents = format!(
            "width={}\nheight={}\nsidebar_pos={}\ndetail_pos={}\nsync_at_launch={}\n",
            self.width, self.height, self.sidebar_pos, self.detail_pos, self.sync_at_launch as i32
        );
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

    let spinner = gtk::Spinner::new();
    let btn_update = gtk::Button::from_icon_name("software-update-available-symbolic");
    btn_update.set_tooltip_text(Some("Sync repositories and reload package list"));
    let btn_reload = gtk::Button::from_icon_name("view-refresh-symbolic");
    btn_reload.set_tooltip_text(Some("Reload local package list without syncing"));
    let btn_mark_upgrades = gtk::Button::with_label("Mark All Upgrades");
    btn_mark_upgrades.set_tooltip_text(Some(
        "Queue every upgradable package as a pending mark, reviewed and applied via Apply \
         — unlike the app menu's Full System Upgrade, this can be combined with other \
         pending install/remove marks and reviewed before anything runs.",
    ));
    let btn_unmark_all = gtk::Button::with_label("Unmark All");
    btn_unmark_all.set_sensitive(false);
    btn_unmark_all.set_tooltip_text(Some(
        "Clear every pending Install/Upgrade/Remove/Purge mark",
    ));

    header.pack_start(&spinner);
    header.pack_start(&btn_update);
    header.pack_start(&btn_reload);
    header.pack_start(&btn_mark_upgrades);
    header.pack_start(&btn_unmark_all);

    let btn_apply = gtk::Button::with_label("Apply (0)");
    btn_apply.set_sensitive(false);
    btn_apply.add_css_class("suggested-action");

    let btn_search_name_only = gtk::ToggleButton::new();
    btn_search_name_only.set_icon_name("edit-find-symbolic");
    btn_search_name_only
        .set_tooltip_text(Some("Search by name only (default: name + description)"));

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_width_request(220);
    search_entry.set_placeholder_text(Some("Search packages\u{2026}"));

    header.pack_end(&search_entry);
    header.pack_end(&btn_search_name_only);
    header.pack_end(&btn_apply);

    let (menu_button, menu_buttons) = build_app_menu(&window);
    header.pack_end(&menu_button);

    window.set_titlebar(Some(&header));

    // ── Backend ──
    let store = PackageStore::new();
    let session = Transaction::new();

    // ── Body ──
    let sidebar = FilterSidebar::new();
    let pkg_list = PackageList::new(store.clone());
    let detail_pane = DetailPane::new(store.clone());

    let right_paned = gtk::Paned::new(gtk::Orientation::Vertical);
    right_paned.set_position(geometry.detail_pos);
    right_paned.set_resize_start_child(true);
    right_paned.set_shrink_start_child(false);
    right_paned.set_resize_end_child(false);
    right_paned.set_shrink_end_child(false);
    right_paned.set_start_child(Some(pkg_list.widget()));
    right_paned.set_end_child(Some(detail_pane.widget()));

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

    let state = Rc::new(WindowState {
        window: window.clone(),
        store,
        session,
        sidebar,
        pkg_list,
        detail_pane,
        main_paned: main_paned.clone(),
        right_paned: right_paned.clone(),
        spinner,
        btn_update,
        btn_reload,
        btn_mark_upgrades,
        btn_unmark_all,
        btn_apply,
        menu_button: menu_button.clone(),
        menu_buttons,
        search_entry,
        btn_search_name_only,
        status_label,
        #[cfg(feature = "adwaita")]
        toast_overlay,
        selected_pkg: RefCell::new(None),
        sync_at_launch: std::cell::Cell::new(geometry.sync_at_launch),
    });
    state
        .menu_buttons
        .sync_at_launch
        .set_active(geometry.sync_at_launch);

    wire_up(&state);
    wire_keyboard_shortcuts(&state);

    // Sync repos at launch silently (no dialog), then reload — unless
    // the user has opted out via "Sync Repositories at Launch" in the
    // app menu, in which case this is a plain local reload with no
    // privileged action at all. When it does run, the auth prompt fires
    // immediately via the session spawn; if sync fails, the error
    // appears in the status bar and local load continues — matching the
    // original's `trigger_update(win, TRUE, TRUE)`.
    trigger_update(&state, geometry.sync_at_launch, true);

    window
}

fn install_css(window: &gtk::ApplicationWindow) {
    let css = gtk::CssProvider::new();
    css.load_from_string(
        ".statusbar {
  background: @headerbar_bg_color;
  border-top: 1px solid @borders; }
.section-header {
  font-weight: bold; padding: 4px 6px;
  opacity: 0.55; font-size: 0.78em;
  letter-spacing: 0.06em; }
.pkg-marked   { font-weight: bold; }
.pkg-installed  { color: @success_color; }
.pkg-upgradable { color: @warning_color; }
progressbar.apply-progress trough {
  min-height: 22px; }
progressbar.apply-progress trough progress {
  min-height: 22px; }
.apply-progress-text {
  font-size: 0.8em; font-weight: bold; color: white;
  text-shadow: 0 1px 2px rgba(0,0,0,0.7); }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(window),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Every symbolic icon name used anywhere in the app — kept in one place
/// so the startup fallback check below doesn't drift from what's
/// actually referenced across the UI modules.
const USED_SYMBOLIC_ICONS: &[&str] = &[
    "software-update-available-symbolic",
    "view-refresh-symbolic",
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
];

/// GTK only resolves an icon name against the *active* icon theme (plus
/// the "hicolor" fallback theme, which normally ships no real icon files
/// of its own — it's just the spec-mandated fallback directory
/// hierarchy). It does not also try Adwaita as a second fallback. Outside
/// GNOME, the active theme (Breeze, or whatever the desktop sets)
/// commonly covers most standard symbolic names but not all of them, so
/// a handful of icons in the header bar and filter sidebar render blank
/// even when `adwaita-icon-theme` is installed — it's just not the
/// active theme. Fixed the same way the app's own logo already is: a
/// bundled copy of the specific icons this app needs lives under
/// `data/icons/hicolor/symbolic/` (copied from Adwaita, which ships them
/// under a CC0/LGPL-compatible license same as the rest of GNOME's
/// icon set), placed in the *hicolor* theme's own directory structure —
/// hicolor is checked as a fallback for every icon lookup regardless of
/// which theme is active, by design, so this doesn't depend on guessing
/// or overriding the desktop's chosen theme at all.
///
/// `install.sh`/`dev-install.sh` register this tree at its real system
/// location for an installed build. For a bare `cargo build`/`cargo run`
/// with neither script run yet, this also registers the checkout's own
/// `caerus/data/icons` directory directly — same dev-vs-installed
/// resolution shape as `Transaction::find_helper_path`.
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
/// fallback icons, or `None` if it can't be found (e.g. a stripped
/// install where `install.sh` already placed them at the real system
/// icon path, which GTK searches on its own with no extra help needed).
fn bundled_icons_dir() -> Option<std::path::PathBuf> {
    let self_exe = std::fs::read_link("/proc/self/exe").ok()?;
    // Dev build layout: `<repo>/target/{debug,release}/caerus`, data at
    // `<repo>/caerus/data/icons`.
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

/// The action buttons the app menu contains beyond Keyboard
/// Shortcuts/About/Quit (which `build_app_menu` wires up entirely on
/// its own since they don't need `WindowState`). Everything here does,
/// so `build_window` stashes these on `WindowState` and `wire_up`
/// attaches their click handlers once it exists.
struct AppMenuButtons {
    full_upgrade: gtk::Button,
    remove_orphans: gtk::Button,
    clean_cache: gtk::Button,
    verify_db: gtk::Button,
    reconfigure_all: gtk::Button,
    find_owner: gtk::Button,
    alternatives: gtk::Button,
    repositories: gtk::Button,
    history: gtk::Button,
    sync_at_launch: gtk::CheckButton,
}

fn build_app_menu(window: &gtk::ApplicationWindow) -> (gtk::MenuButton, AppMenuButtons) {
    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Main Menu"));

    let popover = gtk::Popover::new();
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);
    vbox.set_width_request(240);

    let maintenance_header = gtk::Label::new(Some("MAINTENANCE"));
    maintenance_header.set_xalign(0.0);
    maintenance_header.add_css_class("section-header");
    let full_upgrade = flat_menu_button("Full System Upgrade\u{2026}");
    full_upgrade.set_tooltip_text(Some(
        "xbps-install -Su — upgrade every installed package immediately, independent of \
         (and without touching) anything currently marked. For queuing upgrades alongside \
         other pending changes instead, use Mark All Upgrades + Apply.",
    ));
    let remove_orphans = flat_menu_button("Remove Orphaned Packages");
    remove_orphans.set_tooltip_text(Some(
        "xbps-remove -o — drop packages nothing else depends on anymore",
    ));
    let clean_cache = flat_menu_button("Clean Package Cache");
    clean_cache.set_tooltip_text(Some(
        "xbps-remove -O — delete cached package files superseded by a newer version",
    ));
    let verify_db = flat_menu_button("Verify Package Database\u{2026}");
    verify_db.set_tooltip_text(Some(
        "xbps-pkgdb -a --checks files,dependencies,alternatives,pkgdb",
    ));
    let reconfigure_all = flat_menu_button("Reconfigure All Packages\u{2026}");
    reconfigure_all.set_tooltip_text(Some(
        "xbps-reconfigure -fa — force re-run every installed package's post-install \
         configuration script. Useful after an interrupted transaction or a libc/shared \
         library upgrade left some packages unconfigured.",
    ));

    let options_header = gtk::Label::new(Some("OPTIONS"));
    options_header.set_xalign(0.0);
    options_header.add_css_class("section-header");
    options_header.set_margin_top(6);
    let sync_at_launch = gtk::CheckButton::with_label("Sync Repositories at Launch");
    sync_at_launch.set_margin_start(6);
    sync_at_launch.set_margin_top(2);
    sync_at_launch.set_margin_bottom(2);
    sync_at_launch.set_tooltip_text(Some(
        "When enabled, Caerus syncs repository indexes (a privileged action, prompting for \
         your password) automatically every time it starts. Disable this to skip that prompt \
         at launch — you can still sync manually any time via the header bar's sync button.",
    ));

    let tools_header = gtk::Label::new(Some("TOOLS"));
    tools_header.set_xalign(0.0);
    tools_header.add_css_class("section-header");
    tools_header.set_margin_top(6);
    let find_owner = flat_menu_button("Find Owning Package\u{2026}");
    find_owner.set_tooltip_text(Some("xbps-query -o — which package owns a given file"));
    let alternatives = flat_menu_button("Alternatives\u{2026}");
    alternatives.set_tooltip_text(Some("Switch between packages providing the same files"));
    let repositories = flat_menu_button("Repositories\u{2026}");
    repositories.set_tooltip_text(Some("Add or remove xbps repositories"));
    let history = flat_menu_button("Transaction History\u{2026}");
    history.set_tooltip_text(Some(
        "Past Apply batches and maintenance actions, newest first",
    ));

    let btn_shortcuts = flat_menu_button("Keyboard Shortcuts");
    let btn_about = flat_menu_button("About Caerus");
    let btn_quit = flat_menu_button("Quit");

    vbox.append(&maintenance_header);
    vbox.append(&full_upgrade);
    vbox.append(&remove_orphans);
    vbox.append(&clean_cache);
    vbox.append(&verify_db);
    vbox.append(&reconfigure_all);
    vbox.append(&options_header);
    vbox.append(&sync_at_launch);
    vbox.append(&tools_header);
    vbox.append(&find_owner);
    vbox.append(&alternatives);
    vbox.append(&repositories);
    vbox.append(&history);
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    vbox.append(&btn_shortcuts);
    vbox.append(&btn_about);
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    vbox.append(&btn_quit);
    popover.set_child(Some(&vbox));
    menu_button.set_popover(Some(&popover));

    // These all need `WindowState`, wired later; every click should
    // still close the popover regardless of which button.
    let buttons = AppMenuButtons {
        full_upgrade,
        remove_orphans,
        clean_cache,
        verify_db,
        reconfigure_all,
        find_owner,
        alternatives,
        repositories,
        history,
        sync_at_launch,
    };
    for btn in [
        &buttons.full_upgrade,
        &buttons.remove_orphans,
        &buttons.clean_cache,
        &buttons.verify_db,
        &buttons.reconfigure_all,
        &buttons.find_owner,
        &buttons.alternatives,
        &buttons.repositories,
        &buttons.history,
    ] {
        let popover = popover.clone();
        btn.connect_clicked(move |_| popover.popdown());
    }

    {
        let window = window.clone();
        let popover = popover.clone();
        btn_shortcuts.connect_clicked(move |_| {
            popover.popdown();
            show_shortcuts_dialog(&window);
        });
    }
    {
        let window = window.clone();
        let popover = popover.clone();
        btn_about.connect_clicked(move |_| {
            popover.popdown();
            show_about_dialog(&window);
        });
    }
    {
        let window = window.clone();
        btn_quit.connect_clicked(move |_| {
            // Goes through the window's own close_request handler, so
            // the layout gets saved and the helper session shut down
            // the same as any other way of closing — one code path.
            window.close();
        });
    }

    (menu_button, buttons)
}

// libadwaita's AboutWindow gets a proper CSD titlebar matching every
// other dialog in the app (see the earlier UX review's finding that
// plain GtkAboutDialog was the one visibly inconsistent dialog); the
// plain-GTK4 fallback below is otherwise identical in content. Which one
// gets compiled in is a build-time choice (`--features adwaita`), not
// something detected per-machine at runtime — see the `[features]` note
// in caerus/Cargo.toml.
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
    // GTK hands initial keyboard focus to the first focusable widget on
    // present — here, a selectable comments/version label — which then
    // renders as if its entire text were pre-selected/highlighted. Same
    // root cause `dialog_util::present_focused` works around for this
    // project's own hand-built dialogs by focusing a specific button
    // instead; `AboutDialog` exposes no such button to target, so just
    // clear focus outright.
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
        ("Delete", "Mark selected package for removal"),
        (
            "Ctrl+A",
            "Select all visible packages (for right-click bulk actions)",
        ),
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
            // Guarded on the search entry not having focus so this
            // doesn't hijack the ordinary "select all text" behavior
            // while typing a search query.
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
            // Same guard as Ctrl+A above — otherwise editing the search
            // query with Delete/Backspace could simultaneously mark
            // the currently-selected row for removal.
            gtk::gdk::Key::Delete if !state.search_entry.has_focus() => {
                if let Some(pkg) = state.selected_pkg.borrow().clone() {
                    // Goes through the same reverse-dependency confirmation
                    // and "not already marked" guard every other removal
                    // path uses (checkbox, context menu, detail pane,
                    // double-click) — status bar/detail pane refresh
                    // happens via the usual marks-changed callback once
                    // (or if) the mark is actually applied, not here.
                    let root = state.window.clone().upcast::<gtk::Window>();
                    state.pkg_list.request_remove(Some(root), &pkg);
                }
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
            state
                .sidebar
                .set_available_repositories(state.pkg_list.available_repositories());
        });
    }
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_error(move |msg| {
            set_loading(&state, false);
            show_toast(&state, &format!("Error loading packages: {}", msg));
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
        let pkg_list = state.pkg_list.clone();
        let state = state.clone();
        pkg_list.connect_marks_changed(move || {
            update_status_bar(&state);

            // A mark can change via the checkbox column or the
            // right-click context menu while the very same package is
            // showing in the detail pane, whose own Install/Upgrade/
            // Remove/Unmark buttons only ever refresh themselves on
            // their own click — without this, they'd keep showing the
            // pre-mark state until the row is re-selected.
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
                &format!("REINSTALL {}", pkgname),
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
                &format!("RECONFIGURE {}", pkgname),
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
                &format!("DOWNLOAD {}", pkgname),
                "Downloading Package",
            );
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_repolock_requested(move |pkgname, want_locked| {
            let cmd = if want_locked {
                format!("REPOLOCK {}", pkgname)
            } else {
                format!("REPOUNLOCK {}", pkgname)
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
                format!("MARKAUTO {}", pkgname)
            } else {
                format!("MARKMANUAL {}", pkgname)
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
        let btn = state.menu_buttons.full_upgrade.clone();
        let state = state.clone();
        btn.connect_clicked(move |_| {
            on_full_upgrade_clicked(&state);
        });
    }
    {
        let btn = state.menu_buttons.remove_orphans.clone();
        let state = state.clone();
        btn.connect_clicked(move |_| {
            run_maintenance_command(&state, "ORPHANS", "Removing Orphaned Packages");
        });
    }
    {
        let btn = state.menu_buttons.clean_cache.clone();
        let state = state.clone();
        btn.connect_clicked(move |_| {
            run_maintenance_command(&state, "CLEANCACHE", "Cleaning Package Cache");
        });
    }
    {
        let btn = state.menu_buttons.verify_db.clone();
        let state = state.clone();
        btn.connect_clicked(move |_| {
            run_maintenance_command(&state, "VERIFY", "Verifying Package Database");
        });
    }
    {
        let btn = state.menu_buttons.reconfigure_all.clone();
        let state = state.clone();
        btn.connect_clicked(move |_| {
            run_maintenance_command(&state, "RECONFIGURE_ALL", "Reconfiguring All Packages");
        });
    }
    {
        let btn = state.menu_buttons.sync_at_launch.clone();
        let state = state.clone();
        btn.connect_toggled(move |btn| {
            state.sync_at_launch.set(btn.is_active());
        });
    }
    {
        let btn = state.menu_buttons.find_owner.clone();
        let window = state.window.clone();
        btn.connect_clicked(move |_| {
            crate::ui::file_owner_dialog::show(Some(window.upcast_ref()));
        });
    }
    {
        let btn = state.menu_buttons.alternatives.clone();
        let window = state.window.clone();
        let session = state.session.clone();
        btn.connect_clicked(move |_| {
            crate::ui::alternatives_dialog::show(Some(window.upcast_ref()), &session);
        });
    }
    {
        let btn = state.menu_buttons.repositories.clone();
        let window = state.window.clone();
        let session = state.session.clone();
        let state_for_reload = state.clone();
        btn.connect_clicked(move |_| {
            let state_for_reload = state_for_reload.clone();
            crate::ui::repo_manager::show(Some(window.upcast_ref()), &session, move || {
                do_reload(&state_for_reload);
            });
        });
    }
    {
        let btn = state.menu_buttons.history.clone();
        let window = state.window.clone();
        btn.connect_clicked(move |_| {
            crate::ui::history_dialog::show(Some(window.upcast_ref()));
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
        let btn_mark_upgrades = state.btn_mark_upgrades.clone();
        let state = state.clone();
        btn_mark_upgrades.connect_clicked(move |_| {
            let n = state.store.list().n_items();
            for i in 0..n {
                if let Some(obj) = state.store.list().item(i) {
                    let obj = obj
                        .downcast::<crate::backend::package::PackageObject>()
                        .unwrap();
                    let p = obj.pkg();
                    if p.state == PkgState::Upgradable && p.mark == PkgMark::None {
                        let name = p.name.clone();
                        drop(p);
                        state.store.set_mark(&name, PkgMark::Upgrade);
                    }
                }
            }
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

    // ── Shutdown: persist the window/paned layout, and make sure the
    // privileged helper is told to exit when the window closes,
    // mirroring the original's dispose() handler on CaerusWindow.
    {
        let window = state.window.clone();
        let state = state.clone();
        window.connect_close_request(move |win| {
            WindowGeometry {
                width: win.width(),
                height: win.height(),
                sidebar_pos: state.main_paned.position(),
                detail_pos: state.right_paned.position(),
                sync_at_launch: state.sync_at_launch.get(),
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
        // The app menu's maintenance actions (Full System Upgrade, etc.)
        // and Repositories/Alternatives all queue commands on the same
        // shared `Transaction` session — disabling the whole menu button
        // (not just btn_update/btn_reload) closes it off during the
        // silent at-launch sync too, so nothing can queue a second,
        // independent batch against a session that's still mid-SYNC.
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
    // Otherwise Delete right after a reload (e.g. once an Apply batch
    // finishes) would still act on a pre-reload Package snapshot —
    // wrong `essential`/state if this package's status changed in the
    // transaction that just ran, or a no-op `set_mark` log if it was
    // removed entirely.
    *state.selected_pkg.borrow_mut() = None;
    state.store.load_async();
}

fn trigger_update(state: &Rc<WindowState>, sync_first: bool, silent: bool) {
    set_loading(state, true);
    if sync_first {
        state.status_label.set_text(if silent {
            // At-launch: this is the very first thing the user sees, so
            // spell out that a password prompt is about to appear rather
            // than letting it show up unexplained — see "Sync
            // Repositories at Launch" in the app menu to turn this off.
            "Requesting authentication to sync repositories\u{2026}"
        } else {
            "Syncing repositories\u{2026}"
        });
        let commands = vec!["SYNC".to_string()];
        if silent {
            // At-launch: queue SYNC and run it without a dialog, via a
            // one-shot "finished" listener that detaches itself the
            // moment it fires.
            for c in &commands {
                state.session.add_command(c);
            }
            let state2 = state.clone();
            let session = state.session.clone();
            let finished_id_cell = Rc::new(std::cell::Cell::new(0u64));
            let finished_id_cell2 = finished_id_cell.clone();
            let commands_for_history = commands.clone();
            let finished_id = state.session.connect_finished(move |success| {
                session.disconnect_finished(finished_id_cell2.get());
                crate::backend::history::record(&commands_for_history, success);
                if !success {
                    show_toast(&state2, "Repository sync failed — loading local data.");
                } else {
                    show_toast(&state2, "Repositories synced. Loading package list\u{2026}");
                }
                do_reload(&state2);
            });
            finished_id_cell.set(finished_id);
            state.session.run_async();
        } else {
            let state2 = state.clone();
            let commands_for_history = commands.clone();
            apply_dialog::run(
                Some(state.window.upcast_ref()),
                &state.session,
                &commands,
                "Syncing Repositories",
                move |success| {
                    crate::backend::history::record(&commands_for_history, success);
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
    let preview = state.store.preview_transaction(ops);

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
            let commands_for_history = commands.clone();
            let commands_for_retry = commands.clone();
            apply_dialog::run(
                Some(state2.window.upcast_ref()),
                &state2.session,
                &commands,
                "Applying Changes",
                move |success| {
                    crate::backend::history::record(&commands_for_history, success);
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
}

/// Hold/unhold is applied right away rather than queued as a pending
/// mark — unlike install/upgrade/remove, it needs no dependency
/// resolution or batching, so there's nothing to gain from deferring it
/// to Apply.
fn on_hold_requested(state: &Rc<WindowState>, pkgname: &str, want_hold: bool) {
    let cmd = if want_hold {
        format!("HOLD {}", pkgname)
    } else {
        format!("UNHOLD {}", pkgname)
    };
    let title = if want_hold {
        "Holding Package"
    } else {
        "Releasing Hold"
    };
    run_maintenance_command(state, &cmd, title);
}

/// "xbps-install -Su" via the helper's UPGRADE command — a full-system
/// pass, independent of (and doesn't touch) whatever the user has
/// separately marked for install/remove. Previews the set via a real
/// `xbps_transaction_prepare()` dry-run (see
/// `PackageStore::preview_transaction`) built from the app's own
/// currently-known-upgradable names; the actual command still lets xbps
/// resolve its own upgrade set, which may differ slightly (e.g. deps
/// pulled in along the way), but the preview itself is real libxbps
/// output, not a local guess.
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
    let preview = state.store.preview_transaction(ops);

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
}

/// Runs a single privileged protocol command outside the normal
/// mark/Apply batch — used for actions that are self-contained and
/// don't need dependency resolution or a pending-changes review
/// (hold/unhold, orphan removal, cache cleanup). Shows the same
/// progress dialog as a regular Apply, then reloads.
fn run_maintenance_command(state: &Rc<WindowState>, cmd: &str, title: &str) {
    let state2 = state.clone();
    let cmd_for_history = cmd.to_string();
    apply_dialog::run(
        Some(state.window.upcast_ref()),
        &state.session,
        &[cmd.to_string()],
        title,
        move |success| {
            crate::backend::history::record(std::slice::from_ref(&cmd_for_history), success);
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

/// Maps a queued INSTALL/REMOVE/PURGE line to its force-override verb
/// (same package names, `_FORCE` suffix on the verb) — see the matching
/// `INSTALL_FORCE`/`REMOVE_FORCE`/`PURGE_FORCE` handlers in
/// `caerus-helper`. Commands with no force variant (UPGRADE, HOLD, ...)
/// pass through unchanged, though in practice only INSTALL/REMOVE/PURGE
/// ever reach this from `on_apply_clicked`'s batch.
fn force_variant(cmd: &str) -> String {
    for verb in ["INSTALL", "REMOVE", "PURGE"] {
        if let Some(rest) = cmd.strip_prefix(verb) {
            return format!("{}_FORCE{}", verb, rest);
        }
    }
    cmd.to_string()
}

/// Shown when an Apply batch fails — file conflicts and unresolved
/// reverse-dependencies/shared libraries are the two cases a plain
/// retry can't fix but forcing through can, so offer that explicitly
/// rather than leaving the user to go find the equivalent `xbps-install`/
/// `xbps-remove` flags themselves. Declining (Cancel, Escape, or the
/// window-manager close affordance) falls back to the same
/// clear-marks-and-reload the non-offered failure path used before.
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
            let forced_for_history = forced.clone();
            apply_dialog::run(
                Some(state.window.upcast_ref()),
                &state.session,
                &forced,
                "Retrying With Force",
                move |success| {
                    crate::backend::history::record(&forced_for_history, success);
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

/// Shows a transient, self-dismissing notification — sync failed,
/// changes applied, a batch finished, etc — as opposed to
/// `update_status_bar`'s persistent package-count summary. On the
/// `adwaita` build this is a real auto-dismissing `AdwToast` that
/// doesn't clobber the persistent summary underneath; otherwise it
/// falls back to overwriting `status_label` directly, exactly like
/// before this existed (the next reload's `update_status_bar` call
/// still puts the real summary back either way).
fn show_toast(state: &Rc<WindowState>, msg: &str) {
    #[cfg(feature = "adwaita")]
    {
        state.toast_overlay.add_toast(adw::Toast::new(msg));
    }
    #[cfg(not(feature = "adwaita"))]
    {
        state.status_label.set_text(msg);
    }
}

fn update_status_bar(state: &Rc<WindowState>) {
    let upgradable = state.store.count_upgradable();
    let marked = state.store.count_marked();

    if state.pkg_list.has_active_search() {
        // While searching, the whole-database totals aren't what the
        // user is looking at — show counts for the results actually on
        // screen instead (installed vs. not, among matches).
        let (total, installed, not_installed) = state.pkg_list.visible_counts();
        state.status_label.set_text(&format!(
            "{} result{} — {} installed, {} not installed.  {} marked.",
            total,
            if total == 1 { "" } else { "s" },
            installed,
            not_installed,
            marked
        ));
    } else {
        let total = state.store.list().n_items();
        let installed = state.store.count_installed();
        state.status_label.set_text(&format!(
            "{} packages.  {} installed.  {} upgradable.  {} marked.",
            total, installed, upgradable, marked
        ));
    }
    update_apply_button(state, marked);
    update_mark_upgrades_button(state, upgradable);
}

fn update_apply_button(state: &Rc<WindowState>, marked: u32) {
    state.btn_apply.set_label(&format!("Apply ({})", marked));
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
