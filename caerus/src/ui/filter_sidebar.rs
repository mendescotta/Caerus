//! Preset filter sidebar. Rust translation of `ui/filter_sidebar.{h,c}`
//! (built directly in code here rather than from a `GtkBuilder` .ui file
//! — see the top-level README for why).
//!
//! Row order must stay in sync with `FilterMode::from_row_index` in
//! backend/package.rs, exactly like the original's comment about
//! `on_preset_selected()`'s use of `gtk_list_box_row_get_index()`.

use crate::backend::custom_filters::{ActiveFilter, CustomFilters};
use crate::backend::package::FilterMode;
use crate::backend::repo_names::{display_repo, RepoNames};
use crate::ui::custom_filters_dialog;
use crate::ui::dialog_util::{modal_window, present_focused};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type FilterChangedCbs = RefCell<Vec<Box<dyn Fn(ActiveFilter)>>>;
type RepositoryChangedCbs = RefCell<Vec<Box<dyn Fn(Option<String>)>>>;
type ActionCbs = RefCell<Vec<Box<dyn Fn(SidebarAction)>>>;

/// Row count of the fixed preset filters (All … Orphaned) at the top of
/// `preset_lb`, before any custom filter rows. Must match the number of
/// `preset_lb.append(&make_row(...))` calls in `FilterSidebar::new` and
/// `FilterMode::from_row_index`'s range.
const NUM_PRESET_ROWS: i32 = 7;

/// An operational command living in the sidebar's MAINTENANCE / TOOLS
/// sections (or the REPOSITORIES section's manage row). The sidebar only
/// emits these; `window.rs` routes each to the same handler the old app
/// menu used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    FullUpgrade,
    RemoveOrphans,
    CleanCache,
    VerifyDb,
    Reconfigure,
    PurgeKernels,
    FindOwner,
    Alternatives,
    History,
    ManageRepos,
}

/// The four collapsible sidebar sections, in display order. Used both
/// for the View-menu visibility toggles and expanded-state persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Filters,
    Repositories,
    Maintenance,
    Tools,
}

impl Section {
    pub const ALL: [Self; 4] = [
        Self::Filters,
        Self::Repositories,
        Self::Maintenance,
        Self::Tools,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Filters => "FILTERS",
            Self::Repositories => "REPOSITORIES",
            Self::Maintenance => "MAINTENANCE",
            Self::Tools => "TOOLS",
        }
    }

    /// Human name for the View menu's switch rows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Filters => "Filters",
            Self::Repositories => "Repositories",
            Self::Maintenance => "Maintenance",
            Self::Tools => "Tools",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Filters => 0,
            Self::Repositories => 1,
            Self::Maintenance => 2,
            Self::Tools => 3,
        }
    }
}

/// One collapsible section: a clickable header (disclosure triangle +
/// uppercase title) over a `gtk::Revealer` holding the content. The
/// whole thing is one `gtk::Box` so View-menu toggles can hide a section
/// wholesale via `visible`, independent of its expanded state.
struct SectionWidgets {
    container: gtk::Box,
    revealer: gtk::Revealer,
    triangle: gtk::Label,
}

struct Inner {
    widget: gtk::Box,
    preset_lb: gtk::ListBox,
    custom_filters: Rc<RefCell<CustomFilters>>,
    repo_lb: gtk::ListBox,
    /// Row `i + 1` of `repo_lb` corresponds to `repo_names[i]`; row 0
    /// is always the fixed "All Repositories" row. Holds only the
    /// currently *displayed* repos (stales excluded while hidden).
    repo_names: RefCell<Vec<String>>,
    /// Every known repo as (url, stale). Stale = a package origin not
    /// configured in any xbps.d conf file.
    all_repos: RefCell<Vec<(String, bool)>>,
    show_stale: std::cell::Cell<bool>,
    /// User-chosen display names, keyed by repository URL — right-click
    /// a repository row to set one.
    display_names: RefCell<RepoNames>,
    on_filter_changed: FilterChangedCbs,
    on_repository_changed: RepositoryChangedCbs,
    on_action: ActionCbs,
    sections: [SectionWidgets; 4],
}

