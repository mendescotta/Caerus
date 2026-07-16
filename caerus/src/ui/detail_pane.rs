//! Detail pane: action buttons + Package Information / Size Statistics
//! / Maintainer & Source frames + a Files expander + side-by-side
//! Dependencies / Reverse Dependencies lists. Rust translation of
//! `ui/detail_pane.{h,c}` (built directly in code here rather than from a
//! `GtkBuilder` .ui file).

use crate::backend::package::{Package, PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use crate::ui::remove_confirm;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Files lists can run into the thousands of entries for large
/// packages; a plain (non-virtualized) `gtk::ListBox` materializes one
/// widget per row, so this caps how many are actually shown to keep the
/// expander responsive.
const MAX_FILES_SHOWN: usize = 300;

/// A value cell in the metadata grid — plain selectable text or a
/// clickable homepage link.
enum KvValue {
    Text(String),
    Link(String),
}

/// 0.5 redesign (see the locked mockup): the old framed boxes became a
/// header row (name + version + state chip + tag chips) over one
/// unframed key/value grid grouped under SIZE / INSTALLATION / SOURCE
/// micro-headers. **A row or group without data is simply not built** —
/// no "—" dashes, no placeholder text; the state chip always shows
/// because install state is data, not absence.
struct Header {
    name: gtk::Label,
    version: gtk::Label,
    state_chip: gtk::Label,
    tags_box: gtk::Box,
    desc: gtk::Label,
    size_group: gtk::Box,
    install_group: gtk::Box,
    source_group: gtk::Box,
}

type MarkChangedCbs = RefCell<Vec<Box<dyn Fn()>>>;
type HoldRequestedCbs = RefCell<Vec<Box<dyn Fn(String, bool)>>>;
type ActionRequestedCbs = RefCell<Vec<Box<dyn Fn(String)>>>;

struct Inner {
    widget: gtk::Box,
    store: PackageStore,
    current_pkgname: RefCell<Option<String>>,
    btn_install: gtk::Button,
    btn_upgrade: gtk::Button,
    btn_remove: gtk::Button,
    btn_purge: gtk::Button,
    btn_hold: gtk::Button,
    btn_unhold: gtk::Button,
    btn_reinstall: gtk::Button,
    btn_reconfigure: gtk::Button,
    btn_download: gtk::Button,
    btn_repolock: gtk::Button,
    btn_repounlock: gtk::Button,
    btn_mark_manual: gtk::Button,
    btn_mark_auto: gtk::Button,
    btn_unmark: gtk::Button,
    more_button: gtk::MenuButton,
    more_popover: gtk::Popover,
    header: Header,
    /// Switches between a centered "Select a package…" empty page and
    /// the real content — with no selection nothing else renders at all.
    content_stack: gtk::Stack,
    /// Sizes are known synchronously but the download size can be
    /// corrected by the async extra-info reply, which rebuilds the SIZE
    /// group from these.
    install_size: std::cell::Cell<u64>,
    download_size: std::cell::Cell<u64>,
    /// Maintainer comes from the sync package data but lives in the
    /// async-rebuilt SOURCE group, so it's stashed here for the rebuild.
    current_maintainer: RefCell<String>,
    files_pill: gtk::Label,
    provides_pill: gtk::Label,
    deps_pill: gtk::Label,
    rdeps_pill: gtk::Label,
    deps_col: gtk::Box,
    rdeps_col: gtk::Box,
    provides_expander: gtk::Expander,
    deps_list: gtk::ListBox,
    rdeps_list: gtk::ListBox,
    /// The two lists' placeholder labels, retargeted per state: "Select
    /// a package" with no selection, "Loading…" while the async fetch is
    /// in flight, "No (reverse) dependencies" once an empty reply lands.
    deps_placeholder: gtk::Label,
    rdeps_placeholder: gtk::Label,
    files_expander: gtk::Expander,
    files_list: gtk::ListBox,
    provides_list: gtk::ListBox,
    on_mark_changed: MarkChangedCbs,
    /// Fired when the user clicks Hold/Release Hold. Unlike
    /// install/upgrade/remove/purge, a hold toggle isn't queued as a
    /// pending mark — it needs its own privileged action right away (the
    /// `Transaction` this pane doesn't own), so the actual work is left
    /// to whoever wires this up (see window.rs). Args: pkgname, `want_hold`.
    on_hold_requested: HoldRequestedCbs,
    /// Fired when the user clicks Reinstall — same rationale as
    /// `on_hold_requested` (an immediate privileged action, not a queued
    /// mark). Arg: pkgname.
    on_reinstall_requested: ActionRequestedCbs,
    /// Fired when the user clicks Reconfigure — same rationale as
    /// `on_hold_requested`. Arg: pkgname.
    on_reconfigure_requested: ActionRequestedCbs,
    /// Fired when the user clicks Download Only. Arg: pkgname.
    on_download_requested: ActionRequestedCbs,
    /// Fired when the user clicks Repo-Lock/Release Repo-Lock. Args:
    /// pkgname, `want_locked`.
    on_repolock_requested: HoldRequestedCbs,
    /// Fired when the user clicks Mark as Manually/Automatically
    /// Installed. Args: pkgname, `want_automatic`.
    on_automatic_requested: HoldRequestedCbs,
}

#[derive(Clone)]
pub struct DetailPane {
    inner: Rc<Inner>,
}

/// A flat, left-aligned button for the "More" popover's menu-style list
/// — same look as `flat_menu_button` in `window.rs`'s app menu.
fn more_menu_button(label: &str) -> gtk::Button {
    let btn = gtk::Button::with_label(label);
    btn.set_has_frame(false);
    if let Some(l) = btn.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    btn
}

/// A pill-styled chip label (state chip, tag chips, count pills share
/// the same shape; CSS classes differentiate the coloring).
fn chip(text: &str, extra_class: Option<&str>) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("chip");
    if let Some(class) = extra_class {
        l.add_css_class(class);
    }
    l.set_valign(gtk::Align::Center);
    l
}

