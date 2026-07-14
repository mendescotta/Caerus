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

const DASH: &str = "\u{2014}";

/// Files lists can run into the thousands of entries for large
/// packages; a plain (non-virtualized) `gtk::ListBox` materializes one
/// widget per row, so this caps how many are actually shown to keep the
/// expander responsive.
const MAX_FILES_SHOWN: usize = 300;

struct Labels {
    name: gtk::Label,
    version: gtk::Label,
    tags: gtk::Label,
    state: gtk::Label,
    desc: gtk::Label,
    installed_size: gtk::Label,
    download_size: gtk::Label,
    maintainer: gtk::Label,
    homepage_box: gtk::Box,
    license: gtk::Label,
    repository: gtk::Label,
    install_date: gtk::Label,
    auto_install: gtk::Label,
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
    labels: Labels,
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

fn info_row(container: &gtk::Box, label: &str) -> gtk::Label {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let key = gtk::Label::new(Some(label));
    key.set_width_chars(13);
    key.set_xalign(1.0);
    key.add_css_class("dim-label");
    row.append(&key);

    let val = gtk::Label::new(Some(DASH));
    val.set_xalign(0.0);
    val.set_selectable(true);
    val.set_wrap(true);
    val.set_hexpand(true);
    row.append(&val);

    container.append(&row);
    val
}

fn framed(title: &str) -> (gtk::Frame, gtk::Box) {
    let frame = gtk::Frame::new(Some(title));
    frame.set_margin_bottom(6);
    let inner_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    inner_box.set_margin_start(8);
    inner_box.set_margin_end(8);
    inner_box.set_margin_top(6);
    inner_box.set_margin_bottom(8);
    frame.set_child(Some(&inner_box));
    (frame, inner_box)
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

        let (frame_info, box_info) = framed("Package Information");
        let name = info_row(&box_info, "Name:");
        let version = info_row(&box_info, "Version:");
        let tags = info_row(&box_info, "Tags:");
        let state = info_row(&box_info, "State:");
        box_info.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let desc = gtk::Label::new(Some("Select a package to view details."));
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(gtk::pango::WrapMode::Word);
        desc.set_selectable(true);
        box_info.append(&desc);
        metadata_col.append(&frame_info);

        let (frame_sizes, box_sizes) = framed("Size Statistics");
        let installed_size = info_row(&box_sizes, "Installed:");
        let download_size = info_row(&box_sizes, "Download:");
        metadata_col.append(&frame_sizes);

        let (frame_maint, box_maint) = framed("Maintainer & Source");
        let maintainer = info_row(&box_maint, "Maintainer:");

        let homepage_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let homepage_key = gtk::Label::new(Some("Homepage:"));
        homepage_key.set_width_chars(13);
        homepage_key.set_xalign(1.0);
        homepage_key.add_css_class("dim-label");
        homepage_row.append(&homepage_key);
        let homepage_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        homepage_box.set_hexpand(true);
        homepage_row.append(&homepage_box);
        box_maint.append(&homepage_row);

        let license = info_row(&box_maint, "License:");
        let repository = info_row(&box_maint, "Repository:");
        let install_date = info_row(&box_maint, "Installed on:");
        let auto_install = info_row(&box_maint, "Auto-installed:");
        metadata_col.append(&frame_maint);

        let files_expander = gtk::Expander::new(Some("Files"));
        files_expander.set_margin_bottom(6);
        let files_list = gtk::ListBox::new();
        files_list.set_selection_mode(gtk::SelectionMode::None);
        let files_ph = gtk::Label::new(Some("Not installed"));
        files_ph.add_css_class("dim-label");
        files_ph.set_margin_top(8);
        files_ph.set_margin_bottom(8);
        files_list.set_placeholder(Some(&files_ph));
        let files_scroll = gtk::ScrolledWindow::new();
        files_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        files_scroll.set_max_content_height(260);
        files_scroll.set_propagate_natural_height(true);
        files_scroll.set_child(Some(&files_list));
        files_expander.set_child(Some(&files_scroll));
        metadata_col.append(&files_expander);

        let provides_expander = gtk::Expander::new(Some("Provides / Conflicts / Replaces"));
        provides_expander.set_margin_bottom(6);
        let provides_list = gtk::ListBox::new();
        provides_list.set_selection_mode(gtk::SelectionMode::None);
        let provides_ph = gtk::Label::new(Some("None"));
        provides_ph.add_css_class("dim-label");
        provides_ph.set_margin_top(8);
        provides_ph.set_margin_bottom(8);
        provides_list.set_placeholder(Some(&provides_ph));
        provides_expander.set_child(Some(&provides_list));
        metadata_col.append(&provides_expander);

        metadata_scroll.set_child(Some(&metadata_col));
        split.append(&metadata_scroll);
        split.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let dependency_col = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        dependency_col.set_hexpand(true);

        let deps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        deps_col.set_hexpand(true);
        let deps_header = gtk::Label::new(Some("DEPENDENCIES"));
        deps_header.set_xalign(0.0);
        deps_header.add_css_class("section-header");
        deps_col.append(&deps_header);
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
        let rdeps_header = gtk::Label::new(Some("REVERSE DEPENDENCIES"));
        rdeps_header.set_xalign(0.0);
        rdeps_header.add_css_class("section-header");
        rdeps_col.append(&rdeps_header);
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
        widget.append(&split);

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
            labels: Labels {
                name,
                version,
                tags,
                state,
                desc,
                installed_size,
                download_size,
                maintainer,
                homepage_box,
                license,
                repository,
                install_date,
                auto_install,
            },
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
    let Some(extra) = extra else { return };
    let sections: [(&str, &[String]); 5] = [
        ("Provides", &extra.provides),
        ("Conflicts", &extra.conflicts),
        ("Replaces", &extra.replaces),
        ("Requires", &extra.shlib_requires),
        ("Exports", &extra.shlib_provides),
    ];
    for (label, items) in sections {
        if items.is_empty() {
            continue;
        }
        inner
            .provides_list
            .append(&crate::ui::dialog_util::text_list_row(
                &format!("{}: {}", label, items.join(", ")),
                true,
            ));
    }
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
                populate_files(&inner2.files_list, files);
            }
        });
    });
}