fn repo_display_text(inner: &Inner, url: &str) -> String {
    inner
        .display_names
        .borrow()
        .get(url)
        .map_or_else(|| display_repo(url).to_string(), str::to_string)
}

#[derive(Clone)]
pub struct FilterSidebar {
    inner: Rc<Inner>,
}

/// Builds one collapsible section shell. Clicking the header flips the
/// revealer and the disclosure triangle. Content is appended by the
/// caller via the returned revealer's child box.
fn build_section(title: &str, content: &impl IsA<gtk::Widget>) -> SectionWidgets {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header.set_margin_top(8);
    header.set_margin_bottom(2);
    header.add_css_class("section-header");

    let triangle = gtk::Label::new(Some("\u{25be}")); // ▾
    triangle.set_width_chars(2);
    header.append(&triangle);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    header.append(&title_label);

    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_reveal_child(true);
    revealer.set_child(Some(content));

    {
        let gesture = gtk::GestureClick::new();
        let revealer = revealer.clone();
        let triangle = triangle.clone();
        gesture.connect_released(move |_, _, _, _| {
            let expand = !revealer.reveals_child();
            revealer.set_reveal_child(expand);
            triangle.set_text(if expand { "\u{25be}" } else { "\u{25b8}" });
        });
        header.add_controller(gesture);
    }

    container.append(&header);
    container.append(&revealer);

    SectionWidgets {
        container,
        revealer,
        triangle,
    }
}

/// An action row for the MAINTENANCE / TOOLS sections. Reuses the filter
/// rows' icon+label look; activation is handled by the enclosing
/// ListBox's `row-activated`.
fn make_action_row(icon: &str, label: &str) -> gtk::ListBoxRow {
    make_row(icon, label)
}

fn make_row(icon: &str, label: &str) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.set_margin_start(8);
    row_box.set_margin_end(8);
    row_box.set_margin_top(5);
    row_box.set_margin_bottom(5);
    row_box.append(&gtk::Image::from_icon_name(icon));
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    row_box.append(&l);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

/// Repository rows have no natural icon (repos are arbitrary
/// user-configured URIs/paths) and can be long, so this variant skips
/// the icon and ellipsizes instead.
fn make_text_row(label: &str) -> gtk::ListBoxRow {
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    l.set_margin_start(8);
    l.set_margin_end(8);
    l.set_margin_top(5);
    l.set_margin_bottom(5);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&l));
    row
}

/// A repository row: same look as `make_text_row`, plus a right-click
/// gesture opening a rename dialog for `url` and a tooltip showing the
/// full URL (the visible text may be a custom name or the
/// scheme-stripped URL, either of which can be a truncated/altered
/// view of it).
fn build_repo_row(inner: &Rc<Inner>, url: String, stale: bool) -> gtk::ListBoxRow {
    let l = gtk::Label::new(Some(&repo_display_text(inner, &url)));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    l.set_margin_start(8);
    l.set_margin_end(8);
    l.set_margin_top(5);
    l.set_margin_bottom(5);
    if stale {
        l.add_css_class("dim-label");
        l.set_tooltip_text(Some(&format!(
            "{url}\nNot currently configured — packages were installed from it in the past"
        )));
    } else {
        l.set_tooltip_text(Some(&url));
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&l));

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
    let inner = inner.clone();
    let label = l.clone();
    gesture.connect_pressed(move |g, _n_press, _x, _y| {
        let Some(widget) = g.widget() else { return };
        let root = widget.root().and_downcast::<gtk::Window>();
        show_rename_dialog(root, &inner, url.clone(), &label);
    });
    l.add_controller(gesture);

    row
}

