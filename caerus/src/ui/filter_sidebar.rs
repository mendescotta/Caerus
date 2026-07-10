//! Preset filter sidebar. Rust translation of ui/filter_sidebar.{h,c}
//! (built directly in code here rather than from a GtkBuilder .ui file
//! — see the top-level README for why).
//!
//! Row order must stay in sync with `FilterMode::from_row_index` in
//! backend/package.rs, exactly like the original's comment about
//! `on_preset_selected()`'s use of `gtk_list_box_row_get_index()`.

use crate::backend::package::FilterMode;
use crate::backend::repo_names::{display_repo, RepoNames};
use crate::ui::dialog_util::{modal_window, present_focused};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type FilterChangedCbs = RefCell<Vec<Box<dyn Fn(FilterMode)>>>;
type RepositoryChangedCbs = RefCell<Vec<Box<dyn Fn(Option<String>)>>>;

struct Inner {
    widget: gtk::Box,
    repo_lb: gtk::ListBox,
    /// Row `i + 1` of `repo_lb` corresponds to `repo_names[i]`; row 0
    /// is always the fixed "All Repositories" row.
    repo_names: RefCell<Vec<String>>,
    /// User-chosen display names, keyed by repository URL — right-click
    /// a repository row to set one.
    display_names: RefCell<RepoNames>,
    on_filter_changed: FilterChangedCbs,
    on_repository_changed: RepositoryChangedCbs,
}

fn repo_display_text(inner: &Inner, url: &str) -> String {
    inner
        .display_names
        .borrow()
        .get(url)
        .map(str::to_string)
        .unwrap_or_else(|| display_repo(url).to_string())
}

#[derive(Clone)]
pub struct FilterSidebar {
    inner: Rc<Inner>,
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
fn build_repo_row(inner: &Rc<Inner>, url: String) -> gtk::ListBoxRow {
    let l = gtk::Label::new(Some(&repo_display_text(inner, &url)));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    l.set_margin_start(8);
    l.set_margin_end(8);
    l.set_margin_top(5);
    l.set_margin_bottom(5);
    l.set_tooltip_text(Some(&url));

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
        let url = url.clone();
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

impl FilterSidebar {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_width_request(190);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);

        let inner_box = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let filter_header = gtk::Label::new(Some("FILTER"));
        filter_header.set_xalign(0.0);
        filter_header.set_margin_top(8);
        filter_header.set_margin_start(6);
        filter_header.set_margin_bottom(2);
        filter_header.add_css_class("section-header");
        inner_box.append(&filter_header);

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
        preset_lb.append(&make_row("emblem-important-symbolic", "Marked"));

        inner_box.append(&preset_lb);

        let repo_header = gtk::Label::new(Some("REPOSITORY"));
        repo_header.set_xalign(0.0);
        repo_header.set_margin_top(10);
        repo_header.set_margin_start(6);
        repo_header.set_margin_bottom(2);
        repo_header.add_css_class("section-header");
        inner_box.append(&repo_header);

        // Populated later via `set_available_repositories` once a load
        // has actually happened — the set of repositories isn't known
        // until then. Starts with just "All Repositories".
        let repo_lb = gtk::ListBox::new();
        repo_lb.set_selection_mode(gtk::SelectionMode::Single);
        repo_lb.add_css_class("navigation-sidebar");
        repo_lb.append(&make_text_row("All Repositories"));
        inner_box.append(&repo_lb);

        scroll.set_child(Some(&inner_box));
        widget.append(&scroll);

        let inner = Rc::new(Inner {
            widget,
            repo_lb: repo_lb.clone(),
            repo_names: RefCell::new(Vec::new()),
            display_names: RefCell::new(RepoNames::load()),
            on_filter_changed: RefCell::new(Vec::new()),
            on_repository_changed: RefCell::new(Vec::new()),
        });

        {
            let inner_weak = Rc::downgrade(&inner);
            preset_lb.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };
                let mode = FilterMode::from_row_index(row.index());
                for cb in inner.on_filter_changed.borrow().iter() {
                    cb(mode);
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

        FilterSidebar { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_filter_changed(&self, f: impl Fn(FilterMode) + 'static) {
        self.inner.on_filter_changed.borrow_mut().push(Box::new(f));
    }

    pub fn connect_repository_changed(&self, f: impl Fn(Option<String>) + 'static) {
        self.inner
            .on_repository_changed
            .borrow_mut()
            .push(Box::new(f));
    }

    /// Rebuilds the repository rows from a freshly-loaded package set.
    /// Mirrors `PackageStore`'s own mark-preservation-across-reload
    /// approach: if the previously-selected repository is still present
    /// in the new set, the selection (and therefore `PackageList`'s
    /// filter) carries over instead of silently resetting to "All" on
    /// every reload.
    pub fn set_available_repositories(&self, mut repos: Vec<String>) {
        repos.sort();
        repos.dedup();

        let previously_selected = self
            .inner
            .repo_lb
            .selected_row()
            .map(|r| r.index())
            .filter(|&i| i > 0)
            .and_then(|i| {
                self.inner
                    .repo_names
                    .borrow()
                    .get((i - 1) as usize)
                    .cloned()
            });

        while let Some(child) = self.inner.repo_lb.first_child() {
            self.inner.repo_lb.remove(&child);
        }
        self.inner
            .repo_lb
            .append(&make_text_row("All Repositories"));
        for r in &repos {
            self.inner
                .repo_lb
                .append(&build_repo_row(&self.inner, r.clone()));
        }
        *self.inner.repo_names.borrow_mut() = repos;

        let restore_index = previously_selected
            .as_ref()
            .and_then(|name| {
                self.inner
                    .repo_names
                    .borrow()
                    .iter()
                    .position(|r| r == name)
            })
            .map(|pos| pos as i32 + 1)
            .unwrap_or(0);

        if let Some(row) = self.inner.repo_lb.row_at_index(restore_index) {
            // Every row was just recreated, so nothing is selected yet
            // at this point — this always fires "row-selected" (via the
            // handler wired in `new()`), which notifies listeners with
            // the correct value in every case (restored or reset).
            self.inner.repo_lb.select_row(Some(&row));
        }
    }
}
