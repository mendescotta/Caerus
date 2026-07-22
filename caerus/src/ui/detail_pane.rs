//! Detail pane: action buttons, a header (name/version/state/tags) over
//! an unframed SIZE/INSTALLATION/SOURCE key/value grid plus a Files
//! expander, and — in the wider right-hand column — Dependencies /
//! Reverse Dependencies side by side over a second Provides / Conflicts
//! & Replaces row. Rust translation of `ui/detail_pane.{h,c}`, built
//! directly in code here rather than from a `GtkBuilder` .ui file.

use crate::backend::package::{Package, PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use crate::ui::dialog_util::{count_pill, set_count};
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
    /// The `.linked` wrapper `Box`es around each button pair/trio in
    /// `secondary_row`. A `gtk::Box` with every child hidden is still
    /// itself "visible" (an empty widget, not an absent one) — inside a
    /// `FlowBox` that still reserves a flow slot and column-spacing for
    /// it, silently shoving any later cluster rightward out of
    /// alignment with `action_row` above. So each cluster's own
    /// visibility is kept in lockstep with "does it have anything to
    /// show" everywhere its members' visibility changes, rather than
    /// leaving it permanently `true` from construction.
    hold_cluster: gtk::Box,
    repolock_cluster: gtk::Box,
    mark_cluster: gtk::Box,
    /// Unlike the other three, this one always has *some* visible
    /// member when a package is selected (Reinstall+Reconfigure when
    /// installed, Download Only when not) — but with no package
    /// selected at all, none of its three buttons are, and it needs the
    /// same hide-when-empty treatment as the others.
    oneshot_cluster: gtk::Box,
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
    deps_pill: gtk::Label,
    rdeps_pill: gtk::Label,
    deps_col: gtk::Box,
    rdeps_col: gtk::Box,
    dependency_col: gtk::Box,
    relation_rows: RefCell<Vec<gtk::Box>>,
    deps_list: gtk::ListBox,
    rdeps_list: gtk::ListBox,
    /// The two lists' placeholder labels, retargeted per state: "Select
    /// a package" with no selection, "Loading…" while the async fetch is
    /// in flight, "No (reverse) dependencies" once an empty reply lands.
    deps_placeholder: gtk::Label,
    rdeps_placeholder: gtk::Label,
    files_expander: gtk::Expander,
    files_list: gtk::ListBox,
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

/// One segment of a `.linked` button cluster (see `linked_cluster`) — an
/// icon + label button, the standard GTK4 look for segmented groups.
fn secondary_button(icon_name: &str, label: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon_name));
    content.append(&gtk::Label::new(Some(label)));
    let btn = gtk::Button::new();
    btn.set_child(Some(&content));
    // A plain GTK button, at rest, reads as low-contrast enough on a
    // dark theme to look disabled next to `action_row`'s bold
    // `suggested-action` Install/Upgrade — even though every one of
    // these is a fully live action. `.secondary-action` (install_css in
    // window.rs) gives it the same subtle-tint idiom as `.chip`/
    // `.count-pill` so it reads as "available", not "greyed out".
    // `.segment-active` (applied on top of this for the button that
    // already describes the package's current state) overrides the
    // tint back down — declared after `.secondary-action` in the
    // stylesheet so it wins the cascade.
    btn.add_css_class("secondary-action");
    btn
}

/// Groups buttons into a single fused-border strip via GTK4's own
/// `.linked` style class — defined by the theme itself (every GTK4
/// theme, not just libadwaita's), so it renders correctly with or
/// without the `adwaita` Cargo feature.
fn linked_cluster(buttons: &[&gtk::Button]) -> gtk::Box {
    let cluster = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    cluster.add_css_class("linked");
    for btn in buttons {
        cluster.append(*btn);
    }
    cluster
}