/// A count pill for section headers/expanders; starts hidden until a
/// count is known.
fn count_pill() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("count-pill");
    l.set_visible(false);
    l.set_valign(gtk::Align::Center);
    l
}

fn set_count(pill: &gtk::Label, count: Option<usize>) {
    match count {
        Some(n) => {
            pill.set_text(&n.to_string());
            pill.set_visible(true);
        }
        None => pill.set_visible(false),
    }
}

/// Rebuilds one metadata group (SIZE / INSTALLATION / SOURCE): a
/// micro-header plus one key/value row per entry. An empty `rows`
/// hides the group entirely — omission, not placeholders.
fn rebuild_kv_group(group: &gtk::Box, title: &str, rows: Vec<(&str, KvValue)>) {
    clear_box_children(group);
    if rows.is_empty() {
        group.set_visible(false);
        return;
    }
    group.set_visible(true);

    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("section-header");
    group.append(&header);

    for (key, value) in rows {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let key_label = gtk::Label::new(Some(key));
        key_label.set_width_chars(12);
        key_label.set_xalign(0.0);
        key_label.add_css_class("dim-label");
        row.append(&key_label);
        match value {
            KvValue::Text(text) => {
                let val = gtk::Label::new(Some(&text));
                val.set_xalign(0.0);
                val.set_selectable(true);
                val.set_wrap(true);
                val.set_hexpand(true);
                row.append(&val);
            }
            KvValue::Link(url) => {
                let link = gtk::LinkButton::new(&url);
                link.set_halign(gtk::Align::Start);
                link.set_hexpand(true);
                row.append(&link);
            }
        }
        group.append(&row);
    }
}

/// Rebuilds the SIZE group from the currently-known sizes (the async
/// extra-info reply can correct the download size after the fact).
fn rebuild_size_group(inner: &Inner) {
    let mut rows = Vec::new();
    if inner.install_size.get() > 0 {
        rows.push((
            "Installed",
            KvValue::Text(crate::backend::package::pkg_format_size(
                inner.install_size.get(),
            )),
        ));
    }
    if inner.download_size.get() > 0 {
        rows.push((
            "Download",
            KvValue::Text(crate::backend::package::pkg_format_size(
                inner.download_size.get(),
            )),
        ));
    }
    rebuild_kv_group(&inner.header.size_group, "SIZE", rows);
}

impl DetailPane {
    pub fn new(store: PackageStore) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_height_request(220);
        widget.set_margin_start(12);
        widget.set_margin_end(12);
        widget.set_margin_top(10);
        widget.set_margin_bottom(10);

        // ── Action row ──
        let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        action_row.set_margin_bottom(8);
        let btn_install = gtk::Button::with_label("Install");
        btn_install.set_visible(false);
        btn_install.add_css_class("suggested-action");
        let btn_upgrade = gtk::Button::with_label("Upgrade");
        btn_upgrade.set_visible(false);
        btn_upgrade.add_css_class("suggested-action");
        let btn_remove = gtk::Button::with_label("Remove");
        btn_remove.set_visible(false);
        btn_remove.add_css_class("destructive-action");
        let btn_purge = gtk::Button::with_label("Purge");
        btn_purge.set_visible(false);
        btn_purge.add_css_class("destructive-action");
        btn_purge.set_tooltip_text(Some(
            "Remove this package and any dependencies left orphaned by doing so",
        ));
        let btn_unmark = gtk::Button::with_label("Unmark");
        btn_unmark.set_visible(false);
        action_row.append(&btn_install);
        action_row.append(&btn_upgrade);
        action_row.append(&btn_remove);
        action_row.append(&btn_purge);
        action_row.append(&btn_unmark);

        // ── "More" popover: every other, less-common action. Kept out of
        // the row above so it doesn't grow one button per feature —
        // these are all still-immediate (not queued-mark) actions, same
        // rationale as Hold below.
        let more_button = gtk::MenuButton::new();
        more_button.set_label("More");
        more_button.set_visible(false);
        let more_popover = gtk::Popover::new();
        let more_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        more_box.set_margin_start(4);
        more_box.set_margin_end(4);
        more_box.set_margin_top(4);
        more_box.set_margin_bottom(4);
        more_box.set_width_request(220);