/// Lets the user set (or clear) a custom display name for `url`,
/// updating `label` and persisted storage immediately on Save/Reset.
fn show_rename_dialog(
    parent: Option<gtk::Window>,
    inner: &Rc<Inner>,
    url: String,
    label: &gtk::Label,
) {
    let (dlg, outer) = modal_window("Rename Repository", parent.as_ref(), false, (380, -1), 10);

    let url_label = gtk::Label::new(Some(&url));
    url_label.set_xalign(0.0);
    url_label.set_wrap(true);
    url_label.set_selectable(true);
    url_label.add_css_class("dim-label");
    outer.append(&url_label);

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some(display_repo(&url)));
    if let Some(current) = inner.display_names.borrow().get(&url) {
        entry.set_text(current);
    }
    entry.set_activates_default(true);
    outer.append(&entry);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(4);
    let reset_btn = gtk::Button::with_label("Reset to Default");
    let cancel_btn = gtk::Button::with_label("Cancel");
    let save_btn = gtk::Button::with_label("Save");
    save_btn.add_css_class("suggested-action");
    btn_box.append(&reset_btn);
    btn_box.append(&cancel_btn);
    btn_box.append(&save_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&save_btn));

    {
        let inner = inner.clone();
        let url = url.clone();
        let label = label.clone();
        let dlg = dlg.clone();
        reset_btn.connect_clicked(move |_| {
            inner.display_names.borrow_mut().set(&url, "");
            label.set_text(display_repo(&url));
            dlg.destroy();
        });
    }
    {
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| dlg.destroy());
    }
    {
        let inner = inner.clone();
        let label = label.clone();
        let entry = entry.clone();
        let dlg = dlg.clone();
        save_btn.connect_clicked(move |_| {
            inner.display_names.borrow_mut().set(&url, &entry.text());
            label.set_text(&repo_display_text(&inner, &url));
            dlg.destroy();
        });
    }

    present_focused(&dlg, &entry);
}

/// Rebuilds the custom-filter rows below the fixed presets after an add/
/// rename/remove in the "Edit Custom Filters…" dialog. Mirrors
/// `set_available_repositories`'s snapshot-reselect approach: if the
/// previously-selected custom filter still exists (by name — indices
/// shift on add/remove), the selection carries over; otherwise falls
/// back to "All" (also covers the deleted-while-active case, and the
/// renaming-the-active-filter wart called out in the plan).
fn refresh_custom_rows(inner: &Rc<Inner>) {
    let previously_selected = inner
        .preset_lb
        .selected_row()
        .map(|r| r.index())
        .filter(|&i| i >= NUM_PRESET_ROWS)
        .and_then(|i| {
            inner
                .custom_filters
                .borrow()
                .list()
                .get((i - NUM_PRESET_ROWS) as usize)
                .map(|f| f.name.clone())
        });

    while let Some(row) = inner.preset_lb.row_at_index(NUM_PRESET_ROWS) {
        inner.preset_lb.remove(&row);
    }
    for f in inner.custom_filters.borrow().list() {
        inner.preset_lb.append(&make_text_row(&f.name));
    }
    inner.preset_lb.invalidate_headers();

    let restore_index = previously_selected
        .as_ref()
        .and_then(|name| {
            inner
                .custom_filters
                .borrow()
                .list()
                .iter()
                .position(|f| &f.name == name)
        })
        .map_or(0, |pos| pos as i32 + NUM_PRESET_ROWS);

    if let Some(row) = inner.preset_lb.row_at_index(restore_index) {
        // A restored custom row is always a freshly-recreated widget, so
        // this reliably fires "row-selected" even when the logical
        // selection (by name) didn't change. Falling back to row 0
        // ("All") when it was already selected is a harmless no-op —
        // nothing about the active filter changed either way.
        inner.preset_lb.select_row(Some(&row));
    }
}

