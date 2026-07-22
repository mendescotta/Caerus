//! "Edit Custom Filters…" editor: a two-pane master/detail window, left
//! = the list of saved filters (add/delete), right = the selected
//! filter's exclusion patterns (add/delete) plus a rename control.
//! Every mutation saves immediately (`CustomFilters` saves on every
//! call) and calls back into the sidebar so the live filter rows and,
//! if the edited filter is currently active, the package list's filter
//! predicate stay in sync while this dialog is still open.

use crate::backend::custom_filters::{sanitize, CustomFilters, FilterKind};
use crate::ui::dialog_util::{close_button, modal_window, present_focused};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const EXCLUDE_CAPTION: &str = "Packages matching any pattern below are hidden. \u{2018}*\u{2019} \
     matches any run of characters (lib*, *-devel); plain text matches anywhere in the name. \
     Case-insensitive.";
const INCLUDE_ONLY_CAPTION: &str = "Only packages matching a pattern below are shown; \
     everything else is hidden. With no patterns yet, nothing is shown. \u{2018}*\u{2019} \
     matches any run of characters (lib*, *-devel); plain text matches anywhere in the name. \
     Case-insensitive.";

struct Inner {
    dlg: gtk::Window,
    custom_filters: Rc<RefCell<CustomFilters>>,
    filter_lb: gtk::ListBox,
    /// Row `i` of `filter_lb` names `filter_names[i]` — snapshotted at
    /// each `refresh_filters` since list order can shift on add/remove.
    filter_names: RefCell<Vec<String>>,
    selected: RefCell<Option<String>>,
    detail_heading: gtk::Label,
    rename_btn: gtk::Button,
    mode_exclude_btn: gtk::ToggleButton,
    mode_include_btn: gtk::ToggleButton,
    syntax_caption: gtk::Label,
    /// Set while `refresh_detail` is driving the mode toggle buttons
    /// programmatically, so their own "toggled" handlers (which write
    /// through to `custom_filters`) know to ignore that change instead
    /// of treating it as a user edit.
    refreshing_mode: Cell<bool>,
    pattern_lb: gtk::ListBox,
    new_pattern_entry: gtk::Entry,
    new_pattern_add: gtk::Button,
    on_changed: Box<dyn Fn()>,
}

fn refresh_filters(inner: &Rc<Inner>) {
    let previously_selected = inner.selected.borrow().clone();

    while let Some(child) = inner.filter_lb.first_child() {
        inner.filter_lb.remove(&child);
    }

    let names: Vec<String> = inner
        .custom_filters
        .borrow()
        .list()
        .iter()
        .map(|f| f.name.clone())
        .collect();

    for name in &names {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);

        let l = gtk::Label::new(Some(name));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row_box.append(&l);

        let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
        del_btn.set_tooltip_text(Some("Delete this filter"));
        del_btn.add_css_class("flat");
        {
            let inner = inner.clone();
            let name = name.clone();
            del_btn.connect_clicked(move |_| {
                inner.custom_filters.borrow_mut().remove(&name);
                if inner.selected.borrow().as_deref() == Some(name.as_str()) {
                    *inner.selected.borrow_mut() = None;
                }
                refresh_filters(&inner);
                refresh_detail(&inner);
                (inner.on_changed)();
            });
        }
        row_box.append(&del_btn);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        inner.filter_lb.append(&row);
    }

    *inner.filter_names.borrow_mut() = names.clone();

    match previously_selected
        .as_ref()
        .and_then(|name| names.iter().position(|n| n == name))
    {
        Some(idx) => {
            if let Some(row) = inner.filter_lb.row_at_index(idx as i32) {
                inner.filter_lb.select_row(Some(&row));
            }
        }
        None => {
            inner.filter_lb.select_row(None::<&gtk::ListBoxRow>);
            *inner.selected.borrow_mut() = None;
        }
    }
}

