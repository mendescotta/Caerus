# Minimal Sidebar View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a third sidebar display state — a narrow icon-only "rail" — alongside today's Full and Hidden states, cycled via the header button/F9 and independently switchable in the View menu.

**Architecture:** `FilterSidebar` (in `filter_sidebar.rs`) gains a `set_minimal(bool)` method that shrinks its width, swaps each section's header for a thin separator, and hides row text labels in place (rows keep the same `gtk::ListBoxRow` identity — index-based dispatch, selection, and rebuild logic are untouched). `window.rs` gains persisted state (`WindowGeometry::sidebar_visible`/`sidebar_minimal`) and a small state machine (`apply_sidebar_mode` / `cycle_sidebar_mode`) that keeps the header button, F9, and two independent View-menu switches ("Sidebar", "Minimal Sidebar") all in sync.

**Tech Stack:** Rust, GTK4 (gtk-rs), no new dependencies.

## Global Constraints

- Reuse existing icon names only — no new bundled icon assets (see project history: a prior bug meant new icons under `hicolor/symbolic/<context>/` silently failed to resolve; reusing proven names sidesteps that risk class entirely). The new repository-row icon is the standard stock name `folder-remote-symbolic`.
- The header toggle button keeps its existing icon (`sidebar-show-symbolic`) in all three states; only its tooltip text changes.
- No new automated tests are expected for the GTK layout changes themselves (this project has no existing tests for analogous features like section-visibility toggling or detail-pane docking) — verify via `cargo build`/`clippy`/`fmt` plus manual live testing, per the design doc's Testing section.
- Every task must leave the workspace building cleanly with **and** without `--features caerus/adwaita` (this project's standard dual-config verification).
- Design doc: `docs/superpowers/specs/2026-08-11-minimal-sidebar-design.md` — consult it for full rationale; this plan implements it task-by-task.

---

### Task 1: Row helpers gain tooltips and repository rows gain an icon

**Files:**
- Modify: `caerus/src/ui/filter_sidebar.rs:184-246` (`make_action_row`, `make_row`, `make_custom_filter_row`, `make_text_row`)
- Modify: `caerus/src/ui/filter_sidebar.rs:253-286` (`build_repo_row`)

**Interfaces:**
- Consumes: nothing new.
- Produces: every row built by `make_row`/`make_action_row`/`make_custom_filter_row`/`make_text_row`/`build_repo_row` now has (a) a tooltip on the returned `gtk::ListBoxRow` carrying its label text, and (b) for the two repo-row builders, a leading `gtk::Image::from_icon_name("folder-remote-symbolic")` child appended to the row's internal `gtk::Box` **before** the label — so every row in every section now follows the same "icon child, then label child" shape, which Task 3's generic rail-toggle helper depends on (it locates the label via `row_box.last_child()`).

- [ ] **Step 1: Add tooltips to the icon+label row builders**

In `make_row` (`filter_sidebar.rs:188-203`), after the row is constructed, set a tooltip on `row` itself (not the label — the label will be hidden in rail mode, but the row stays visible and clickable):

```rust
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
    row.set_tooltip_text(Some(label));
    row
}
```

Apply the same one-line addition (`row.set_tooltip_text(Some(label))` / `Some(name)`) to `make_custom_filter_row` (`filter_sidebar.rs:208-228`, use `name`) right before its final `row` return. `make_action_row` (`filter_sidebar.rs:184-186`) just delegates to `make_row`, so it needs no change.

- [ ] **Step 2: Give repository rows an icon and move their tooltip to the row**

Rewrite `make_text_row` (`filter_sidebar.rs:233-246`, currently label-only, used for the static "All Repositories" row) to match the icon+label shape and add a tooltip:

```rust
fn make_text_row(label: &str) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.set_margin_start(8);
    row_box.set_margin_end(8);
    row_box.set_margin_top(5);
    row_box.set_margin_bottom(5);
    row_box.append(&gtk::Image::from_icon_name("folder-remote-symbolic"));
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    row_box.append(&l);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row.set_tooltip_text(Some(label));
    row
}
```

Rewrite `build_repo_row` (`filter_sidebar.rs:253-286`) the same way — wrap the existing label in a `row_box` with a leading icon, and move the stale/non-stale tooltip logic from the label (`l.set_tooltip_text(...)`) onto the row, and move the right-click rename gesture from the label onto `row_box` (so renaming still works when the label is hidden in rail mode):

```rust
fn build_repo_row(inner: &Rc<Inner>, url: String, stale: bool) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.set_margin_start(8);
    row_box.set_margin_end(8);
    row_box.set_margin_top(5);
    row_box.set_margin_bottom(5);
    row_box.append(&gtk::Image::from_icon_name("folder-remote-symbolic"));

    let l = gtk::Label::new(Some(&repo_display_text(inner, &url)));
    l.set_xalign(0.0);
    l.set_hexpand(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    if stale {
        l.add_css_class("dim-label");
    }
    row_box.append(&l);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    if stale {
        row.set_tooltip_text(Some(&format!(
            "{url}\nNot currently configured — packages were installed from it in the past"
        )));
    } else {
        row.set_tooltip_text(Some(&url));
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
    let inner = inner.clone();
    let label = l.clone();
    gesture.connect_pressed(move |g, _n_press, _x, _y| {
        let Some(widget) = g.widget() else { return };
        let root = widget.root().and_downcast::<gtk::Window>();
        show_rename_dialog(root, &inner, url.clone(), &label);
    });
    row_box.add_controller(gesture);

    row
}
```

- [ ] **Step 3: Build and check with both feature configs**

Run: `cd /home/gui/Projects/caerus && cargo build --workspace && cargo build --workspace --features caerus/adwaita`
Expected: both succeed with no errors. `cargo clippy --workspace --all-targets` and `cargo clippy --workspace --all-targets --features caerus/adwaita` both clean (no new warnings).

- [ ] **Step 4: Commit**

```bash
git add caerus/src/ui/filter_sidebar.rs
git commit -m "$(cat <<'EOF'
filter_sidebar: give every row a tooltip and repo rows an icon

Prep for minimal-sidebar rail mode: every row now follows the same
icon-then-label shape, and carries a tooltip on the always-visible row
container instead of the label, so rows stay identifiable and
right-clickable once labels are hidden in rail mode.
EOF
)"
```

---

### Task 2: Section header gets a rail separator

**Files:**
- Modify: `caerus/src/ui/filter_sidebar.rs:89-179` (`SectionWidgets`, `build_section`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `SectionWidgets` gains a `rail_separator: gtk::Separator` field (hidden by default). `build_section` now inserts it between the header and the revealer. Task 3 toggles `header.set_visible`/`rail_separator.set_visible` as a pair.

- [ ] **Step 1: Add the field and build it**

In `SectionWidgets` (`filter_sidebar.rs:93-97`), add a field and rename `header` local into a struct field so Task 3 can toggle it:

```rust
struct SectionWidgets {
    container: gtk::Box,
    revealer: gtk::Revealer,
    triangle: gtk::Label,
    header: gtk::Box,
    rail_separator: gtk::Separator,
}
```

In `build_section` (`filter_sidebar.rs:137-179`), after the existing `header` box is built (through the gesture-controller block) and before the final `container.append` calls, build the separator and change the trailing appends and return value:

```rust
let rail_separator = gtk::Separator::new(gtk::Orientation::Horizontal);
rail_separator.set_visible(false);
rail_separator.set_margin_top(4);
rail_separator.set_margin_bottom(4);

container.append(&header);
container.append(&rail_separator);
container.append(&revealer);

SectionWidgets {
    container,
    revealer,
    triangle,
    header,
    rail_separator,
}
```

(Remove the old `container.append(&header); container.append(&revealer);` pair that currently sits right before the old `SectionWidgets { ... }` return — replaced by the block above.)

- [ ] **Step 2: Build and check**

Run: `cd /home/gui/Projects/caerus && cargo build --workspace && cargo build --workspace --features caerus/adwaita`
Expected: both succeed (the `header` local no longer being consumed-then-dropped is fine — it's now moved into the struct).

- [ ] **Step 3: Commit**

```bash
git add caerus/src/ui/filter_sidebar.rs
git commit -m "$(cat <<'EOF'
filter_sidebar: section headers gain a hidden rail separator

Each collapsible section now owns a thin gtk::Separator alongside its
header, ready to swap in as the header's rail-mode replacement.
EOF
)"
```

---

### Task 3: `FilterSidebar::set_minimal` and rail-mode row toggling

**Files:**
- Modify: `caerus/src/ui/filter_sidebar.rs:99-119` (`Inner`)
- Modify: `caerus/src/ui/filter_sidebar.rs:405-669` (`FilterSidebar::new`)
- Modify: `caerus/src/ui/filter_sidebar.rs:671-` (public API, after `set_expanded`)

**Interfaces:**
- Consumes: `SectionWidgets.header`/`rail_separator` from Task 2; the icon-then-label row shape from Task 1.
- Produces: `pub fn FilterSidebar::set_minimal(&self, minimal: bool)` — window.rs (Task 6) calls this to switch rail mode on/off. No getter is added (window.rs tracks the current minimal flag itself, per the design doc).

- [ ] **Step 1: Store the extra ListBox references and rail state in `Inner`**

`Inner` (`filter_sidebar.rs:99-119`) currently stores `preset_lb` and `repo_lb` but not `maint_lb`/`tools_lb`/`edit_filters_lb` — add them, plus rail bookkeeping:

```rust
struct Inner {
    widget: gtk::Box,
    preset_lb: gtk::ListBox,
    edit_filters_lb: gtk::ListBox,
    custom_filters: Rc<RefCell<CustomFilters>>,
    repo_lb: gtk::ListBox,
    repo_names: RefCell<Vec<String>>,
    all_repos: RefCell<Vec<(String, bool)>>,
    show_stale: std::cell::Cell<bool>,
    display_names: RefCell<RepoNames>,
    maint_lb: gtk::ListBox,
    tools_lb: gtk::ListBox,
    on_filter_changed: FilterChangedCbs,
    on_repository_changed: RepositoryChangedCbs,
    on_action: ActionCbs,
    sections: [SectionWidgets; 4],
    /// Snapshot of each section's `is_expanded()` taken when entering
    /// minimal mode, restored when leaving it (rail mode force-expands
    /// every visible section since a hidden disclosure triangle can't be
    /// clicked).
    expanded_snapshot: RefCell<[bool; 4]>,
}
```

In `FilterSidebar::new`'s `Inner { ... }` construction (`filter_sidebar.rs:551-564`), add the two new fields and the snapshot default:

```rust
let inner = Rc::new(Inner {
    widget,
    preset_lb: preset_lb.clone(),
    edit_filters_lb: edit_filters_lb.clone(),
    custom_filters,
    repo_lb: repo_lb.clone(),
    repo_names: RefCell::new(Vec::new()),
    all_repos: RefCell::new(Vec::new()),
    show_stale: std::cell::Cell::new(true),
    display_names: RefCell::new(RepoNames::load()),
    maint_lb: maint_lb.clone(),
    tools_lb: tools_lb.clone(),
    on_filter_changed: RefCell::new(Vec::new()),
    on_repository_changed: RefCell::new(Vec::new()),
    on_action: RefCell::new(Vec::new()),
    sections: [filters_section, repos_section, maint_section, tools_section],
    expanded_snapshot: RefCell::new([true; 4]),
});
```

(`edit_filters_lb`, `maint_lb`, `tools_lb` already exist as local `gtk::ListBox` variables earlier in `new` — `.clone()` them the same way `preset_lb`/`repo_lb` already are.)

- [ ] **Step 2: Add the generic rail-toggle helper**

Add this free function near `rebuild_repo_rows` at the bottom of `filter_sidebar.rs`:

```rust
/// Hides (or restores) every row's label in `listbox` and centers the
/// remaining icon. Relies on every row built by this module having its
/// label as the last child of the row's content box (see `make_row` /
/// `build_repo_row` etc.) — safe because Task 1 made that shape uniform
/// across every row builder in this file.
fn set_rows_minimal(listbox: &gtk::ListBox, minimal: bool) {
    let mut child = listbox.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        let Some(row) = widget.downcast_ref::<gtk::ListBoxRow>() else {
            continue;
        };
        let Some(row_box) = row.child() else { continue };
        if let Some(label) = row_box.last_child() {
            label.set_visible(!minimal);
        }
        row_box.set_halign(if minimal {
            gtk::Align::Center
        } else {
            gtk::Align::Fill
        });
    }
}
```

- [ ] **Step 3: Add `FilterSidebar::set_minimal`**

Add these two module-level consts near the top of `filter_sidebar.rs` (e.g. right above the `impl FilterSidebar` block), not inside the `impl`:

```rust
/// Rail width when minimal; the sidebar's normal width otherwise (see
/// `FilterSidebar::new`'s `set_width_request(190)`).
const RAIL_WIDTH: i32 = 56;
const FULL_WIDTH: i32 = 190;
```

Then add this method to `impl FilterSidebar` (`filter_sidebar.rs:671-754`), after `set_expanded`:

```rust
/// Switches between the full labeled sidebar and a narrow icon-only
/// rail. Section headers/triangles are replaced by thin separators;
/// every row's label hides and its icon centers; sections force-expand
/// (a hidden triangle can't be clicked to re-expand) with their prior
/// expanded state restored on the way back out.
pub fn set_minimal(&self, minimal: bool) {
    self.inner
        .widget
        .set_width_request(if minimal { RAIL_WIDTH } else { FULL_WIDTH });

    if minimal {
        *self.inner.expanded_snapshot.borrow_mut() =
            std::array::from_fn(|i| self.inner.sections[i].revealer.reveals_child());
    }

    for section in &self.inner.sections {
        section.header.set_visible(!minimal);
        section.rail_separator.set_visible(minimal);
        if minimal {
            section.revealer.set_reveal_child(true);
        }
    }

    if !minimal {
        let snapshot = *self.inner.expanded_snapshot.borrow();
        for (section, expanded) in self.inner.sections.iter().zip(snapshot) {
            section.revealer.set_reveal_child(expanded);
        }
    }

    set_rows_minimal(&self.inner.preset_lb, minimal);
    set_rows_minimal(&self.inner.edit_filters_lb, minimal);
    set_rows_minimal(&self.inner.repo_lb, minimal);
    set_rows_minimal(&self.inner.maint_lb, minimal);
    set_rows_minimal(&self.inner.tools_lb, minimal);
}
```

- [ ] **Step 4: Build and check**

Run: `cd /home/gui/Projects/caerus && cargo build --workspace && cargo build --workspace --features caerus/adwaita`
Expected: both succeed. If `edit_filters_lb`/`maint_lb`/`tools_lb` weren't previously cloned before being moved into `inner_box`/sections, the compiler will flag a use-after-move — clone them at their original definition sites the same way `preset_lb.clone()` already is, before they're appended into their section's content box.
Run: `cargo clippy --workspace --all-targets` and with `--features caerus/adwaita`.
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add caerus/src/ui/filter_sidebar.rs
git commit -m "$(cat <<'EOF'
filter_sidebar: add set_minimal for icon-only rail mode

New FilterSidebar::set_minimal(bool) shrinks the sidebar to a 56px
rail, swaps section headers for thin separators, hides row labels
(centering their icons), and force-expands sections while minimal,
restoring prior expand state on the way back to full mode.
EOF
)"
```

---

### Task 4: Persist sidebar visibility and minimal-mode state

**Files:**
- Modify: `caerus/src/ui/window.rs:76-291` (`WindowGeometry`, `Default`, `load`, `save`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `WindowGeometry` gains `sidebar_visible: bool` (default `true`) and `sidebar_minimal: bool` (default `false`), loaded from and saved to `window-state.conf` as keys `visible_sidebar` and `sidebar_minimal`. Task 6 reads these two fields at launch and writes them at shutdown.

- [ ] **Step 1: Add the fields and their defaults**

In the `WindowGeometry` struct (`window.rs:76-103`), add after `stale_repos_visible`:

```rust
    /// Whether the sidebar is shown at all — previously not persisted
    /// (always started shown); now tracked like `detail_pane_visible`.
    sidebar_visible: bool,
    /// Whether the (visible) sidebar renders as the narrow icon rail
    /// instead of the full labeled layout. Kept even while the sidebar
    /// is hidden, so re-showing it via the View menu's "Sidebar" switch
    /// resumes whichever mode was last active.
    sidebar_minimal: bool,
```

In `impl Default for WindowGeometry` (`window.rs:109-126`), add after `stale_repos_visible: true,`:

```rust
            sidebar_visible: true,
            sidebar_minimal: false,
```

- [ ] **Step 2: Parse the two new keys in `load`**

In `WindowGeometry::load` (`window.rs:181-257`), add a new top-level key check alongside the existing `sync_at_launch`/`search_name_only_default`/`vertical_panel` checks (right after the `vertical_panel` block, before the generic `value.parse::<i32>().map(|b| b != 0)` block at line 213):

```rust
            if key == "sidebar_minimal" {
                if let Ok(b) = value.parse::<i32>() {
                    geometry.sidebar_minimal = b != 0;
                }
                continue;
            }
```

Inside the `strip_prefix("visible_")` match arm (`window.rs:225-239`), add a `"sidebar"` case alongside `"detail_pane"`/`"status_bar"`/`"stale_repos"`:

```rust
                        "sidebar" => {
                            geometry.sidebar_visible = b;
                            continue;
                        }
```

- [ ] **Step 3: Write the two new keys in `save`**

In `WindowGeometry::save` (`window.rs:259-290`), add `sidebar_minimal` to the first `format!` call (alongside `vertical_panel`):

```rust
        let mut contents = format!(
            "width={}\nheight={}\nsidebar_pos={}\ndetail_pos={}\nsync_at_launch={}\nsearch_name_only_default={}\nvertical_panel={}\nsidebar_minimal={}\n",
            self.width,
            self.height,
            self.sidebar_pos,
            self.detail_pos,
            i32::from(self.sync_at_launch),
            i32::from(self.search_name_only_default),
            i32::from(self.vertical_panel),
            i32::from(self.sidebar_minimal)
        );
```

And add `visible_sidebar` to the trailing `visible_detail_pane`/`visible_status_bar`/`visible_stale_repos` block:

```rust
        contents.push_str(&format!(
            "visible_detail_pane={}\nvisible_status_bar={}\nvisible_stale_repos={}\nvisible_sidebar={}\n",
            i32::from(self.detail_pane_visible),
            i32::from(self.status_bar_visible),
            i32::from(self.stale_repos_visible),
            i32::from(self.sidebar_visible)
        ));
```

- [ ] **Step 4: Build and check**

Run: `cd /home/gui/Projects/caerus && cargo build --workspace && cargo build --workspace --features caerus/adwaita`
Expected: compile error — `WindowGeometry { ... }` construction at `window.rs:1452-1475` (the `connect_close_request` handler) is missing the two new fields (struct literals don't have defaults). This is expected; Task 6 fixes it by adding the missing fields there. Confirm the *only* new error is that missing-fields error (no other breakage), then stop — do not fix it in this task, that beloutlongs to Task 6's wiring.

- [ ] **Step 5: Commit**

```bash
git add caerus/src/ui/window.rs
git commit -m "$(cat <<'EOF'
window: persist sidebar visibility and minimal-mode state

WindowGeometry gains sidebar_visible/sidebar_minimal fields (window-
state.conf keys visible_sidebar/sidebar_minimal). Sidebar visibility
was previously not persisted at all; it now survives restarts like
every other View-menu toggle. Leaves connect_close_request's
WindowGeometry literal intentionally broken — Task 6 wires it up.
EOF
)"
```

---

### Task 5: `switch_row_with` refactor and the two View-menu switches

**Files:**
- Modify: `caerus/src/ui/window.rs:715-739` (`switch_row`)
- Modify: `caerus/src/ui/window.rs:18-71` (`WindowState`)
- Modify: `caerus/src/ui/window.rs:820-833` (View page "Sidebar" row in `populate_menu_popover`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn switch_row_with(switch: &gtk::Switch, label: &str, accel: Option<&str>) -> gtk::Box` (row-building without constructing a fresh switch); `switch_row` becomes a thin wrapper over it. `WindowState` gains `sw_sidebar_visible: gtk::Switch`, `sw_sidebar_minimal: gtk::Switch`, `sidebar_minimal: std::cell::Cell<bool>` — Task 6's `apply_sidebar_mode`/`cycle_sidebar_mode` read/write all three plus the (type-changed) `btn_toggle_sidebar`.

- [ ] **Step 1: Split `switch_row` into a reusable row-builder**

Replace `switch_row` (`window.rs:715-739`) with:

```rust
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
```

- [ ] **Step 2: Add the new `WindowState` fields**

In `WindowState` (`window.rs:18-71`), change the existing field and add two more, right after `btn_toggle_sidebar`:

```rust
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
```

(`btn_toggle_sidebar`'s type changes from `gtk::ToggleButton` to `gtk::Button` — a plain button, since a boolean toggle can't represent three states.)

- [ ] **Step 3: Build the button and switches before `WindowState` construction**

In `build_window` (`window.rs:309-313`), change the button construction:

```rust
    let btn_toggle_sidebar = gtk::Button::new();
    btn_toggle_sidebar.set_icon_name("sidebar-show-symbolic");
    btn_toggle_sidebar.set_tooltip_text(Some("Show/hide the filter sidebar"));
    header.pack_start(&btn_toggle_sidebar);
```

(Drop `set_active(true)` — `gtk::Button` has no such property; initial state is applied explicitly in Task 6.)

Remove the old wiring block right after `let sidebar = FilterSidebar::new();` (`window.rs:382-388`):

```rust
    {
        let sidebar_widget = sidebar.widget().clone();
        btn_toggle_sidebar.connect_toggled(move |btn| {
            sidebar_widget.set_visible(btn.is_active());
        });
    }
```

Delete it entirely — Task 6 replaces it with a `connect_clicked` handler added inside `wire_up`, once `state` exists.

Just before the `WindowState { ... }` construction (`window.rs:451`), add:

```rust
    let sw_sidebar_visible = gtk::Switch::new();
    let sw_sidebar_minimal = gtk::Switch::new();
```

And inside the `WindowState { ... }` literal (`window.rs:451-481`), add the three new fields (next to the existing `btn_toggle_sidebar: btn_toggle_sidebar.clone(),`):

```rust
        btn_toggle_sidebar: btn_toggle_sidebar.clone(),
        sw_sidebar_visible: sw_sidebar_visible.clone(),
        sw_sidebar_minimal: sw_sidebar_minimal.clone(),
        sidebar_minimal: std::cell::Cell::new(geometry.sidebar_minimal),
```

- [ ] **Step 4: Wire the two switches into the View page**

Replace the old bidirectional-bind block for the "Sidebar" row (`window.rs:825-832`):

```rust
    let (sidebar_row, sw_sidebar) = switch_row("Sidebar", Some("F9"));
    state
        .btn_toggle_sidebar
        .bind_property("active", &sw_sidebar, "active")
        .bidirectional()
        .sync_create()
        .build();
    view.append(&sidebar_row);
    view.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
```

with:

```rust
    let sidebar_row = switch_row_with(&state.sw_sidebar_visible, "Sidebar", Some("F9"));
    {
        let state = state.clone();
        state.sw_sidebar_visible.connect_active_notify(move |sw| {
            apply_sidebar_mode(&state, sw.is_active(), state.sidebar_minimal.get());
        });
    }
    view.append(&sidebar_row);

    let minimal_row = switch_row_with(&state.sw_sidebar_minimal, "Minimal Sidebar", None);
    {
        let state = state.clone();
        state.sw_sidebar_minimal.connect_active_notify(move |sw| {
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
```

(`apply_sidebar_mode` is defined in Task 6 — this task will not compile standalone; that's expected and resolved by Task 6. Keep both tasks' diffs in mind if executing out of order, but they're written to land as consecutive commits.)

- [ ] **Step 5: Commit**

```bash
git add caerus/src/ui/window.rs
git commit -m "$(cat <<'EOF'
window: split switch_row, add sidebar mode state to WindowState

switch_row_with lets a row be built around a pre-existing switch, so
the new "Sidebar"/"Minimal Sidebar" View-menu switches can be driven
from outside populate_menu_popover (needed for the header button/F9 to
stay in sync with them). btn_toggle_sidebar becomes a plain Button —
three sidebar states don't fit a boolean ToggleButton. Does not yet
compile standalone; Task 6 adds the apply_sidebar_mode function these
switches call.
EOF
)"
```

---

### Task 6: Sidebar mode state machine, button/F9 wiring, launch/shutdown

**Files:**
- Modify: `caerus/src/ui/window.rs` (new functions near `switch_row_with`; `wire_up`; `wire_keyboard_shortcuts`; `build_window`'s post-construction geometry restore; `connect_close_request`)

**Interfaces:**
- Consumes: `WindowGeometry::sidebar_visible`/`sidebar_minimal` (Task 4); `WindowState::sw_sidebar_visible`/`sw_sidebar_minimal`/`sidebar_minimal`/`btn_toggle_sidebar: gtk::Button` (Task 5); `FilterSidebar::set_minimal` (Task 3).
- Produces: `fn apply_sidebar_mode(state: &Rc<WindowState>, visible: bool, minimal: bool)` and `fn cycle_sidebar_mode(state: &Rc<WindowState>)` — the single funnel every sidebar-mode entry point (button, F9, both switches) goes through.

- [ ] **Step 1: Add the state-machine functions**

Add near `switch_row_with` (top-level function, anywhere before `populate_menu_popover` is fine — e.g. directly above it):

```rust
/// Single funnel for every sidebar-mode change (header button, F9, and
/// both View-menu switches all call into this) — applies the widget
/// state and re-syncs the two switches + button tooltip to match.
/// Calling `Switch::set_active` with the value it already holds does
/// not re-fire `notify::active`, so this can't loop even though the
/// switches' own handlers call back into this function.
fn apply_sidebar_mode(state: &Rc<WindowState>, visible: bool, minimal: bool) {
    state.sidebar.widget().set_visible(visible);
    state.sidebar.set_minimal(minimal);
    state.sidebar_minimal.set(minimal);
    state.btn_toggle_sidebar.set_tooltip_text(Some(match (visible, minimal) {
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
```

- [ ] **Step 2: Wire the header button's click in `wire_up`**

In `wire_up` (`window.rs:1117-`), add near the other button-click wiring (e.g. alongside `btn_update.connect_clicked`/`btn_reload.connect_clicked` around line 1373-1380):

```rust
    {
        let state = state.clone();
        state
            .btn_toggle_sidebar
            .connect_clicked(move |_| cycle_sidebar_mode(&state));
    }
```

- [ ] **Step 3: Replace the F9 handler**

In `wire_keyboard_shortcuts` (`window.rs:1088-1093`), replace:

```rust
            gtk::gdk::Key::F9 => {
                state
                    .btn_toggle_sidebar
                    .set_active(!state.btn_toggle_sidebar.is_active());
                glib::Propagation::Stop
            }
```

with:

```rust
            gtk::gdk::Key::F9 => {
                cycle_sidebar_mode(&state);
                glib::Propagation::Stop
            }
```

- [ ] **Step 4: Apply the loaded geometry at launch**

In `build_window`, in the block that restores section/detail-pane/status-bar visibility from `geometry` (`window.rs:486-507`, right after the `for (i, section) in ... { ... }` loop and before `populate_menu_popover(&state);`), add:

```rust
    apply_sidebar_mode(&state, geometry.sidebar_visible, geometry.sidebar_minimal);
```

This must run before `populate_menu_popover(&state)` (same reasoning as the existing comment above that block: switches read live state via `sync_create`-equivalent — here, `apply_sidebar_mode` explicitly sets `sw_sidebar_visible`/`sw_sidebar_minimal`, so order doesn't actually matter for *this* call, but keeping it grouped with the other geometry-restore calls keeps the launch sequence easy to read). Note `populate_menu_popover` itself still needs to run afterward as before to build the row widgets around these two switches.

- [ ] **Step 5: Fix `connect_close_request`'s `WindowGeometry` literal**

In the `connect_close_request` handler (`window.rs:1450-1479`), add the two new fields to the `WindowGeometry { ... }` literal (this is the compile error Task 4 deliberately left open):

```rust
                sidebar_visible: state.sidebar.widget().get_visible(),
                sidebar_minimal: state.sidebar_minimal.get(),
```

(Place them anywhere in the literal — e.g. right after `stale_repos_visible: state.sidebar.show_stale_repositories(),`.)

- [ ] **Step 6: Build, clippy, fmt — full verification pass**

Run, in order:
```bash
cd /home/gui/Projects/caerus
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo build --workspace --features caerus/adwaita
cargo clippy --workspace --all-targets --features caerus/adwaita -- -D warnings
```
Expected: every command succeeds with no errors/warnings/diffs. This mirrors `.github/workflows/rust.yml`, this project's standard verification pass.

- [ ] **Step 7: Commit**

```bash
git add caerus/src/ui/window.rs
git commit -m "$(cat <<'EOF'
window: wire up the 3-state sidebar mode cycle

apply_sidebar_mode is the single funnel for every sidebar-mode change;
cycle_sidebar_mode implements the header button's/F9's fixed Full ->
Minimal -> Hidden -> Full cycle. Sidebar visibility and minimal-mode
now both survive restart (previously visibility always reset to
shown). Completes the minimal-sidebar feature from
docs/superpowers/specs/2026-08-11-minimal-sidebar-design.md.
EOF
)"
```

---

### Task 7: Manual live verification

**Files:** none (verification only).

**Interfaces:**
- Consumes: the fully wired feature from Tasks 1-6.
- Produces: a confirmed-working feature, or a bug report to fix before considering the plan done.

- [ ] **Step 1: Launch and cycle via the header button**

Run: `cd /home/gui/Projects/caerus && cargo run` (or use the `run` skill if available). Click the sidebar header button three times.
Expected: 1st click → sidebar becomes a narrow icon rail (labels gone, icons centered, thin separators between Filters/Repositories/Maintenance/Tools groups, repository rows now show `folder-remote-symbolic`). 2nd click → sidebar disappears entirely. 3rd click → sidebar returns full-width with labels, and any section that was expanded/collapsed before entering minimal mode is back to that same state.

- [ ] **Step 2: Cycle via F9**

Press F9 three times with focus on the main window.
Expected: identical cycle to Step 1.

- [ ] **Step 3: Verify the View-menu switches independently**

Open the hamburger menu → View. Toggle "Sidebar" off/on, and separately toggle "Minimal Sidebar" off/on (including from a state where the sidebar is currently hidden — turning "Minimal Sidebar" on from Hidden should show the sidebar directly in rail mode).
Expected: both switches always reflect the sidebar's actual current state (including staying in sync with each other and the header button after using either), and behave as described in Step 1's expectations for the resulting visible/minimal combination.

- [ ] **Step 4: Verify tooltips and functionality in rail mode**

With the sidebar in rail mode, hover several icons (a filter preset, a repository, a maintenance action, "Edit Custom Filters…").
Expected: each shows a tooltip naming the row. Click a filter preset icon — the package list filters as expected. Right-click a repository icon — the rename dialog still opens.

- [ ] **Step 5: Verify persistence across restart**

Leave the sidebar in Minimal mode, close the app, relaunch.
Expected: sidebar reopens directly in Minimal mode (not Full). Repeat leaving it Hidden — relaunch should reopen Hidden.

- [ ] **Step 6: Report results**

If any expectation in Steps 1-5 fails, note the exact discrepancy (which step, what happened instead) — do not mark this task complete until every check passes.