        let btn_hold = more_menu_button("Hold");
        btn_hold.set_visible(false);
        btn_hold.set_tooltip_text(Some(
            "Pin this package's version — exclude it from upgrades",
        ));
        let btn_unhold = more_menu_button("Release Hold");
        btn_unhold.set_visible(false);
        let btn_reinstall = more_menu_button("Reinstall");
        btn_reinstall.set_visible(false);
        btn_reinstall.set_tooltip_text(Some(
            "Force re-installation, overwriting any locally-modified files",
        ));
        let btn_reconfigure = more_menu_button("Reconfigure");
        btn_reconfigure.set_visible(false);
        btn_reconfigure.set_tooltip_text(Some("Re-run this package's post-install configuration"));
        let btn_download = more_menu_button("Download Only");
        btn_download.set_visible(false);
        btn_download.set_tooltip_text(Some(
            "Fetch and verify the package file without installing it",
        ));
        let btn_repolock = more_menu_button("Repo-Lock");
        btn_repolock.set_visible(false);
        btn_repolock.set_tooltip_text(Some(
            "Only ever upgrade this package from the repository it's currently installed from",
        ));
        let btn_repounlock = more_menu_button("Release Repo-Lock");
        btn_repounlock.set_visible(false);
        let btn_mark_manual = more_menu_button("Mark as Manually Installed");
        btn_mark_manual.set_visible(false);
        btn_mark_manual.set_tooltip_text(Some(
            "Treat as explicitly requested — won't be offered for orphan cleanup",
        ));
        let btn_mark_auto = more_menu_button("Mark as Automatically Installed");
        btn_mark_auto.set_visible(false);
        btn_mark_auto.set_tooltip_text(Some(
            "Treat as a dependency pulled in for something else — eligible for orphan cleanup \
             if nothing ends up needing it",
        ));
        more_box.append(&btn_hold);
        more_box.append(&btn_unhold);
        more_box.append(&btn_reinstall);
        more_box.append(&btn_reconfigure);
        more_box.append(&btn_download);
        more_box.append(&btn_repolock);
        more_box.append(&btn_repounlock);
        more_box.append(&btn_mark_manual);
        more_box.append(&btn_mark_auto);
        more_popover.set_child(Some(&more_box));
        more_button.set_popover(Some(&more_popover));
        action_row.append(&more_button);
        widget.append(&action_row);

        // ── Split: metadata column | dependency column ──
        let split = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        split.set_vexpand(true);

        let metadata_scroll = gtk::ScrolledWindow::new();
        metadata_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let metadata_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        metadata_col.set_width_request(320);

        // ── Header: name + version + state chip + tag chips, over the
        // description ──
        let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_selectable(true);
        name.add_css_class("detail-name");
        header_row.append(&name);

        let version = gtk::Label::new(None);
        version.set_xalign(0.0);
        version.set_selectable(true);
        version.add_css_class("dim-label");
        version.set_valign(gtk::Align::Baseline);
        header_row.append(&version);

        let state_chip = chip("", None);
        header_row.append(&state_chip);

        let tags_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header_row.append(&tags_box);
        metadata_col.append(&header_row);

        let desc = gtk::Label::new(None);
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(gtk::pango::WrapMode::Word);
        desc.set_selectable(true);
        desc.add_css_class("dim-label");
        desc.set_margin_top(2);
        desc.set_margin_bottom(6);
        metadata_col.append(&desc);

        // ── Unframed key/value grid: SIZE + INSTALLATION on the left,
        // SOURCE on the right — groups without data stay hidden ──
        let kv_split = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        let kv_left = gtk::Box::new(gtk::Orientation::Vertical, 2);
        kv_left.set_hexpand(true);
        let kv_right = gtk::Box::new(gtk::Orientation::Vertical, 2);
        kv_right.set_hexpand(true);

        let size_group = gtk::Box::new(gtk::Orientation::Vertical, 2);
        size_group.set_visible(false);
        let install_group = gtk::Box::new(gtk::Orientation::Vertical, 2);
        install_group.set_visible(false);
        let source_group = gtk::Box::new(gtk::Orientation::Vertical, 2);
        source_group.set_visible(false);
        kv_left.append(&size_group);
        kv_left.append(&install_group);
        kv_right.append(&source_group);
        kv_split.append(&kv_left);
        kv_split.append(&kv_right);
        metadata_col.append(&kv_split);

        // ── Files / Provides disclosure rows with count pills ──
        let files_pill = count_pill();
        let files_label_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        files_label_box.append(&gtk::Label::new(Some("Files")));
        files_label_box.append(&files_pill);
        let files_expander = gtk::Expander::new(None);
        files_expander.set_label_widget(Some(&files_label_box));
        files_expander.set_margin_top(6);
        files_expander.set_margin_bottom(6);
        let files_list = gtk::ListBox::new();
        files_list.set_selection_mode(gtk::SelectionMode::None);
        let files_scroll = gtk::ScrolledWindow::new();
        files_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        files_scroll.set_max_content_height(260);
        files_scroll.set_propagate_natural_height(true);
        files_scroll.set_child(Some(&files_list));
        files_expander.set_child(Some(&files_scroll));
        metadata_col.append(&files_expander);

        let provides_pill = count_pill();
        let provides_label_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        provides_label_box.append(&gtk::Label::new(Some("Provides / Conflicts / Replaces")));
        provides_label_box.append(&provides_pill);
        let provides_expander = gtk::Expander::new(None);
        provides_expander.set_label_widget(Some(&provides_label_box));
        provides_expander.set_margin_bottom(6);
        // Hidden until the async extra-info reply reports something to
        // show — an empty section is omitted, not placeholder-ed.
        provides_expander.set_visible(false);
        let provides_list = gtk::ListBox::new();
        provides_list.set_selection_mode(gtk::SelectionMode::None);
        provides_expander.set_child(Some(&provides_list));
        metadata_col.append(&provides_expander);