/// Rebuilds the right-hand pane for whichever filter (if any) is
/// currently selected in `filter_lb`.
fn refresh_detail(inner: &Rc<Inner>) {
    let selected = inner.selected.borrow().clone();

    while let Some(child) = inner.pattern_lb.first_child() {
        inner.pattern_lb.remove(&child);
    }

    let Some(name) = selected else {
        inner.detail_heading.set_text("Select a filter");
        inner.rename_btn.set_sensitive(false);
        inner.mode_exclude_btn.set_sensitive(false);
        inner.mode_include_btn.set_sensitive(false);
        inner.new_pattern_entry.set_sensitive(false);
        inner.new_pattern_add.set_sensitive(false);
        return;
    };

    inner.detail_heading.set_text(&name);
    inner.rename_btn.set_sensitive(true);
    inner.mode_exclude_btn.set_sensitive(true);
    inner.mode_include_btn.set_sensitive(true);
    inner.new_pattern_entry.set_sensitive(true);
    inner.new_pattern_add.set_sensitive(true);

    let kind = inner
        .custom_filters
        .borrow()
        .get(&name)
        .map_or(FilterKind::Exclude, |f| f.kind);
    inner.refreshing_mode.set(true);
    inner
        .mode_exclude_btn
        .set_active(kind == FilterKind::Exclude);
    inner
        .mode_include_btn
        .set_active(kind == FilterKind::IncludeOnly);
    inner.refreshing_mode.set(false);
    inner.syntax_caption.set_text(match kind {
        FilterKind::Exclude => EXCLUDE_CAPTION,
        FilterKind::IncludeOnly => INCLUDE_ONLY_CAPTION,
    });

    let patterns = inner
        .custom_filters
        .borrow()
        .get(&name)
        .map(|f| f.patterns.clone())
        .unwrap_or_default();

    for pattern in patterns {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);

        let l = gtk::Label::new(Some(&pattern));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row_box.append(&l);

        let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
        del_btn.set_tooltip_text(Some("Remove this pattern"));
        del_btn.add_css_class("flat");
        {
            let inner = inner.clone();
            let name = name.clone();
            let pattern = pattern.clone();
            del_btn.connect_clicked(move |_| {
                inner
                    .custom_filters
                    .borrow_mut()
                    .remove_pattern(&name, &pattern);
                refresh_detail(&inner);
                (inner.on_changed)();
            });
        }
        row_box.append(&del_btn);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        inner.pattern_lb.append(&row);
    }
}

fn add_new_filter(inner: &Rc<Inner>, entry: &gtk::Entry) {
    let Some(name) = sanitize(&entry.text()) else {
        return;
    };
    if inner.custom_filters.borrow_mut().add(&name) {
        entry.set_text("");
        *inner.selected.borrow_mut() = Some(name);
        refresh_filters(inner);
        refresh_detail(inner);
        (inner.on_changed)();
    }
}

fn add_new_pattern(inner: &Rc<Inner>, entry: &gtk::Entry) {
    let Some(pattern) = sanitize(&entry.text()) else {
        return;
    };
    let Some(name) = inner.selected.borrow().clone() else {
        return;
    };
    if inner
        .custom_filters
        .borrow_mut()
        .add_pattern(&name, &pattern)
    {
        entry.set_text("");
        refresh_detail(inner);
        (inner.on_changed)();
    }
}

/// Prompts for a new name for `old` and applies the rename on Save.
/// Same shape as `filter_sidebar::show_rename_dialog` (label + entry +
/// Cancel/Save), minus the "reset to default" option that only makes
/// sense for repository display names. A rejected rename (invalid or
/// already taken) is a silent no-op — the dialog just stays open,
/// matching how `repo_manager`'s add-repository entry rejects invalid
/// input without inline feedback.
fn show_rename_filter_dialog(parent: Option<gtk::Window>, inner: &Rc<Inner>, old: String) {
    let (dlg, outer) = modal_window("Rename Filter", parent.as_ref(), false, (360, -1), 10);

    let entry = gtk::Entry::new();
    entry.set_text(&old);
    entry.set_activates_default(true);
    outer.append(&entry);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(4);
    let cancel_btn = gtk::Button::with_label("Cancel");
    let save_btn = gtk::Button::with_label("Save");
    save_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&save_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&save_btn));

    {
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| dlg.destroy());
    }
    {
        let inner = inner.clone();
        let dlg = dlg.clone();
        let entry = entry.clone();
        let old = old.clone();
        save_btn.connect_clicked(move |_| {
            let Some(new_name) = sanitize(&entry.text()) else {
                return;
            };
            if inner.custom_filters.borrow_mut().rename(&old, &new_name) {
                if inner.selected.borrow().as_deref() == Some(old.as_str()) {
                    *inner.selected.borrow_mut() = Some(new_name);
                }
                refresh_filters(&inner);
                refresh_detail(&inner);
                (inner.on_changed)();
                dlg.destroy();
            }
        });
    }

    present_focused(&dlg, &entry);
}