/// Hides (or shows) a secondary-action cluster, *and* the
/// `GtkFlowBoxChild` `secondary_row.insert()` auto-wraps it in.
/// Hiding only the cluster `Box` itself leaves that wrapper visible —
/// an empty but still-present flow cell that `GtkFlowBox` keeps
/// allocating a column slot and `column_spacing` for, silently padding
/// out every hidden cluster ahead of the first visible one. With three
/// of the four clusters commonly hidden at once (e.g. a not-installed
/// package, where only "Download Only" shows), that reserved spacing
/// compounds into tens of pixels indenting the one visible cluster away
/// from `action_row`'s left edge above it — this is what actually broke
/// "Download Only" flush-left alignment, not anything about the button
/// or its own margins. Toggling the wrapper's visibility in lockstep
/// with the cluster's is the actual fix; `parent()` is always the
/// `FlowBoxChild` once the cluster has been `insert()`-ed into
/// `secondary_row`, so no extra field is needed to reach it.
fn set_cluster_visible(cluster: &gtk::Box, visible: bool) {
    cluster.set_visible(visible);
    if let Some(parent) = cluster.parent() {
        parent.set_visible(visible);
    }
}

/// Marks `active` as the segment describing the package's *current*
/// state in a two-segment toggle pair (Hold/Release Hold, Repo-Lock/
/// Release, Mark Manual/Auto): tinted via the `.segment-active` CSS
/// class (plain `alpha(currentColor, …)`, same idiom as `.chip` in
/// `install_css` — no libadwaita named accent color involved) and
/// insensitive, since it already describes reality and isn't a
/// pending action. `inactive` is left as a normal, clickable button —
/// the one action this pair actually offers right now.
fn set_segment_state(active: &gtk::Button, inactive: &gtk::Button) {
    active.add_css_class("segment-active");
    active.set_sensitive(false);
    inactive.remove_css_class("segment-active");
    inactive.set_sensitive(true);
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

    let header = gtk::Label::new(Some(&format!("{title}:")));
    header.set_xalign(0.0);
    header.set_halign(gtk::Align::Start);
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
                // A bare `LinkButton` carries GtkButton's own padding,
                // which is taller than a plain value `Label` — the
                // Homepage row would otherwise sit visibly further from
                // its neighbor than every other row in the group.
                // `.flat` (no border/background) plus `.inline-link`
                // (install_css in window.rs, zeroes the leftover button
                // padding) makes it read as inline hyperlink text at
                // the same row height as everything else.
                let link = gtk::LinkButton::new(&url);
                link.add_css_class("flat");
                link.add_css_class("inline-link");
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

        // ── Secondary actions: everything that used to live behind a
        // "More" popover, now visible by default as GTK `.linked`
        // segmented clusters (a core-GTK4 style class, not
        // libadwaita-specific — renders correctly whether or not the
        // `adwaita` Cargo feature is enabled) grouped by relationship —
        // the three mutually-exclusive state pairs, then the one-shot
        // actions — rather than dumped in one flat row. Both segments of
        // a pair stay visible at once now (see `update_action_buttons`
        // / `set_segment_state`): the one matching the package's current
        // state is tinted and insensitive, its sibling is the live
        // action. Icons are bundled symbolic SVGs under
        // `data/icons/hicolor/symbolic/` (see `USED_SYMBOLIC_ICONS` in
        // `window.rs`) — hicolor is a fallback checked regardless of the
        // active icon theme, same mechanism as every other icon in this
        // app, nothing libadwaita-specific.
        let btn_hold = secondary_button("hold-symbolic", "Hold");
        btn_hold.set_visible(false);
        btn_hold.set_tooltip_text(Some(
            "Pin this package's version — exclude it from upgrades",
        ));
        let btn_unhold = secondary_button("unhold-symbolic", "Release Hold");
        btn_unhold.set_visible(false);
        btn_unhold.set_tooltip_text(Some("Un-pin this package's version — allow upgrades again"));
        let btn_reinstall = secondary_button("reinstall-symbolic", "Reinstall");
        btn_reinstall.set_visible(false);
        btn_reinstall.set_tooltip_text(Some(
            "Force re-installation, overwriting any locally-modified files",
        ));
        let btn_reconfigure = secondary_button("applications-utilities-symbolic", "Reconfigure");
        btn_reconfigure.set_visible(false);
        btn_reconfigure.set_tooltip_text(Some("Re-run this package's post-install configuration"));
        let btn_download = secondary_button("download-only-symbolic", "Download Only");
        btn_download.set_visible(false);
        btn_download.set_tooltip_text(Some(
            "Fetch and verify the package file without installing it",
        ));
        let btn_repolock = secondary_button("repo-lock-symbolic", "Repo-Lock");
        btn_repolock.set_visible(false);
        btn_repolock.set_tooltip_text(Some(
            "Only ever upgrade this package from the repository it's currently installed from",
        ));
        let btn_repounlock = secondary_button("repo-unlock-symbolic", "Release Repo-Lock");
        btn_repounlock.set_visible(false);
        btn_repounlock.set_tooltip_text(Some("Allow upgrades from any enabled repository again"));
        let btn_mark_manual =
            secondary_button("mark-manual-symbolic", "Mark as Manually Installed");
        btn_mark_manual.set_visible(false);
        btn_mark_manual.set_tooltip_text(Some(
            "Treat as explicitly requested — won't be offered for orphan cleanup",
        ));
        let btn_mark_auto =
            secondary_button("mark-auto-symbolic", "Mark as Automatically Installed");
        btn_mark_auto.set_visible(false);
        btn_mark_auto.set_tooltip_text(Some(
            "Treat as a dependency pulled in for something else — eligible for orphan cleanup \
             if nothing ends up needing it",
        ));

        let hold_cluster = linked_cluster(&[&btn_hold, &btn_unhold]);
        let repolock_cluster = linked_cluster(&[&btn_repolock, &btn_repounlock]);
        let mark_cluster = linked_cluster(&[&btn_mark_manual, &btn_mark_auto]);
        let oneshot_cluster = linked_cluster(&[&btn_reinstall, &btn_reconfigure, &btn_download]);

        // A plain `gtk::Box` was tried first (its children are the
        // actual cluster widgets, with no intermediate wrapper node
        // that could carry surprise theme padding — a structural
        // guarantee of left-flush alignment rather than a CSS patch).
        // But a plain `Box` never wraps: with every cluster visible
        // (installed, on-hold, repo-locked, manually-marked package —
        // an entirely ordinary state) the unwrapped row is wider than
        // `left_col`'s share of `split`, and a non-hexpanding `Box`
        // reports *that* full width as its natural size — which
        // dragged `left_col`'s own natural width along with it and
        // squeezed `dependency_col` down past visibility. `GtkFlowBox`
        // is the correct tool here: it wraps onto additional lines
        // instead of reporting an ever-growing natural width, so it
        // can never force a sibling column out of the layout. Its
        // downside — every GTK4 theme styles the `GtkFlowBoxChild`
        // wrapper around each child with its own padding/margin (sized
        // for icon-grid cells, e.g. Nautilus' icon view), several times
        // wider than this row needs, which would otherwise inset the
        // first visible cluster from the left edge and break flush
        // alignment with `action_row`'s buttons above — is zeroed out
        // by `.tight-flow` (install_css in window.rs) at APPLICATION
        // style-provider priority, which beats the theme's own
        // THEME-priority rule regardless of which theme is active; the
        // pixel measurement in this pane's verification pass confirmed
        // the zeroing actually lands, not just the CSS rule existing.
        let secondary_row = gtk::FlowBox::new();
        secondary_row.add_css_class("tight-flow");
        secondary_row.set_selection_mode(gtk::SelectionMode::None);
        secondary_row.set_row_spacing(8);
        secondary_row.set_column_spacing(14);
        secondary_row.set_homogeneous(false);
        secondary_row.set_halign(gtk::Align::Start);
        secondary_row.set_margin_bottom(10);
        secondary_row.insert(&hold_cluster, -1);
        secondary_row.insert(&repolock_cluster, -1);
        secondary_row.insert(&mark_cluster, -1);
        secondary_row.insert(&oneshot_cluster, -1);

        // ── Split: metadata column (action buttons on top) | dependency
        // column — both start at the same y, so Dependencies lines up
        // with the Install/Download Only row instead of sitting below
        // it. `action_row`/`secondary_row` live in `left_col` rather
        // than in `metadata_scroll`'s scrollable content, so they stay
        // pinned in view even if the description/SIZE/SOURCE grid is
        // long enough to scroll.
        let split = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        split.set_vexpand(true);
        split.set_hexpand(true);

        let left_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // Every container from here down to the individual SIZE/
        // INSTALLATION/SOURCE groups gets *both* `hexpand(true)` and
        // `halign(Fill)` set explicitly and directly on itself — never
        // relying on GTK's expand-computation bubbling up from a
        // descendant, and never relying on a plain-`Box` child's default
        // `halign: Fill` alone. Two concrete reasons this matters here:
        // (1) `GtkScrolledWindow` wraps its child in an implicit
        // `GtkViewport`, which does not reliably bubble a descendant's
        // `hexpand` back out to decide the `ScrolledWindow`'s own width
        // — so `metadata_scroll` needs its *own* `hexpand(true)`, not
        // just its child's. (2) a plain `gtk::Box` child's `halign:
        // Fill` only has something to fill if the box itself was told
        // to claim the extra width in the first place (`hexpand`) —
        // `halign` alone, without `hexpand`, still just settles at
        // natural width in a container that hands out surplus space by
        // expand flags. Setting both, on every level, removes any
        // dependency on which of these two mechanisms actually carries
        // the width through in a given GTK version/theme.
        left_col.set_hexpand(true);
        left_col.set_halign(gtk::Align::Fill);
        // Without this, a plain `gtk::Box` only claims its natural
        // (minimum-needed) height from `split`'s vexpand row instead of
        // filling it — `metadata_scroll` then gets squeezed down to a
        // sliver, clipping SIZE/INSTALLATION/SOURCE out of view even
        // though they're still genuinely there (this was the actual
        // cause of an apparent "SIZE/SOURCE vanished" regression while
        // developing this layout — not a visibility bug in
        // `rebuild_kv_group`, a sizing one here).
        left_col.set_vexpand(true);
        left_col.append(&action_row);
        left_col.append(&secondary_row);

        let metadata_scroll = gtk::ScrolledWindow::new();
        metadata_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        // As a child of the *horizontal* `split` (its old parent),
        // height was the cross axis, filled automatically via the
        // default `valign: Fill` — no `vexpand` needed. Now it's a
        // child of the *vertical* `left_col`, where height is the main
        // axis: without `vexpand`, it only claims its own small natural
        // height and leaves the rest of `left_col` as dead space,
        // clipping SIZE/INSTALLATION/SOURCE out of view below the fold
        // rather than actually hiding them.
        metadata_scroll.set_vexpand(true);
        metadata_scroll.set_hexpand(true);
        metadata_scroll.set_halign(gtk::Align::Fill);
        let metadata_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        metadata_col.set_width_request(320);
        metadata_col.set_hexpand(true);
        metadata_col.set_halign(gtk::Align::Fill);

        // ── Header: name + version + state chip + tag chips, over the
        // description ──
        let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header_row.set_hexpand(true);
        header_row.set_halign(gtk::Align::Fill);
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
        desc.set_hexpand(true);
        desc.set_halign(gtk::Align::Fill);
        metadata_col.append(&desc);

        // ── Unframed key/value grid: SIZE, then INSTALLATION, then
        // SOURCE, stacked in reading order — groups without data stay
        // hidden. (Previously SOURCE sat beside SIZE/INSTALLATION in a
        // second column; stacking single-column keeps every group's
        // left edge — headers and values alike — on the same line
        // metadata_col itself starts at, rather than SOURCE's edge
        // depending on how wide the left column happened to be.)
        let kv_stack = gtk::Box::new(gtk::Orientation::Vertical, 10);
        kv_stack.set_hexpand(true);
        kv_stack.set_halign(gtk::Align::Fill);

        let size_group = gtk::Box::new(gtk::Orientation::Vertical, 2);
        size_group.set_visible(false);
        size_group.set_hexpand(true);
        size_group.set_halign(gtk::Align::Fill);
        let install_group = gtk::Box::new(gtk::Orientation::Vertical, 2);
        install_group.set_visible(false);
        install_group.set_hexpand(true);
        install_group.set_halign(gtk::Align::Fill);
        let source_group = gtk::Box::new(gtk::Orientation::Vertical, 2);
        source_group.set_visible(false);
        source_group.set_hexpand(true);
        source_group.set_halign(gtk::Align::Fill);
        kv_stack.append(&size_group);
        kv_stack.append(&install_group);
        kv_stack.append(&source_group);
        metadata_col.append(&kv_stack);

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

        metadata_scroll.set_child(Some(&metadata_col));
        left_col.append(&metadata_scroll);
        split.append(&left_col);
        split.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // Each section row takes its natural height (lists scroll when
        // the pane is smaller than the content; leftover space collects
        // at the bottom). Rows are homogeneous horizontally, so the
        // column boundary (drawn by `.vsep`) sits at 50% in every row.
        let dependency_col = gtk::Box::new(gtk::Orientation::Vertical, 8);
        dependency_col.set_hexpand(true);

        let deps_rdeps_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        deps_rdeps_row.set_homogeneous(true);

        let deps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        deps_col.set_hexpand(true);
        let deps_header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let deps_header = gtk::Label::new(Some("DEPENDENCIES"));
        deps_header.set_xalign(0.0);
        deps_header.add_css_class("section-header");
        // `.section-header` itself carries zero left padding (needed so
        // it flush-aligns with the zero-margin kv-group rows elsewhere
        // in this file) — but this header's own rows come from
        // `dialog_util::text_list_row`, which indents each row's label
        // 8px via `margin_start`. Match that here so header and rows
        // still share a left edge.
        deps_header.set_margin_start(8);
        let deps_pill = count_pill();
        deps_header_row.append(&deps_header);
        deps_header_row.append(&deps_pill);
        deps_col.append(&deps_header_row);
        let deps_scroll = gtk::ScrolledWindow::new();
        deps_scroll.set_propagate_natural_height(true);
        let deps_list = gtk::ListBox::new();
        deps_list.set_selection_mode(gtk::SelectionMode::None);
        let deps_ph = gtk::Label::new(Some("Select a package"));
        deps_ph.add_css_class("dim-label");
        deps_ph.set_margin_top(12);
        deps_list.set_placeholder(Some(&deps_ph));
        deps_scroll.set_child(Some(&deps_list));
        deps_col.append(&deps_scroll);
        deps_rdeps_row.append(&deps_col);

        let rdeps_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rdeps_col.set_hexpand(true);
        rdeps_col.add_css_class("vsep");
        let rdeps_header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let rdeps_header = gtk::Label::new(Some("REVERSE DEPENDENCIES"));
        rdeps_header.set_xalign(0.0);
        rdeps_header.add_css_class("section-header");
        // See the matching comment on `deps_header` above.
        rdeps_header.set_margin_start(8);
        let rdeps_pill = count_pill();
        rdeps_header_row.append(&rdeps_header);
        rdeps_header_row.append(&rdeps_pill);
        rdeps_col.append(&rdeps_header_row);
        let rdeps_scroll = gtk::ScrolledWindow::new();
        rdeps_scroll.set_propagate_natural_height(true);
        let rdeps_list = gtk::ListBox::new();
        rdeps_list.set_selection_mode(gtk::SelectionMode::None);
        let rdeps_ph = gtk::Label::new(Some("Select a package"));
        rdeps_ph.add_css_class("dim-label");
        rdeps_ph.set_margin_top(12);
        rdeps_list.set_placeholder(Some(&rdeps_ph));
        rdeps_scroll.set_child(Some(&rdeps_list));
        rdeps_col.append(&rdeps_scroll);
        deps_rdeps_row.append(&rdeps_col);

        dependency_col.append(&deps_rdeps_row);

        // Provides/Requires/Exports/Conflicts/Replaces rows are appended
        // here per selection; see `populate_provides_conflicts`.
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
            hold_cluster,
            repolock_cluster,
            mark_cluster,
            oneshot_cluster,
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
            deps_pill,
            rdeps_pill,
            deps_col,
            rdeps_col,
            dependency_col,
            relation_rows: RefCell::new(Vec::new()),
            deps_list,
            rdeps_list,
            deps_placeholder: deps_ph,
            rdeps_placeholder: rdeps_ph,
            files_expander,
            files_list,
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

    // Hold/unhold and the other secondary actions aren't a queued mark —
    // each needs a privileged action of its own, so this pane just
    // reports the request and lets the caller (which owns the
    // `Transaction`) carry it out.
    wire_bool_action_button(inner, &inner.btn_hold, |i| &i.on_hold_requested, true);
    wire_bool_action_button(inner, &inner.btn_unhold, |i| &i.on_hold_requested, false);
    wire_action_button(inner, &inner.btn_reinstall, |i| &i.on_reinstall_requested);
    wire_action_button(inner, &inner.btn_reconfigure, |i| {
        &i.on_reconfigure_requested
    });
    wire_action_button(inner, &inner.btn_download, |i| &i.on_download_requested);
    wire_bool_action_button(
        inner,
        &inner.btn_repolock,
        |i| &i.on_repolock_requested,
        true,
    );
    wire_bool_action_button(
        inner,
        &inner.btn_repounlock,
        |i| &i.on_repolock_requested,
        false,
    );
    wire_bool_action_button(
        inner,
        &inner.btn_mark_manual,
        |i| &i.on_automatic_requested,
        false,
    );
    wire_bool_action_button(
        inner,
        &inner.btn_mark_auto,
        |i| &i.on_automatic_requested,
        true,
    );
}