        metadata_scroll.set_child(Some(&metadata_col));
        split.append(&metadata_scroll);
        split.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let dependency_col = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        dependency_col.set_hexpand(true);

        let deps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        deps_col.set_hexpand(true);
        let deps_header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let deps_header = gtk::Label::new(Some("DEPENDENCIES"));
        deps_header.set_xalign(0.0);
        deps_header.add_css_class("section-header");
        let deps_pill = count_pill();
        deps_header_row.append(&deps_header);
        deps_header_row.append(&deps_pill);
        deps_col.append(&deps_header_row);
        let deps_scroll = gtk::ScrolledWindow::new();
        deps_scroll.set_vexpand(true);
        let deps_list = gtk::ListBox::new();
        deps_list.set_selection_mode(gtk::SelectionMode::None);
        let deps_ph = gtk::Label::new(Some("Select a package"));
        deps_ph.add_css_class("dim-label");
        deps_ph.set_margin_top(12);
        deps_list.set_placeholder(Some(&deps_ph));
        deps_scroll.set_child(Some(&deps_list));
        deps_col.append(&deps_scroll);
        dependency_col.append(&deps_col);

        dependency_col.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let rdeps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rdeps_col.set_hexpand(true);
        let rdeps_header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let rdeps_header = gtk::Label::new(Some("REVERSE DEPENDENCIES"));
        rdeps_header.set_xalign(0.0);
        rdeps_header.add_css_class("section-header");
        let rdeps_pill = count_pill();
        rdeps_header_row.append(&rdeps_header);
        rdeps_header_row.append(&rdeps_pill);
        rdeps_col.append(&rdeps_header_row);
        let rdeps_scroll = gtk::ScrolledWindow::new();
        rdeps_scroll.set_vexpand(true);
        let rdeps_list = gtk::ListBox::new();
        rdeps_list.set_selection_mode(gtk::SelectionMode::None);
        let rdeps_ph = gtk::Label::new(Some("Select a package"));
        rdeps_ph.add_css_class("dim-label");
        rdeps_ph.set_margin_top(12);
        rdeps_list.set_placeholder(Some(&rdeps_ph));
        rdeps_scroll.set_child(Some(&rdeps_list));
        rdeps_col.append(&rdeps_scroll);
        dependency_col.append(&rdeps_col);

        split.append(&dependency_col);

        // ── Empty state vs content ──
        let content_stack = gtk::Stack::new();
        content_stack.set_vexpand(true);
        let empty_label = gtk::Label::new(Some("Select a package to view details."));
        empty_label.add_css_class("dim-label");
        content_stack.add_named(&empty_label, Some("empty"));
        content_stack.add_named(&split, Some("content"));
        content_stack.set_visible_child_name("empty");
        widget.append(&content_stack);

        let inner = Rc::new(Inner {
            widget,
            store,
            current_pkgname: RefCell::new(None),
            btn_install,
            btn_upgrade,
            btn_remove,
            btn_purge,
            btn_hold,
            btn_unhold,
            btn_reinstall,
            btn_reconfigure,
            btn_download,
            btn_repolock,
            btn_repounlock,
            btn_mark_manual,
            btn_mark_auto,
            btn_unmark,
            more_button,
            more_popover,
            header: Header {
                name,
                version,
                state_chip,
                tags_box,
                desc,
                size_group,
                install_group,
                source_group,
            },
            content_stack,
            install_size: std::cell::Cell::new(0),
            download_size: std::cell::Cell::new(0),
            current_maintainer: RefCell::new(String::new()),
            files_pill,
            provides_pill,
            deps_pill,
            rdeps_pill,
            deps_col,
            rdeps_col,
            provides_expander,
            deps_list,
            rdeps_list,
            deps_placeholder: deps_ph,
            rdeps_placeholder: rdeps_ph,
            files_expander,
            files_list,
            provides_list,
            on_mark_changed: RefCell::new(Vec::new()),
            on_hold_requested: RefCell::new(Vec::new()),
            on_reinstall_requested: RefCell::new(Vec::new()),
            on_reconfigure_requested: RefCell::new(Vec::new()),
            on_download_requested: RefCell::new(Vec::new()),
            on_repolock_requested: RefCell::new(Vec::new()),
            on_automatic_requested: RefCell::new(Vec::new()),
        });

        wire_buttons(&inner);
        wire_files_expander(&inner);

