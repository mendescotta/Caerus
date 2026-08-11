# Minimal Sidebar View — Design

## Purpose

Add a third sidebar display state — a narrow icon-only "rail" — alongside
today's Full (labeled rows) and Hidden states, selectable via the existing
header toggle button, F9, and the View menu.

## State model

`WindowGeometry` gains two persisted fields:

- `sidebar_visible: bool` (default `true`) — sidebar visibility was
  previously not persisted at all (always started shown); this is now
  tracked like `detail_pane_visible`.
- `sidebar_minimal: bool` (default `false`)

Three effective states:

| State   | visible | minimal |
|---------|---------|---------|
| Full    | true    | false   |
| Minimal | true    | true    |
| Hidden  | false   | (kept, not shown) |

## Toggle behavior

**Header button.** The `sidebar-show-symbolic` button changes from
`gtk::ToggleButton` to a plain `gtk::Button` (a boolean toggle can't
represent three states). Its click handler cycles:

```
Full -> Minimal -> Hidden -> Full -> ...
```

**F9** invokes the same cycle function (previously it flipped the
`ToggleButton`'s `active` property).

No new icon asset is introduced — the button keeps
`sidebar-show-symbolic` in all three states. Only its tooltip text
changes to name the *next* state reachable by clicking ("Show Minimal
Sidebar (F9)" / "Hide Sidebar (F9)" / "Show Sidebar (F9)"). This
deliberately avoids adding a new bundled symbolic icon, given this
project's prior latent bug where new icons under
`hicolor/symbolic/<context>/` silently failed to resolve (fixed by
moving to `scalable/<context>/`, per project memory) — reusing an
already-proven icon name sidesteps that risk class entirely.

**View menu.** Two independent switch rows in the View popover page:

- "Sidebar" (existing) — bound to `sidebar_visible`.
- "Minimal Sidebar" (new) — bound to `sidebar_minimal`. Turning it on
  while the sidebar is hidden also sets `sidebar_visible = true` (so the
  switch alone can reach Minimal from Hidden). Turning it off returns to
  Full (does not hide the sidebar).

Both switches and the header button/F9 drive the same underlying state,
so they always stay in sync (no independent state machines).

## Rail rendering (minimal mode)

- Sidebar width drops from 190px (`FilterSidebar::new`'s
  `set_width_request(190)`) to a narrow rail width (56px — enough for a
  centered ~24px icon plus margins).
- Each section's header (title label + disclosure triangle) is replaced
  by a thin `gtk::Separator` between the four icon groups (Filters /
  Repositories / Maintenance / Tools), so some visual structure survives
  without full text headers.
- Every row's existing text label is hidden (`set_visible(false)`) and
  its icon is horizontally centered in the row. This requires the
  row-building helpers (`make_row`, `make_action_row`,
  `make_custom_filter_row`, `build_repo_row`) to return/retain a handle
  to the label widget so it can be toggled — currently they return only
  the opaque `gtk::ListBoxRow`.
- Every row gains a tooltip carrying its label text (repo rows already
  have this pattern via `l.set_tooltip_text`; filters/maintenance/tools
  rows do not yet and need it added). Tooltips are harmless in Full mode
  (hover-only) and are how rail-mode rows stay identifiable without
  visible text, per the "thin separators + tooltips" decision.
- Sections are force-expanded while minimal (a disclosure triangle is
  meaningless when hidden). `FilterSidebar::set_minimal(true)` snapshots
  each section's current `is_expanded()` state, then reveals all;
  `set_minimal(false)` restores the snapshot.
- Existing per-section visibility switches (`section_visible` in
  `WindowGeometry`) are unaffected and still apply: a section the user
  has hidden entirely stays hidden in minimal mode too.

## Repository row icon

Repository rows currently render as text-only (`make_text_row` /
`build_repo_row`, no icon). They gain a `folder-remote-symbolic` icon
(a standard stock GTK/Adwaita icon name — not a new bundled asset, so no
icon-cache risk) so they have something to show in the rail. This icon
is added in both Full and Minimal mode for consistency, sitting to the
left of the existing text label.

## Non-goals

- No changes to what filters/actions exist, or their icons.
- No resizable/draggable rail width — fixed at 56px.
- No animation between states beyond the existing revealer transitions
  already used for section expand/collapse.
- Custom filter rows keep their existing kind icon
  (`list-remove-symbolic` / `object-select-symbolic`) in the rail —
  ambiguous without the label, but disambiguated by tooltip, consistent
  with the rest of the rail.

## Testing

- Existing `cargo test --workspace` coverage is UI-adjacent at most
  (`filter_sidebar.rs` has no pure-logic unit tests currently); no new
  automated tests are expected for this GTK widget-layout change. Verify
  via the `run` skill / manual live testing: cycle through all three
  states via header button, F9, and both View-menu switches; confirm
  section-visibility switches still hide sections correctly in minimal
  mode; confirm state survives an app restart (persisted
  `sidebar_visible`/`sidebar_minimal`).
- `cargo clippy --workspace --all-targets` and `cargo fmt --check` clean,
  with and without `--features caerus/adwaita`, per this project's
  standard verification pass.