impl FilterSidebar {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_width_request(190);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);

        let inner_box = gtk::Box::new(gtk::Orientation::Vertical, 0);

        // ── FILTERS ──
        let preset_lb = gtk::ListBox::new();
        preset_lb.set_selection_mode(gtk::SelectionMode::Single);
        preset_lb.add_css_class("navigation-sidebar");

        preset_lb.append(&make_row("view-list-symbolic", "All"));
        preset_lb.append(&make_row("object-select-symbolic", "Installed"));
        preset_lb.append(&make_row("list-remove-symbolic", "Not Installed"));
        preset_lb.append(&make_row(
            "software-update-available-symbolic",
            "Upgradable",
        ));
        preset_lb.append(&make_row("media-playback-pause-symbolic", "On Hold"));
        preset_lb.append(&make_row("starred-symbolic", "Marked"));
        preset_lb.append(&make_row("edit-clear-symbolic", "Orphaned"));

        // Separator above the first custom-filter row, if any — cleared
        // for every other row so it doesn't linger on stale rows after a
        // `refresh_custom_rows` shrinks the list.
        preset_lb.set_header_func(move |row, _before| {
            if row.index() == NUM_PRESET_ROWS {
                row.set_header(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
            } else {
                row.set_header(None::<&gtk::Widget>);
            }
        });

        let custom_filters = Rc::new(RefCell::new(CustomFilters::load()));
        for f in custom_filters.borrow().list() {
            preset_lb.append(&make_text_row(&f.name));
        }

        let edit_filters_lb = gtk::ListBox::new();
        edit_filters_lb.set_selection_mode(gtk::SelectionMode::None);
        edit_filters_lb.add_css_class("navigation-sidebar");
        edit_filters_lb.append(&make_action_row(
            "applications-system-symbolic",
            "Edit Custom Filters\u{2026}",
        ));

        let filters_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        filters_content.append(&preset_lb);
        filters_content.append(&edit_filters_lb);

        let filters_section = build_section(Section::Filters.title(), &filters_content);
        inner_box.append(&filters_section.container);

        // ── REPOSITORIES ──
        // Populated later via `set_available_repositories` once a load
        // has actually happened — starts with just "All Repositories".
        let repo_lb = gtk::ListBox::new();
        repo_lb.set_selection_mode(gtk::SelectionMode::Single);
        repo_lb.add_css_class("navigation-sidebar");
        repo_lb.append(&make_text_row("All Repositories"));

        let repos_section = build_section(Section::Repositories.title(), &repo_lb);
        inner_box.append(&repos_section.container);

        // ── MAINTENANCE ── (icon rule: trash = remove, wrench-ish
        // utilities = reconfigure; the gear stays reserved for "manage")
        let maint_lb = gtk::ListBox::new();
        maint_lb.set_selection_mode(gtk::SelectionMode::None);
        maint_lb.add_css_class("navigation-sidebar");
        let maint_actions: &[(&str, &str, SidebarAction)] = &[
            (
                "software-update-available-symbolic",
                "Full System Upgrade\u{2026}",
                SidebarAction::FullUpgrade,
            ),
            (
                "user-trash-symbolic",
                "Remove Orphans\u{2026}",
                SidebarAction::RemoveOrphans,
            ),
            (
                "edit-clear-all-symbolic",
                "Clean Package Cache",
                SidebarAction::CleanCache,
            ),
            (
                "security-high-symbolic",
                "Verify Package Database",
                SidebarAction::VerifyDb,
            ),
            (
                "applications-utilities-symbolic",
                "Reconfigure Packages\u{2026}",
                SidebarAction::Reconfigure,
            ),
            (
                "application-x-firmware-symbolic",
                "Purge Old Kernels\u{2026}",
                SidebarAction::PurgeKernels,
            ),
        ];
        for (icon, label, _) in maint_actions {
            maint_lb.append(&make_action_row(icon, label));
        }

        let maint_section = build_section(Section::Maintenance.title(), &maint_lb);
        inner_box.append(&maint_section.container);

        // ── TOOLS ──
        let tools_lb = gtk::ListBox::new();
        tools_lb.set_selection_mode(gtk::SelectionMode::None);
        tools_lb.add_css_class("navigation-sidebar");
        let tools_actions: &[(&str, &str, SidebarAction)] = &[
            (
                "edit-find-symbolic",
                "Find Owning Package\u{2026}",
                SidebarAction::FindOwner,
            ),
            (
                "object-flip-horizontal-symbolic",
                "Alternatives\u{2026}",
                SidebarAction::Alternatives,
            ),
            (
                "network-server-symbolic",
                "Manage Repositories\u{2026}",
                SidebarAction::ManageRepos,
            ),
            (
                "document-open-recent-symbolic",
                "Transaction History\u{2026}",
                SidebarAction::History,
            ),
        ];
        for (icon, label, _) in tools_actions {
            tools_lb.append(&make_action_row(icon, label));
        }

        let tools_section = build_section(Section::Tools.title(), &tools_lb);
        inner_box.append(&tools_section.container);

        scroll.set_child(Some(&inner_box));
        widget.append(&scroll);

        let inner = Rc::new(Inner {
            widget,
            preset_lb: preset_lb.clone(),
            custom_filters,
            repo_lb: repo_lb.clone(),
            repo_names: RefCell::new(Vec::new()),
            all_repos: RefCell::new(Vec::new()),
            show_stale: std::cell::Cell::new(true),
            display_names: RefCell::new(RepoNames::load()),
            on_filter_changed: RefCell::new(Vec::new()),
            on_repository_changed: RefCell::new(Vec::new()),
            on_action: RefCell::new(Vec::new()),
            sections: [filters_section, repos_section, maint_section, tools_section],
        });

        // Action row dispatch: each action ListBox row maps by index to
        // its section's action table (same index-mapping approach the
        // preset list already uses with FilterMode::from_row_index).
        {
            let actions: Vec<SidebarAction> = maint_actions.iter().map(|&(_, _, a)| a).collect();
            let inner_weak = Rc::downgrade(&inner);
            maint_lb.connect_row_activated(move |_, row| {
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                if let Some(&action) = actions.get(row.index().max(0) as usize) {
                    for cb in inner.on_action.borrow().iter() {
                        cb(action);
                    }
                }
            });
        }
        {
            let actions: Vec<SidebarAction> = tools_actions.iter().map(|&(_, _, a)| a).collect();
            let inner_weak = Rc::downgrade(&inner);
            tools_lb.connect_row_activated(move |_, row| {
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                if let Some(&action) = actions.get(row.index().max(0) as usize) {
                    for cb in inner.on_action.borrow().iter() {
                        cb(action);
                    }
                }
            });
        }
        {
            let inner_weak = Rc::downgrade(&inner);
            edit_filters_lb.connect_row_activated(move |lb, _| {
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                let root = lb.root().and_downcast::<gtk::Window>();
                let inner_for_refresh = inner.clone();
                custom_filters_dialog::show(root, inner.custom_filters.clone(), move || {
                    refresh_custom_rows(&inner_for_refresh);
                });
            });
        }

        {
            let inner_weak = Rc::downgrade(&inner);
            preset_lb.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                let idx = row.index();
                let filter = if idx < NUM_PRESET_ROWS {
                    ActiveFilter::Preset(FilterMode::from_row_index(idx))
                } else {
                    let custom_idx = (idx - NUM_PRESET_ROWS) as usize;
                    match inner.custom_filters.borrow().list().get(custom_idx) {
                        Some(f) => ActiveFilter::Custom {
                            name: f.name.clone(),
                            patterns: f.patterns.clone(),
                        },
                        None => ActiveFilter::Preset(FilterMode::All),
                    }
                };
                for cb in inner.on_filter_changed.borrow().iter() {
                    cb(filter.clone());
                }
            });
        }

        // Selects "All" visually. NOTE: exactly like the original, this
        // fires "filter-changed" synchronously during construction,
        // before the caller has had a chance to call
        // `connect_filter_changed` — so that first emission is
        // silently dropped. Harmless only because `PackageList`'s own
        // default (`FilterMode::All`) already matches; see the same
        // caveat in the original ui/filter_sidebar.c.
        if let Some(row0) = preset_lb.row_at_index(0) {
            preset_lb.select_row(Some(&row0));
        }

        {
            let inner_weak = Rc::downgrade(&inner);
            repo_lb.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                let idx = row.index();
                let repo = if idx <= 0 {
                    None
                } else {
                    inner.repo_names.borrow().get((idx - 1) as usize).cloned()
                };
                for cb in inner.on_repository_changed.borrow().iter() {
                    cb(repo.clone());
                }
            });
        }
        // Same first-emission caveat as the preset list above: fires
        // during construction, before the caller can connect — harmless
        // since `PackageList`'s own default (no repository filter)
        // already matches "All Repositories".
        if let Some(row0) = repo_lb.row_at_index(0) {
            repo_lb.select_row(Some(&row0));
        }

        Self { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_filter_changed(&self, f: impl Fn(ActiveFilter) + 'static) {
        self.inner.on_filter_changed.borrow_mut().push(Box::new(f));
    }

    pub fn connect_repository_changed(&self, f: impl Fn(Option<String>) + 'static) {
        self.inner
            .on_repository_changed
            .borrow_mut()
            .push(Box::new(f));
    }

    /// Fires when an action row (MAINTENANCE / TOOLS / Manage
    /// Repositories) is activated.
    pub fn connect_action(&self, f: impl Fn(SidebarAction) + 'static) {
        self.inner.on_action.borrow_mut().push(Box::new(f));
    }

    /// The whole section (header + content) — hidden/shown by the View
    /// menu's switches, independent of collapse state.
    pub fn section_widget(&self, section: Section) -> &gtk::Box {
        &self.inner.sections[section.index()].container
    }

    pub fn is_expanded(&self, section: Section) -> bool {
        self.inner.sections[section.index()]
            .revealer
            .reveals_child()
    }

    pub fn set_expanded(&self, section: Section, expanded: bool) {
        let widgets = &self.inner.sections[section.index()];
        widgets.revealer.set_reveal_child(expanded);
        widgets
            .triangle
            .set_text(if expanded { "\u{25be}" } else { "\u{25b8}" });
    }

    /// Rebuilds the repository rows from a freshly-loaded package set.
    /// `configured` = URLs present in xbps.d conf files; anything else
    /// is marked stale. Selection carries over by name across the
    /// rebuild instead of silently resetting to "All".
    pub fn set_available_repositories(
        &self,
        mut repos: Vec<String>,
        configured: &std::collections::HashSet<String>,
    ) {
        repos.sort();
        repos.dedup();
        *self.inner.all_repos.borrow_mut() = repos
            .into_iter()
            .map(|url| {
                let stale = !configured.contains(&url);
                (url, stale)
            })
            .collect();
        rebuild_repo_rows(&self.inner);
    }

    pub fn show_stale_repositories(&self) -> bool {
        self.inner.show_stale.get()
    }

    pub fn set_show_stale_repositories(&self, show: bool) {
        if self.inner.show_stale.replace(show) != show {
            rebuild_repo_rows(&self.inner);
        }
    }
}

