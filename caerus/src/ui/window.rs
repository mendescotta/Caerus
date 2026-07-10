//! Main application window. Rust translation of ui/window.{h,c} (built
//! directly in code here rather than from a GtkBuilder .ui file).

use crate::backend::package::{PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::backend::transaction::Transaction;
use crate::ui::apply_dialog;
use crate::ui::detail_pane::DetailPane;
use crate::ui::filter_sidebar::FilterSidebar;
use crate::ui::package_list::PackageList;
use gio::prelude::*;
use gtk::prelude::*;
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
    btn_apply: gtk::Button,
    search_entry: gtk::SearchEntry,
    btn_search_name_only: gtk::ToggleButton,
    status_label: gtk::Label,
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
}

impl Default for WindowGeometry {
    fn default() -> Self {
        WindowGeometry {
            width: 1100,
            height: 700,
            sidebar_pos: 200,
            detail_pos: 420,
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
            let Ok(n) = value.trim().parse::<i32>() else {
                continue;
            };
            if n <= 0 {
                continue;
            }
            match key.trim() {
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
            "width={}\nheight={}\nsidebar_pos={}\ndetail_pos={}\n",
            self.width, self.height, self.sidebar_pos, self.detail_pos
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

    header.pack_start(&spinner);
    header.pack_start(&btn_update);
    header.pack_start(&btn_reload);
    header.pack_start(&btn_mark_upgrades);

    let btn_apply = gtk::Button::with_label("Apply (0)");
    btn_apply.set_sensitive(false);
    btn_apply.add_css_class("suggested-action");

    let btn_search_name_only = gtk::ToggleButton::new();
    btn_search_name_only.set_icon_name("edit-find-symbolic");
    btn_search_name_only.set_tooltip_text(Some(
        "Search by name only (default: name + description)",
    ));

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_width_request(220);
    search_entry.set_placeholder_text(Some("Search packages\u{2026}"));

    header.pack_end(&search_entry);
    header.pack_end(&btn_search_name_only);
    header.pack_end(&btn_apply);

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
        btn_apply,
        search_entry,
        btn_search_name_only,
        status_label,
    });

    wire_up(&state);

    // Sync repos at launch silently (no dialog), then reload. Auth
    // prompt still fires immediately via the session spawn. If sync
    // fails, the error appears in the status bar and local load
    // continues — matching the original's `trigger_update(win, TRUE, TRUE)`.
    trigger_update(&state, true, true);

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
.pkg-upgradable { color: @warning_color; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(window),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn wire_up(state: &Rc<WindowState>) {
    // ── Store signals ──
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_started(move || {
            set_loading(&state, true);
            state.status_label.set_text("Loading package database\u{2026}");
        });
    }
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_finished(move |_n| {
            set_loading(&state, false);
            update_status_bar(&state);
        });
    }
    {
        let store = state.store.clone();
        let state = state.clone();
        store.connect_load_error(move |msg| {
            set_loading(&state, false);
            state
                .status_label
                .set_text(&format!("Error loading packages: {}", msg));
        });
    }

    // ── Sidebar / list / detail wiring ──
    {
        let sidebar = state.sidebar.clone();
        let state = state.clone();
        sidebar.connect_filter_changed(move |mode| {
            state.pkg_list.set_filter(mode);
        });
    }
    {
        let pkg_list = state.pkg_list.clone();
        let state = state.clone();
        pkg_list.connect_package_selected(move |pkg| {
            state.detail_pane.show_package(pkg.as_ref());
        });
    }
    {
        let pkg_list = state.pkg_list.clone();
        let state = state.clone();
        pkg_list.connect_marks_changed(move || {
            update_status_bar(&state);
        });
    }
    {
        let detail_pane = state.detail_pane.clone();
        let state = state.clone();
        detail_pane.connect_mark_changed(move || {
            update_status_bar(&state);
        });
    }

    // ── Session disconnect ──
    {
        let session = state.session.clone();
        let state = state.clone();
        session.connect_disconnected(move |expected| {
            if !expected {
                state.status_label.set_text(
                    "Privileged helper disconnected — the next action will re-authenticate.",
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
    } else {
        state.spinner.stop();
        state.btn_update.set_sensitive(true);
        state.btn_reload.set_sensitive(true);
    }
}

fn do_reload(state: &Rc<WindowState>) {
    state.detail_pane.show_package(None);
    state.store.load_async();
}

fn trigger_update(state: &Rc<WindowState>, sync_first: bool, silent: bool) {
    set_loading(state, true);
    if sync_first {
        state.status_label.set_text("Syncing repositories\u{2026}");
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
            let finished_id = state.session.connect_finished(move |success| {
                session.disconnect_finished(finished_id_cell2.get());
                if !success {
                    state2
                        .status_label
                        .set_text("Repository sync failed — loading local data.");
                } else {
                    state2
                        .status_label
                        .set_text("Repositories synced. Loading package list\u{2026}");
                }
                do_reload(&state2);
            });
            finished_id_cell.set(finished_id);
            state.session.run_async();
        } else {
            let state2 = state.clone();
            apply_dialog::run(
                Some(state.window.upcast_ref()),
                &state.session,
                &commands,
                "Syncing Repositories",
                move |success| {
                    if !success {
                        state2
                            .status_label
                            .set_text("Repository sync failed — loading local data anyway.");
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

    let state2 = state.clone();
    apply_dialog::run(
        Some(state.window.upcast_ref()),
        &state.session,
        &commands,
        "Applying Changes",
        move |success| {
            state2.status_label.set_text(if success {
                "Changes applied. Reloading\u{2026}"
            } else {
                "Some changes failed — see log. Reloading\u{2026}"
            });
            state2.store.clear_all_marks();
            do_reload(&state2);
        },
    );
}

fn update_status_bar(state: &Rc<WindowState>) {
    let total = state.store.list().n_items();
    let installed = state.store.count_installed();
    let upgradable = state.store.count_upgradable();
    let marked = state.store.count_marked();
    state.status_label.set_text(&format!(
        "{} packages.  {} installed.  {} upgradable.  {} marked.",
        total, installed, upgradable, marked
    ));
    update_apply_button(state, marked);
    update_mark_upgrades_button(state, upgradable);
}

fn update_apply_button(state: &Rc<WindowState>, marked: u32) {
    state.btn_apply.set_label(&format!("Apply ({})", marked));
    state.btn_apply.set_sensitive(marked > 0);
}

fn update_mark_upgrades_button(state: &Rc<WindowState>, upgradable: u32) {
    state.btn_mark_upgrades.set_sensitive(upgradable > 0);
}