/// Opens the editor. `on_changed` fires after every save-worthy mutation
/// (add/rename/remove filter, add/remove pattern) — the sidebar passes a
/// closure that rebuilds its own custom-filter rows so they stay live
/// while this dialog is open.
pub fn show(
    parent: Option<gtk::Window>,
    custom_filters: Rc<RefCell<CustomFilters>>,
    on_changed: impl Fn() + 'static,
) {
    let (dlg, outer) = modal_window("Custom Filters", parent.as_ref(), true, (560, 420), 8);

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_vexpand(true);
    paned.set_wide_handle(true);
    paned.set_position(190);

    // ── Left pane: filter list ──
    let left = gtk::Box::new(gtk::Orientation::Vertical, 4);
    left.set_size_request(160, -1);

    let filter_scroll = gtk::ScrolledWindow::new();
    filter_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    filter_scroll.set_vexpand(true);
    let filter_lb = gtk::ListBox::new();
    filter_lb.set_selection_mode(gtk::SelectionMode::Single);
    filter_scroll.set_child(Some(&filter_lb));
    left.append(&filter_scroll);

    let new_filter_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let new_filter_entry = gtk::Entry::new();
    new_filter_entry.set_placeholder_text(Some("Filter name"));
    new_filter_entry.set_hexpand(true);
    let new_filter_add = gtk::Button::from_icon_name("list-add-symbolic");
    new_filter_add.set_tooltip_text(Some("Add filter"));
    new_filter_row.append(&new_filter_entry);
    new_filter_row.append(&new_filter_add);
    left.append(&new_filter_row);

    paned.set_start_child(Some(&left));

    // ── Right pane: selected filter's patterns ──
    let right = gtk::Box::new(gtk::Orientation::Vertical, 6);
    right.set_hexpand(true);

    let heading_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let detail_heading = gtk::Label::new(Some("Select a filter"));
    detail_heading.set_xalign(0.0);
    detail_heading.set_hexpand(true);
    detail_heading.add_css_class("heading");
    let rename_btn = gtk::Button::with_label("Rename\u{2026}");
    rename_btn.set_sensitive(false);
    heading_row.append(&detail_heading);
    heading_row.append(&rename_btn);
    right.append(&heading_row);

    // Exclude/IncludeOnly mode: a two-way segmented toggle via GTK4's
    // `.linked` style class, matching the button clusters used
    // elsewhere in the app (see `detail_pane::linked_cluster`).
    // `set_group` makes the pair mutually exclusive, like radio
    // buttons.
    let mode_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    mode_row.add_css_class("linked");
    let mode_exclude_btn = gtk::ToggleButton::with_label("Hide Matching");
    let mode_include_btn = gtk::ToggleButton::with_label("Show Only Matching");
    mode_include_btn.set_group(Some(&mode_exclude_btn));
    mode_exclude_btn.set_sensitive(false);
    mode_include_btn.set_sensitive(false);
    mode_row.append(&mode_exclude_btn);
    mode_row.append(&mode_include_btn);
    right.append(&mode_row);

    let syntax_caption = gtk::Label::new(Some(EXCLUDE_CAPTION));
    syntax_caption.set_xalign(0.0);
    syntax_caption.set_wrap(true);
    syntax_caption.add_css_class("dim-label");
    right.append(&syntax_caption);

    let pattern_scroll = gtk::ScrolledWindow::new();
    pattern_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    pattern_scroll.set_vexpand(true);
    let pattern_lb = gtk::ListBox::new();
    pattern_lb.set_selection_mode(gtk::SelectionMode::None);
    pattern_scroll.set_child(Some(&pattern_lb));
    right.append(&pattern_scroll);

    let new_pattern_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let new_pattern_entry = gtk::Entry::new();
    new_pattern_entry.set_placeholder_text(Some("lib*, *-devel, or plain text"));
    new_pattern_entry.set_hexpand(true);
    new_pattern_entry.set_sensitive(false);
    let new_pattern_add = gtk::Button::from_icon_name("list-add-symbolic");
    new_pattern_add.set_tooltip_text(Some("Add pattern"));
    new_pattern_add.set_sensitive(false);
    new_pattern_row.append(&new_pattern_entry);
    new_pattern_row.append(&new_pattern_add);
    right.append(&new_pattern_row);

    paned.set_end_child(Some(&right));
    outer.append(&paned);

    close_button(&outer, &dlg, 8);

    let inner = Rc::new(Inner {
        dlg: dlg.clone(),
        custom_filters,
        filter_lb: filter_lb.clone(),
        filter_names: RefCell::new(Vec::new()),
        selected: RefCell::new(None),
        detail_heading,
        rename_btn: rename_btn.clone(),
        mode_exclude_btn: mode_exclude_btn.clone(),
        mode_include_btn: mode_include_btn.clone(),
        syntax_caption: syntax_caption.clone(),
        refreshing_mode: Cell::new(false),
        pattern_lb,
        new_pattern_entry: new_pattern_entry.clone(),
        new_pattern_add: new_pattern_add.clone(),
        on_changed: Box::new(on_changed),
    });

    {
        let inner = inner.clone();
        mode_exclude_btn.connect_toggled(move |btn| {
            if inner.refreshing_mode.get() || !btn.is_active() {
                return;
            }
            let Some(name) = inner.selected.borrow().clone() else {
                return;
            };
            inner
                .custom_filters
                .borrow_mut()
                .set_kind(&name, FilterKind::Exclude);
            inner.syntax_caption.set_text(EXCLUDE_CAPTION);
            (inner.on_changed)();
        });
    }
    {
        let inner = inner.clone();
        mode_include_btn.connect_toggled(move |btn| {
            if inner.refreshing_mode.get() || !btn.is_active() {
                return;
            }
            let Some(name) = inner.selected.borrow().clone() else {
                return;
            };
            inner
                .custom_filters
                .borrow_mut()
                .set_kind(&name, FilterKind::IncludeOnly);
            inner.syntax_caption.set_text(INCLUDE_ONLY_CAPTION);
            (inner.on_changed)();
        });
    }

    {
        let inner_weak = Rc::downgrade(&inner);
        filter_lb.connect_row_selected(move |_, row| {
            let Some(inner) = inner_weak.upgrade() else {
                return;
            };
            let name = row.and_then(|r| {
                let idx = r.index();
                if idx < 0 {
                    None
                } else {
                    inner.filter_names.borrow().get(idx as usize).cloned()
                }
            });
            *inner.selected.borrow_mut() = name;
            refresh_detail(&inner);
        });
    }

    {
        let inner = inner.clone();
        let entry = new_filter_entry.clone();
        new_filter_add.connect_clicked(move |_| add_new_filter(&inner, &entry));
    }
    {
        let inner = inner.clone();
        let entry = new_filter_entry.clone();
        new_filter_entry.connect_activate(move |_| add_new_filter(&inner, &entry));
    }

    {
        let inner = inner.clone();
        let entry = new_pattern_entry.clone();
        new_pattern_add.connect_clicked(move |_| add_new_pattern(&inner, &entry));
    }
    {
        let inner = inner.clone();
        let entry = new_pattern_entry.clone();
        new_pattern_entry.connect_activate(move |_| add_new_pattern(&inner, &entry));
    }

    {
        let inner = inner.clone();
        rename_btn.connect_clicked(move |_| {
            let Some(old) = inner.selected.borrow().clone() else {
                return;
            };
            let root = inner.dlg.clone();
            show_rename_filter_dialog(Some(root), &inner, old);
        });
    }

    refresh_filters(&inner);
    refresh_detail(&inner);

    present_focused(&dlg, &new_filter_entry);
}