fn rebuild_repo_rows(inner: &Rc<Inner>) {
    let previously_selected = inner
        .repo_lb
        .selected_row()
        .map(|r| r.index())
        .filter(|&i| i > 0)
        .and_then(|i| inner.repo_names.borrow().get((i - 1) as usize).cloned());

    while let Some(child) = inner.repo_lb.first_child() {
        inner.repo_lb.remove(&child);
    }
    inner.repo_lb.append(&make_text_row("All Repositories"));

    let show_stale = inner.show_stale.get();
    let displayed: Vec<(String, bool)> = inner
        .all_repos
        .borrow()
        .iter()
        .filter(|(_, stale)| show_stale || !stale)
        .cloned()
        .collect();
    for (url, stale) in &displayed {
        inner
            .repo_lb
            .append(&build_repo_row(inner, url.clone(), *stale));
    }
    *inner.repo_names.borrow_mut() = displayed.into_iter().map(|(url, _)| url).collect();

    let restore_index = previously_selected
        .as_ref()
        .and_then(|name| inner.repo_names.borrow().iter().position(|r| r == name))
        .map_or(0, |pos| pos as i32 + 1);

    if let Some(row) = inner.repo_lb.row_at_index(restore_index) {
        // Every row was just recreated, so nothing is selected yet —
        // this always fires "row-selected", which notifies listeners
        // with the correct value (restored, or reset to "All", which
        // also covers hiding the currently-selected stale repo).
        inner.repo_lb.select_row(Some(&row));
    }
}