        Self { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_mark_changed(&self, f: impl Fn() + 'static) {
        self.inner.on_mark_changed.borrow_mut().push(Box::new(f));
    }

    /// pkgname, `want_hold` — fired when the user clicks Hold/Release Hold.
    pub fn connect_hold_requested(&self, f: impl Fn(String, bool) + 'static) {
        self.inner.on_hold_requested.borrow_mut().push(Box::new(f));
    }

    /// pkgname — fired when the user clicks Reinstall.
    pub fn connect_reinstall_requested(&self, f: impl Fn(String) + 'static) {
        self.inner
            .on_reinstall_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    /// pkgname — fired when the user clicks Reconfigure.
    pub fn connect_reconfigure_requested(&self, f: impl Fn(String) + 'static) {
        self.inner
            .on_reconfigure_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    /// pkgname — fired when the user clicks Download Only.
    pub fn connect_download_requested(&self, f: impl Fn(String) + 'static) {
        self.inner
            .on_download_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    /// pkgname, `want_locked` — fired when the user clicks Repo-Lock/Release
    /// Repo-Lock.
    pub fn connect_repolock_requested(&self, f: impl Fn(String, bool) + 'static) {
        self.inner
            .on_repolock_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    /// pkgname, `want_automatic` — fired when the user clicks Mark as
    /// Manually/Automatically Installed.
    pub fn connect_automatic_requested(&self, f: impl Fn(String, bool) + 'static) {
        self.inner
            .on_automatic_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    pub fn show_package(&self, pkg: Option<&Package>) {
        show_package_impl(&self.inner, pkg);
    }
}

fn wire_buttons(inner: &Rc<Inner>) {
    {
        let btn_install = inner.btn_install.clone();
        let inner = inner.clone();
        btn_install.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            let root = inner.widget.root().and_downcast::<gtk::Window>();
            let store = inner.store.clone();
            let inner2 = inner.clone();
            let name2 = name.clone();
            deps_confirm::confirm_install_deps(root.as_ref(), &store, &name, move |proceed| {
                if proceed {
                    inner2.store.set_mark(&name2, PkgMark::Install);
                    update_action_buttons(&inner2, None);
                    for f in inner2.on_mark_changed.borrow().iter() {
                        f();
                    }
                }
            });
        });
    }
    wire_simple_mark_button(inner, &inner.btn_upgrade, PkgMark::Upgrade);
    wire_remove_button(inner, &inner.btn_remove, PkgMark::Remove);
    wire_remove_button(inner, &inner.btn_purge, PkgMark::Purge);
    wire_simple_mark_button(inner, &inner.btn_unmark, PkgMark::None);

    // Hold/unhold and everything else in the "More" popover isn't a
    // queued mark — each needs a privileged action of its own, so this
    // pane just reports the request and lets the caller (which owns the
    // `Transaction`) carry it out. Every handler also closes the popover
    // itself, since a plain `gtk::Button` inside a `gtk::Popover` doesn't
    // do that automatically the way a real menu item would.
    {
        let btn_hold = inner.btn_hold.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_hold.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_hold_requested.borrow().iter() {
                f(name.clone(), true);
            }
        });
    }
    {
        let btn_unhold = inner.btn_unhold.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_unhold.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_hold_requested.borrow().iter() {
                f(name.clone(), false);
            }
        });
    }
    {
        let btn_reinstall = inner.btn_reinstall.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_reinstall.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_reinstall_requested.borrow().iter() {
                f(name.clone());
            }
        });
    }
    {
        let btn_reconfigure = inner.btn_reconfigure.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_reconfigure.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_reconfigure_requested.borrow().iter() {
                f(name.clone());
            }
        });
    }
    {
        let btn_download = inner.btn_download.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_download.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_download_requested.borrow().iter() {
                f(name.clone());
            }
        });
    }
    {
        let btn_repolock = inner.btn_repolock.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_repolock.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_repolock_requested.borrow().iter() {
                f(name.clone(), true);
            }
        });
    }
    {
        let btn_repounlock = inner.btn_repounlock.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_repounlock.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_repolock_requested.borrow().iter() {
                f(name.clone(), false);
            }
        });
    }
    {
        let btn_mark_manual = inner.btn_mark_manual.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_mark_manual.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_automatic_requested.borrow().iter() {
                f(name.clone(), false);
            }
        });
    }
    {
        let btn_mark_auto = inner.btn_mark_auto.clone();
        let popover = inner.more_popover.clone();
        let inner = inner.clone();
        btn_mark_auto.connect_clicked(move |_| {
            popover.popdown();
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            for f in inner.on_automatic_requested.borrow().iter() {
                f(name.clone(), true);
            }
        });
    }
}

/// Shared by Upgrade/Remove/Purge/Unmark: they all just set a mark on
/// the currently-shown package and notify listeners. (Install is
/// separate — it needs the deps-confirm dialog first.)
fn wire_simple_mark_button(inner: &Rc<Inner>, btn: &gtk::Button, mark: PkgMark) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        inner.store.set_mark(&name, mark);
        update_action_buttons(&inner, None);
        for f in inner.on_mark_changed.borrow().iter() {
            f();
        }
    });
}

/// Remove/Purge additionally warn first if anything else still
/// installed depends on this package (see `remove_confirm`) — unlike
/// Upgrade/Unmark, which can't break another package's dependencies.
fn wire_remove_button(inner: &Rc<Inner>, btn: &gtk::Button, mark: PkgMark) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        let root = inner.widget.root().and_downcast::<gtk::Window>();
        let store = inner.store.clone();
        let inner2 = inner.clone();
        let name2 = name.clone();
        remove_confirm::confirm_remove_impact(root.as_ref(), &store, &name, move |proceed| {
            if proceed {
                inner2.store.set_mark(&name2, mark);
                update_action_buttons(&inner2, None);
                for f in inner2.on_mark_changed.borrow().iter() {
                    f();
                }
            }
        });
    });
}