/// Shared by every secondary action button that just reports a
/// no-argument request (Reinstall/Reconfigure/Download Only): read the
/// current package, fan the request out to `get_cbs`'s listeners.
fn wire_action_button(
    inner: &Rc<Inner>,
    btn: &gtk::Button,
    get_cbs: impl Fn(&Inner) -> &ActionRequestedCbs + 'static,
) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        for f in get_cbs(&inner).borrow().iter() {
            f(name.clone());
        }
    });
}

/// Same as [`wire_action_button`], but for the paired on/off secondary
/// actions (Hold/Release Hold, Repo-Lock/Release, Mark Manual/Auto) that
/// share one callback list and differ only in the bool they pass.
fn wire_bool_action_button(
    inner: &Rc<Inner>,
    btn: &gtk::Button,
    get_cbs: impl Fn(&Inner) -> &HoldRequestedCbs + 'static,
    value: bool,
) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        for f in get_cbs(&inner).borrow().iter() {
            f(name.clone(), value);
        }
    });
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
        inner.btn_unmark.set_visible(false);
        set_cluster_visible(&inner.hold_cluster, false);
        set_cluster_visible(&inner.repolock_cluster, false);
        set_cluster_visible(&inner.mark_cluster, false);
        set_cluster_visible(&inner.oneshot_cluster, false);
        return;
    };

    // Hold/unhold/reinstall/reconfigure/download/repolock are all
    // orthogonal to the pending-mark system (applied immediately, not
    // queued), so their visibility only depends on whether the package
    // is installed at all — not on `pkg.mark`. (Mark-manual/automatic's
    // visibility depends on data only `show_package_impl` has fetched —
    // see there.) Hold/Unhold and Repo-Lock/Release now both stay
    // visible together as a pair; `set_segment_state` picks which one
    // reflects the current state (tinted, insensitive) vs. which one is
    // the live action.
    let installed = pkg.state != PkgState::NotInstalled;
    inner.btn_hold.set_visible(installed);
    inner.btn_unhold.set_visible(installed);
    set_cluster_visible(&inner.hold_cluster, installed);
    if pkg.state == PkgState::OnHold {
        set_segment_state(&inner.btn_hold, &inner.btn_unhold);
    } else {
        set_segment_state(&inner.btn_unhold, &inner.btn_hold);
    }
    inner.btn_reinstall.set_visible(installed);
    inner.btn_reconfigure.set_visible(installed);
    inner.btn_download.set_visible(!installed);
    // Unlike the other three clusters, this one always has *something*
    // visible once a package is selected — Reinstall+Reconfigure when
    // installed, Download Only otherwise — so it's unconditionally
    // shown here (the `pkg == None` branch above is the only place it
    // needs to be hidden).
    set_cluster_visible(&inner.oneshot_cluster, true);
    inner.btn_repolock.set_visible(installed);
    inner.btn_repounlock.set_visible(installed);
    set_cluster_visible(&inner.repolock_cluster, installed);
    if pkg.is_repolocked {
        set_segment_state(&inner.btn_repolock, &inner.btn_repounlock);
    } else {
        set_segment_state(&inner.btn_repounlock, &inner.btn_repolock);
    }

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