fn populate_files(lb: &gtk::ListBox, files: Option<Vec<String>>) {
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
    let Some(mut files) = files else { return };
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

fn set_homepage_value(homepage_box: &gtk::Box, url: Option<&str>) {
    clear_box_children(homepage_box);
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        let link = gtk::LinkButton::new(url);
        link.set_halign(gtk::Align::Start);
        homepage_box.append(&link);
    } else {
        let lbl = gtk::Label::new(Some(DASH));
        lbl.set_xalign(0.0);
        homepage_box.append(&lbl);
    }
}

fn show_package_impl(inner: &Rc<Inner>, pkg: Option<&Package>) {
    *inner.current_pkgname.borrow_mut() = pkg.map(|p| p.name.clone());

    // A new selection invalidates whatever the Files section was
    // showing; collapse it back so re-expanding fetches fresh data for
    // the newly-selected package rather than showing the old one's list
    // (or nothing, if it wasn't installed).
    inner.files_expander.set_expanded(false);
    populate_files(&inner.files_list, None);

    let l = &inner.labels;

    let Some(pkg) = pkg else {
        l.name.set_text(DASH);
        l.version.set_text(DASH);
        l.tags.set_text(DASH);
        l.state.set_text(DASH);
        l.desc.set_text("Select a package to view details.");
        l.installed_size.set_text(DASH);
        l.download_size.set_text(DASH);
        l.maintainer.set_text(DASH);
        set_homepage_value(&l.homepage_box, None);
        l.license.set_text(DASH);
        l.repository.set_text(DASH);
        l.install_date.set_text(DASH);
        l.auto_install.set_text(DASH);
        inner.deps_placeholder.set_text("Select a package");
        inner.rdeps_placeholder.set_text("Select a package");
        populate(&inner.deps_list, None);
        populate(&inner.rdeps_list, None);
        populate_provides_conflicts(inner, None);
        update_action_buttons(inner, None);
        return;
    };

    // ── Package Information ──
    l.name.set_text(&pkg.name);

    let ver = match (&pkg.version_installed, &pkg.version_available) {
        (Some(inst), Some(avail)) if inst != avail => format!("{inst}  \u{2192}  {avail}"),
        (Some(inst), _) => inst.clone(),
        (None, Some(avail)) => avail.clone(),
        (None, None) => DASH.to_string(),
    };
    l.version.set_text(&ver);

    l.tags
        .set_text(if pkg.tags.is_empty() { DASH } else { &pkg.tags });

    let state_text = match pkg.state {
        PkgState::NotInstalled => "Not installed",
        PkgState::Installed => "Installed",
        PkgState::Upgradable => "Upgrade available",
        PkgState::OnHold => "On hold",
        PkgState::Broken => "Broken",
    };
    l.state.set_text(state_text);

    l.desc.set_text(
        pkg.long_desc
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(Some(pkg.short_desc.as_str()))
            .filter(|s| !s.is_empty())
            .unwrap_or("No description available."),
    );

    // ── Size Statistics ──
    l.installed_size
        .set_text(&crate::backend::package::pkg_format_size(pkg.install_size));

    let dsz = if pkg.download_size > 0 {
        Some(crate::backend::package::pkg_format_size(pkg.download_size))
    } else {
        None
    };
    l.download_size.set_text(dsz.as_deref().unwrap_or(DASH));

    l.maintainer.set_text(if pkg.maintainer.is_empty() {
        DASH
    } else {
        &pkg.maintainer
    });

    // ── Maintainer & Source: provisional placeholders now, real values
    // once the async extra-info lookup lands. Would flicker if the
    // worker were slow, but these queries are fast whenever the worker
    // isn't mid-reload — and when it *is*, this is exactly what keeps
    // the whole UI from freezing until the rescan finishes.
    set_homepage_value(&l.homepage_box, None);
    l.license.set_text(DASH);
    l.repository.set_text(DASH);
    l.install_date.set_text(DASH);
    l.auto_install.set_text(DASH);
    inner.btn_mark_manual.set_visible(false);
    inner.btn_mark_auto.set_visible(false);
    populate_provides_conflicts(inner, None);

    {
        let inner = inner.clone();
        let name = pkg.name.clone();
        inner.store.clone().get_extra_info_async(&pkg.name, move |extra| {
            // Stale-reply guard: the user may have selected another
            // package while this was queued behind other worker commands.
            if inner.current_pkgname.borrow().as_deref() != Some(name.as_str()) {
                return;
            }
            let l = &inner.labels;

            if let Some(extra) = &extra {
                if extra.download_size > 0 {
                    l.download_size
                        .set_text(&crate::backend::package::pkg_format_size(
                            extra.download_size,
                        ));
                }
            }

            set_homepage_value(
                &l.homepage_box,
                extra.as_ref().and_then(|e| e.homepage.as_deref()),
            );
            l.license.set_text(
                extra
                    .as_ref()
                    .and_then(|e| e.license.as_deref())
                    .unwrap_or(DASH),
            );
            // Honor the user's custom repository display name (set via
            // right-click in the filter sidebar) — the sidebar and this
            // row otherwise showed two different names for the same repo.
            // Re-loaded per lookup so a rename done mid-session shows up
            // on the next selection; the file is a handful of lines.
            let repo_names = crate::backend::repo_names::RepoNames::load();
            l.repository.set_text(
                extra
                    .as_ref()
                    .and_then(|e| e.repository.as_deref())
                    .map_or(DASH.to_string(), |url| {
                        repo_names.get(url).map_or_else(
                            || crate::backend::repo_names::display_repo(url).to_string(),
                            str::to_string,
                        )
                    })
                    .as_str(),
            );
            l.install_date.set_text(
                extra
                    .as_ref()
                    .and_then(|e| e.install_date.as_deref())
                    .unwrap_or(DASH),
            );

            if let Some(extra) = &extra {
                if extra.has_automatic_install {
                    l.auto_install
                        .set_text(if extra.automatic_install { "Yes" } else { "No" });
                } else {
                    l.auto_install.set_text(DASH);
                }
            } else {
                l.auto_install.set_text(DASH);
            }

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

    // ── Dependency column ──
    inner.deps_placeholder.set_text("Loading\u{2026}");
    inner.rdeps_placeholder.set_text("Loading\u{2026}");
    populate(&inner.deps_list, None);
    populate(&inner.rdeps_list, None);
    {
        let inner2 = inner.clone();
        let name = pkg.name.clone();
        inner.store.get_deps_async(&pkg.name, move |deps| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                if deps.is_none() {
                    inner2.deps_placeholder.set_text("No dependencies");
                }
                populate(&inner2.deps_list, deps);
            }
        });
    }
    {
        let inner2 = inner.clone();
        let name = pkg.name.clone();
        inner.store.get_rdeps_async(&pkg.name, move |rdeps| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                if rdeps.is_none() {
                    inner2.rdeps_placeholder.set_text("No reverse dependencies");
                }
                populate(&inner2.rdeps_list, rdeps);
            }
        });
    }

    update_action_buttons(inner, Some(pkg));
}