/// Re-derives button visibility from the store's live copy of the
/// package (since a caller's `Package` may now be stale after a mark
/// change triggered elsewhere). If `pkg` is `Some`, it's used directly.
fn update_action_buttons(inner: &Rc<Inner>, pkg: Option<&Package>) {
    let owned;
    let pkg: Option<&Package> = if pkg.is_some() {
        pkg
    } else if let Some(name) = inner.current_pkgname.borrow().clone() {
        let n = inner.store.list().n_items();
        let mut found = None;
        for i in 0..n {
            if let Some(obj) = inner.store.list().item(i) {
                let obj = obj
                    .downcast::<crate::backend::package::PackageObject>()
                    .unwrap();
                if obj.name() == name {
                    found = Some(obj.pkg().clone());
                    break;
                }
            }
        }
        owned = found;
        owned.as_ref()
    } else {
        None
    };

    let Some(pkg) = pkg else {
        inner.btn_install.set_visible(false);
        inner.btn_remove.set_visible(false);
        inner.btn_purge.set_visible(false);
        inner.btn_upgrade.set_visible(false);
        inner.btn_hold.set_visible(false);
        inner.btn_unhold.set_visible(false);
        inner.btn_reinstall.set_visible(false);
        inner.btn_reconfigure.set_visible(false);
        inner.btn_download.set_visible(false);
        inner.btn_repolock.set_visible(false);
        inner.btn_repounlock.set_visible(false);
        inner.btn_mark_manual.set_visible(false);
        inner.btn_mark_auto.set_visible(false);
        inner.more_button.set_visible(false);
        inner.btn_unmark.set_visible(false);
        return;
    };

    // Hold/unhold/reinstall/reconfigure/download/repolock are all
    // orthogonal to the pending-mark system (applied immediately, not
    // queued), so their visibility only depends on whether the package
    // is installed at all — not on `pkg.mark`. (Mark-manual/automatic's
    // visibility depends on data only `show_package_impl` has fetched —
    // see there.)
    let installed = pkg.state != PkgState::NotInstalled;
    inner.more_button.set_visible(true);
    inner
        .btn_hold
        .set_visible(installed && pkg.state != PkgState::OnHold);
    inner.btn_unhold.set_visible(pkg.state == PkgState::OnHold);
    inner.btn_reinstall.set_visible(installed);
    inner.btn_reconfigure.set_visible(installed);
    inner.btn_download.set_visible(!installed);
    inner
        .btn_repolock
        .set_visible(installed && !pkg.is_repolocked);
    inner
        .btn_repounlock
        .set_visible(installed && pkg.is_repolocked);

    if pkg.mark != PkgMark::None {
        inner.btn_install.set_visible(false);
        inner.btn_remove.set_visible(false);
        inner.btn_purge.set_visible(false);
        inner.btn_upgrade.set_visible(false);
        inner.btn_unmark.set_visible(true);
        return;
    }
    inner.btn_unmark.set_visible(false);

    if pkg.state == PkgState::NotInstalled {
        inner.btn_install.set_visible(true);
        inner.btn_remove.set_visible(false);
        inner.btn_purge.set_visible(false);
        inner.btn_upgrade.set_visible(false);
    } else {
        inner.btn_install.set_visible(false);
        inner
            .btn_upgrade
            .set_visible(pkg.state == PkgState::Upgradable);
        inner.btn_remove.set_visible(true);
        inner.btn_remove.set_sensitive(!pkg.essential);
        inner.btn_remove.set_tooltip_text(if pkg.essential {
            Some("Essential package — removal disabled")
        } else {
            None
        });
        inner.btn_purge.set_visible(true);
        inner.btn_purge.set_sensitive(!pkg.essential);
        inner.btn_purge.set_tooltip_text(if pkg.essential {
            Some("Essential package — purge disabled")
        } else {
            None
        });
    }
}

fn populate(lb: &gtk::ListBox, items: Option<Vec<String>>) {
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
    let Some(items) = items else { return };
    for item in items {
        lb.append(&crate::ui::dialog_util::text_list_row(&item, false));
    }
}

/// One row per non-empty category — "Provides"/"Conflicts"/"Replaces"
/// (plain package/virtual-package names) and "Requires"/"Exports"
/// (shared-library sonames), each joined onto a single wrapped line
/// since these lists are usually short and this is already a rarely-
/// expanded, supplementary section.
fn populate_provides_conflicts(
    inner: &Rc<Inner>,
    extra: Option<&crate::backend::package::PackageExtraInfo>,
) {
    while let Some(c) = inner.provides_list.first_child() {
        inner.provides_list.remove(&c);
    }
    let Some(extra) = extra else {
        inner.provides_expander.set_visible(false);
        set_count(&inner.provides_pill, None);
        return;
    };
    let sections: [(&str, &[String]); 5] = [
        ("Provides", &extra.provides),
        ("Conflicts", &extra.conflicts),
        ("Replaces", &extra.replaces),
        ("Requires", &extra.shlib_requires),
        ("Exports", &extra.shlib_provides),
    ];
    let mut total = 0;
    for (label, items) in sections {
        if items.is_empty() {
            continue;
        }
        total += items.len();
        inner
            .provides_list
            .append(&crate::ui::dialog_util::text_list_row(
                &format!("{}: {}", label, items.join(", ")),
                true,
            ));
    }
    // An empty section is omitted, not placeholder-ed.
    inner.provides_expander.set_visible(total > 0);
    set_count(&inner.provides_pill, (total > 0).then_some(total));
}