/// One independent field, styled like a Dependencies column: header +
/// count pill + its own scrollable list, one row per item.
fn relation_field(title: &str, items: &[String]) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.set_hexpand(true);

    let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("section-header");
    // See the matching comment on `deps_header` in `DetailPane::new` —
    // this column's rows also come from `text_list_row`'s 8px indent.
    header.set_margin_start(8);
    let pill = count_pill();
    set_count(&pill, Some(items.len()));
    header_row.append(&header);
    header_row.append(&pill);
    col.append(&header_row);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    populate(&list, Some(items.to_vec()));
    scroll.set_child(Some(&list));
    col.append(&scroll);

    col
}

/// Rebuilds the Provides/Requires/Exports/Conflicts/Replaces area:
/// only non-empty fields, two per row, each fully independent.
fn populate_provides_conflicts(
    inner: &Rc<Inner>,
    extra: Option<&crate::backend::package::PackageExtraInfo>,
) {
    for row in inner.relation_rows.borrow_mut().drain(..) {
        inner.dependency_col.remove(&row);
    }
    let Some(extra) = extra else { return };

    let fields: Vec<(&str, &[String])> = [
        ("PROVIDES", extra.provides.as_slice()),
        ("REQUIRES", extra.shlib_requires.as_slice()),
        ("EXPORTS", extra.shlib_provides.as_slice()),
        ("CONFLICTS", extra.conflicts.as_slice()),
        ("REPLACES", extra.replaces.as_slice()),
    ]
    .into_iter()
    .filter(|(_, items)| !items.is_empty())
    .collect();

    for pair in fields.chunks(2) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_homogeneous(true);
        for (i, (title, items)) in pair.iter().enumerate() {
            let field = relation_field(title, items);
            if i > 0 {
                field.add_css_class("vsep");
            }
            row.append(&field);
        }
        inner.dependency_col.append(&row);
        inner.relation_rows.borrow_mut().push(row);
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
    set_cluster_visible(&inner.mark_cluster, false);
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
                // to load) gets neither, matching "missing data ⇒ not
                // rendered" (unlike Hold/Repo-Lock this data isn't known
                // synchronously, so the pair starts hidden and only
                // appears once this async reply lands).
                let auto_flag = extra.as_ref().filter(|e| e.has_automatic_install);
                let mark_pair_visible = auto_flag.is_some();
                inner.btn_mark_manual.set_visible(mark_pair_visible);
                inner.btn_mark_auto.set_visible(mark_pair_visible);
                set_cluster_visible(&inner.mark_cluster, mark_pair_visible);
                if let Some(flag) = auto_flag {
                    if flag.automatic_install {
                        set_segment_state(&inner.btn_mark_auto, &inner.btn_mark_manual);
                    } else {
                        set_segment_state(&inner.btn_mark_manual, &inner.btn_mark_auto);
                    }
                }

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