/// Files lists are only fetched (a round-trip to the xbps worker
/// thread) when the user actually expands the section, rather than on
/// every package selection — most selections are just someone scanning
/// down the list, and a query nobody looks at is wasted latency.
fn wire_files_expander(inner: &Rc<Inner>) {
    let files_expander = inner.files_expander.clone();
    let inner = inner.clone();
    files_expander.connect_expanded_notify(move |exp| {
        if !exp.is_expanded() {
            return;
        }
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        let inner2 = inner.clone();
        let name_for_call = name.clone();
        inner.store.get_files_async(&name_for_call, move |files| {
            // The selection may have moved on (or the expander collapsed,
            // which show_package_impl does on every new selection) while
            // the worker was busy — a stale reply must not overwrite the
            // now-current package's (empty) list.
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                populate_files(&inner2, files);
            }
        });
    });
}

fn populate_files(inner: &Inner, files: Option<Vec<String>>) {
    let lb = &inner.files_list;
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
    let Some(mut files) = files else {
        set_count(&inner.files_pill, None);
        return;
    };
    set_count(&inner.files_pill, Some(files.len()));
    files.sort();
    let total = files.len();
    let shown = total.min(MAX_FILES_SHOWN);
    for f in &files[..shown] {
        let l = gtk::Label::new(Some(f));
        l.set_xalign(0.0);
        l.set_selectable(true);
        l.add_css_class("monospace");
        l.set_margin_start(8);
        l.set_margin_top(2);
        l.set_margin_bottom(2);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        lb.append(&row);
    }
    if total > shown {
        let l = gtk::Label::new(Some(&format!("\u{2026} and {} more", total - shown)));
        l.set_xalign(0.0);
        l.add_css_class("dim-label");
        l.set_margin_start(8);
        l.set_margin_top(4);
        l.set_margin_bottom(4);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        row.set_activatable(false);
        row.set_selectable(false);
        lb.append(&row);
    }
}

fn clear_box_children(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn show_package_impl(inner: &Rc<Inner>, pkg: Option<&Package>) {
    *inner.current_pkgname.borrow_mut() = pkg.map(|p| p.name.clone());

    // A new selection invalidates whatever the Files section was
    // showing; collapse it back so re-expanding fetches fresh data for
    // the newly-selected package rather than showing the old one's list
    // (or nothing, if it wasn't installed).
    inner.files_expander.set_expanded(false);
    populate_files(inner, None);

    let h = &inner.header;

    let Some(pkg) = pkg else {
        // No selection: nothing to describe, so nothing renders — the
        // stack's empty page carries the one line of guidance.
        inner.content_stack.set_visible_child_name("empty");
        update_action_buttons(inner, None);
        return;
    };
    inner.content_stack.set_visible_child_name("content");

    // ── Header ──
    h.name.set_text(&pkg.name);

    let ver = match (&pkg.version_installed, &pkg.version_available) {
        (Some(inst), Some(avail)) if inst != avail => Some(format!("{inst}  \u{2192}  {avail}")),
        (Some(inst), _) => Some(inst.clone()),
        (None, Some(avail)) => Some(avail.clone()),
        (None, None) => None,
    };
    h.version.set_visible(ver.is_some());
    h.version.set_text(ver.as_deref().unwrap_or(""));

    // The state chip always shows: install state is data, not absence.
    let (state_text, state_class) = match pkg.state {
        PkgState::NotInstalled => ("Not installed", None),
        PkgState::Installed => ("Installed", Some("chip-ok")),
        PkgState::Upgradable => ("Upgrade available", Some("chip-warn")),
        PkgState::OnHold => ("On hold", Some("chip-warn")),
        PkgState::Broken => ("Broken", Some("chip-err")),
    };
    for class in ["chip-ok", "chip-warn", "chip-err"] {
        h.state_chip.remove_css_class(class);
    }
    if let Some(class) = state_class {
        h.state_chip.add_css_class(class);
    }
    h.state_chip.set_text(state_text);

    clear_box_children(&h.tags_box);
    for tag in pkg
        .tags
        .split([',', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        h.tags_box.append(&chip(tag, None));
    }

    h.desc.set_text(
        pkg.long_desc
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(Some(pkg.short_desc.as_str()))
            .filter(|s| !s.is_empty())
            .unwrap_or("No description available."),
    );

    // ── SIZE (sync; the async reply may correct the download size) ──
    inner.install_size.set(pkg.install_size);
    inner.download_size.set(pkg.download_size);
    rebuild_size_group(inner);

    // ── INSTALLATION / SOURCE: rebuilt when the async extra-info
    // lookup lands; until then the maintainer (known synchronously) is
    // the only SOURCE row. Would flicker if the worker were slow, but
    // these queries are fast whenever the worker isn't mid-reload — and
    // when it *is*, this is exactly what keeps the whole UI from
    // freezing until the rescan finishes.
    *inner.current_maintainer.borrow_mut() = pkg.maintainer.clone();
    rebuild_kv_group(&h.install_group, "INSTALLATION", Vec::new());
    rebuild_source_group(inner, None);
    inner.btn_mark_manual.set_visible(false);
    inner.btn_mark_auto.set_visible(false);
    populate_provides_conflicts(inner, None);

    // Files are fetched lazily on expand; the row only exists at all
    // for packages that are actually on disk.
    inner
        .files_expander
        .set_visible(pkg.state != PkgState::NotInstalled);

    {
        let inner = inner.clone();
        let name = pkg.name.clone();
        inner
            .store
            .clone()
            .get_extra_info_async(&pkg.name, move |extra| {
                // Stale-reply guard: the user may have selected another
                // package while this was queued behind other worker commands.
                if inner.current_pkgname.borrow().as_deref() != Some(name.as_str()) {
                    return;
                }

                if let Some(extra) = &extra {
                    if extra.download_size > 0 {
                        inner.download_size.set(extra.download_size);
                        rebuild_size_group(&inner);
                    }
                }

                // ── INSTALLATION: rows only for data that exists ──
                let mut install_rows = Vec::new();
                if let Some(date) = extra
                    .as_ref()
                    .and_then(|e| e.install_date.as_deref())
                    .filter(|d| !d.is_empty())
                {
                    install_rows.push(("Installed on", KvValue::Text(date.to_string())));
                }
                if let Some(extra) = extra.as_ref().filter(|e| e.has_automatic_install) {
                    install_rows.push((
                        "Auto-installed",
                        KvValue::Text(if extra.automatic_install { "Yes" } else { "No" }.into()),
                    ));
                }
                rebuild_kv_group(&inner.header.install_group, "INSTALLATION", install_rows);

                rebuild_source_group(&inner, extra.as_ref());

                // Manual/automatic marking only makes sense for a real
                // installed pkgdb entry (`has_automatic_install`) — a
                // not-yet-installed package (or one whose extra info failed
                // to load) gets neither.
                let auto_flag = extra.as_ref().filter(|e| e.has_automatic_install);
                inner
                    .btn_mark_manual
                    .set_visible(auto_flag.is_some_and(|e| e.automatic_install));
                inner
                    .btn_mark_auto
                    .set_visible(auto_flag.is_some_and(|e| !e.automatic_install));

                populate_provides_conflicts(&inner, extra.as_ref());
            });
    }

    // ── Dependency columns: shown while loading, then either filled
    // with a count pill or hidden outright when there's nothing — an
    // empty list is omitted, not placeholder-ed. ──
    inner.deps_col.set_visible(true);
    inner.rdeps_col.set_visible(true);
    inner.deps_placeholder.set_text("Loading\u{2026}");
    inner.rdeps_placeholder.set_text("Loading\u{2026}");
    set_count(&inner.deps_pill, None);
    set_count(&inner.rdeps_pill, None);
    populate(&inner.deps_list, None);
    populate(&inner.rdeps_list, None);
    {
        let inner2 = inner.clone();
        let name = pkg.name.clone();
        inner.store.get_deps_async(&pkg.name, move |deps| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                let count = deps.as_ref().map_or(0, Vec::len);
                inner2.deps_col.set_visible(count > 0);
                set_count(&inner2.deps_pill, (count > 0).then_some(count));
                populate(&inner2.deps_list, deps);
            }
        });
    }
    {
        let inner2 = inner.clone();
        let name = pkg.name.clone();
        inner.store.get_rdeps_async(&pkg.name, move |rdeps| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                let count = rdeps.as_ref().map_or(0, Vec::len);
                inner2.rdeps_col.set_visible(count > 0);
                set_count(&inner2.rdeps_pill, (count > 0).then_some(count));
                populate(&inner2.rdeps_list, rdeps);
            }
        });
    }

    update_action_buttons(inner, Some(pkg));
}

/// Rebuilds the SOURCE group: repository / license / maintainer /
/// homepage — whichever of them actually have values.
fn rebuild_source_group(inner: &Inner, extra: Option<&crate::backend::package::PackageExtraInfo>) {
    let mut rows = Vec::new();

    if let Some(url) = extra
        .and_then(|e| e.repository.as_deref())
        .filter(|r| !r.is_empty())
    {
        // Honor the user's custom repository display name (set via
        // right-click in the filter sidebar) — the sidebar and this
        // row otherwise showed two different names for the same repo.
        // Re-loaded per lookup so a rename done mid-session shows up
        // on the next selection; the file is a handful of lines.
        let repo_names = crate::backend::repo_names::RepoNames::load();
        let display = repo_names.get(url).map_or_else(
            || crate::backend::repo_names::display_repo(url).to_string(),
            str::to_string,
        );
        rows.push(("Repository", KvValue::Text(display)));
    }

    if let Some(license) = extra
        .and_then(|e| e.license.as_deref())
        .filter(|l| !l.is_empty())
    {
        rows.push(("License", KvValue::Text(license.to_string())));
    }

    let maintainer = inner.current_maintainer.borrow();
    if !maintainer.is_empty() {
        rows.push(("Maintainer", KvValue::Text(maintainer.clone())));
    }
    drop(maintainer);

    if let Some(url) = extra
        .and_then(|e| e.homepage.as_deref())
        .filter(|u| !u.is_empty())
    {
        rows.push(("Homepage", KvValue::Link(url.to_string())));
    }

    rebuild_kv_group(&inner.header.source_group, "SOURCE", rows);
}
