# Detail Pane — Design Reference

Working reference for the package detail pane (`caerus/src/ui/detail_pane.rs`),
maintained so a redesign starts from what's already agreed rather than
re-discovering it by trial and error. Update this file whenever the panel's
content, layout, or visual language actually changes — a stale doc is worse
than no doc.

**Redesign in progress (2026-08-11):** target layout is sketched in
`caerus-mockups/roughmockup.png`. Sections 1–2 below are split into
*Current (shipped)* — verified against `detail_pane.rs`/`window.rs` as of
this date — and *Target (redesign)* — the agreed direction, not yet
implemented.

## 1. Content inventory — Current (shipped)

Everything the panel currently shows, in display order. Any redesign must
account for every row here — either keep it, deliberately relocate it, or
note in the "Rejected iterations" log below that it was intentionally cut
and why.

**Header card**
- Package icon (fixed generic glyph — no per-package icon data available)
- Name, version (or `installed → available` when upgradable), state chip
  (`Not installed` / `Installed` / `Upgrade available` / `On hold` / `Broken`)
- Tags (chip row)
- Description (long_desc, falls back to short_desc, falls back to
  "No description available.")
- Primary actions: Install / Upgrade / Remove / Purge / Unmark (mutually
  exclusive visibility based on state + mark)
- Secondary icon actions: Hold toggle, Repo-Lock toggle, Mark Manual/Auto
  toggle, Reinstall, Reconfigure, Download Only — each conditionally visible
  by install state

**Size & Installation card**
- Installed size, Download size, Installed on (date), Auto-installed (Yes/No)

**Source card**
- Repository (respects user's custom display name), License, Maintainer,
  Homepage (clickable link)

**Dependencies card** — count pill + list, rows highlight if installed,
hover tooltip with mini package summary, click jumps to that package

**Reverse Dependencies card** — same shape as Dependencies

**Provides & Requires card** — subgroups (Provides, Requires, Exports,
Conflicts, Replaces) as wrapping tag chips; Conflicts tinted red

**Files card** — lazy-loaded on expand (not fetched on every selection),
capped at 300 rows shown with a "... and N more" trailer, count pill

**Universal rule**: a card with nothing to show is omitted entirely, never
rendered empty. The empty-selection state is a separate centered page, not
a degenerate version of the card stack.

## 1a. Content inventory — Target (redesign)

Confirmed 2026-08-11, from `roughmockup.png`:

- **Provides & Requires splits apart.** The single card with 5 subgroups
  (Provides, Requires, Exports, Conflicts, Replaces) becomes **5 separate
  cards**, one per subgroup — no subgroup stays bundled with another.
- **Name card and Source card are the pairing exception.** Every other
  card carries one kind of information; Name and Source are the two
  cards that sit paired side by side in the top row (see §2a) — this is
  a layout exception, not a content-grouping exception. Name card content
  is unchanged from the current header card (icon, name, version, state
  chip, description, primary actions, secondary icon-button strip).
  Source card content is unchanged (Repository, License, Maintainer,
  Homepage).
- **Tags placement is undecided.** Not shown in the mockup. Before
  deciding where they go (or whether to cut them), audit how many
  installed/available packages actually carry tag data — if it's rare,
  that changes the calculus. Open item, do not implement until resolved.
- All other current cards (Size & Installation, Dependencies, Reverse
  Dependencies, Files) are unchanged in content, per confirmed answers.
- **Card container is optional below the top row.** For Dependencies,
  Reverse Dependencies, and the five Provides/Requires/Exports/Conflicts/
  Replaces sections, the `.card` wrapper (border + background) can be
  dropped in favor of a plainer labeled section, as long as the result
  stays clearly labeled and visually consistent across all of them —
  don't mix card-wrapped and unwrapped sections in the same stack.
  **Undecided**: whether that replacement is a plain labeled section, a
  collapsible/expandable section, or something else. Settle this with a
  concrete mockup before implementing rather than guessing mid-diff.

## 2. Layout conventions — Current (shipped)

- Single vertical stack (`cards_col`) of equal-weight cards, all the same
  width, `hexpand`/`halign: Fill`, minimum 260px. Header card is an ordinary
  member of the stack, not a special full-width case.
- Uniform inter-card gap: **12px** (`cards_col` spacing), same value used
  everywhere — don't introduce a second spacing constant.
- Within a card: 8px vertical gap between key/value rows.
- Key/value rows: fixed-width (12 chars) dim label on the left, value
  right of it, `xalign: 0.0`.
- Long lists (Dependencies, Reverse Dependencies, Provides body, Files) sit
  in an internal `ScrolledWindow` with a capped max-content-height, so one
  long list can't push every card below it off-screen.
- The whole card stack scrolls as one unit inside the pane; individual
  cards don't scroll independently except where noted above.

## 2a. Layout conventions — Target (redesign)

Confirmed 2026-08-11: this deliberately breaks from the pure single-column
stack above.

- **Top row**: Name card and Source card side by side (two columns),
  per `roughmockup.png`.
- **Below the top row**: every other card (Size & Installation,
  Dependencies, Reverse Dependencies, Provides, Requires, Exports,
  Conflicts, Replaces, Files — the last five now separate cards per
  §1a) stacks single-column, one card per row, same as current behavior.
  Confirmed: no other card pairs up side by side, only Name+Source.
- Spacing/sizing constants (12px gap, 8px row gap, 260px min width,
  12-char key label) and the "empty card omitted" rule carry over
  unchanged unless a specific card's rework says otherwise.

## 3. Visual language (CSS classes — see `window.rs::install_css`)

- `.card` — 1px border, 10px radius, `10px 14px 14px` padding, faint
  tinted background (`alpha(@theme_fg_color, 0.035)`)
- `.card-header` — bold, 0.78em, uppercase, letter-spacing 0.06em, accent
  color — the micro-header pattern for every card title
- `.chip` — pill shape, used for state chips, tag chips, count pills;
  color variants `.chip-ok` / `.chip-warn` / `.chip-err`
- `.dim-label` — muted secondary text (keys, descriptions, placeholders)
- `.icon-btn` — 32x32 minimum, subtle tinted background, used for every
  icon-only secondary action
- `.state-dot` — 8px corner dot on a toggle icon button, `.on` (filled,
  accent color) vs `.off` (hollow) — this is how toggle state (Hold,
  Repo-Lock, Mark Manual/Auto) is shown, not a second button
- `.plain-tag` / `.plain-tag-conflict` — monospace chips for
  Provides/Requires/Conflicts/Replaces
- `.actions-secondary` — top border divider above the icon-button strip

Any redesign that wants to change one of these should change the CSS rule
in `install_css`, not introduce a parallel one-off style in `detail_pane.rs`.

## 4. Interaction patterns to preserve

- Dependency/reverse-dependency rows: click jumps to that package in the
  main list. **Under revision (2026-08-11)**: hover-triggered custom-tooltip
  is being replaced by a click-triggered info popup — see §4a, this
  reopens the ~2026-07-26 rejected-iterations entry deliberately.
- Toggle actions (Hold, Repo-Lock, Mark Manual/Auto) apply immediately,
  independent of the queued Install/Remove/Upgrade mark system.
- Files are fetched only when the section is expanded, never eagerly on
  selection — large packages can have thousands of files.
- A stale-async-reply guard (compare `current_pkgname` before applying)
  wraps every async callback that can outlive the selection that triggered
  it.

## 4a. New interaction: clickable package-name popup (target)

Confirmed 2026-08-11, scope is **Dependency/Reverse-Dependency rows only**
(not the main package list).

- Package names in Dependencies/Reverse Dependencies become click-triggered
  (not hover-triggered) with an info popup showing the mini package
  summary that today appears in the hover tooltip.
- This deliberately revisits the ~2026-07-26 rejected `Popover` direction.
  The original bug: `Popover`'s `autohide` grabs the pointer, which could
  eat the click meant for `row-activated` (the jump-to-package action) and
  could leave a dangling grab on list rebuild, freezing the app. **Whatever
  mechanism implements this must solve that specific failure mode** — e.g.
  scope the popup so it doesn't compete with row-activation, or don't use
  GTK's `Popover` autohide grab at all. Don't reintroduce the old bug by
  reflex-reaching for `gtk::Popover`.

## 4b. Detail-pane orientation toggle — Current (shipped 2026-08-11)

Went through two rounds of correction before landing — recorded in full so
the next redesign pass doesn't re-derive it from scratch.

- **What it does**: `right_paned` (the `pkg_list`/`detail_pane` split)
  flips orientation. Default (`vertical_panel = false`): `Vertical`
  orientation, detail pane docked **below** the list, full width — cards
  render in `cards_flow` (`gtk::FlowBox`, `HORIZONTAL_CARDS_PER_LINE = 4`,
  wraps toward 2 rows). Toggled on: `Horizontal` orientation, detail pane
  docked to the **right** as a narrow column — cards render in `cards_col`
  (single-column stack, the original layout). `DetailPane::set_horizontal`
  reparents the same card widgets between the two containers rather than
  rebuilding them; toggling is driven by `apply_panel_orientation` in
  `window.rs`, which flips `right_paned`'s orientation and calls
  `set_horizontal` together so dock position and card layout never drift
  out of sync.
- **Where the control lives**: exposed as a "Vertical Panel" switch row in
  the Settings/View menu (not a header button — an earlier header-button
  version was replaced here). A *separate* concern, the detail pane's
  show/hide **visibility**, has its own header-bar button
  (`btn_toggle_detail_pane`, icon `sidebar-show-right-symbolic`) packed at
  the outer right edge of the header, past the search bar — the
  right-side mirror of `btn_toggle_sidebar` on the left. Don't conflate
  the two: one toggles dock orientation (menu only), the other toggles
  whether the panel is shown at all (header button + menu switch, same
  bidirectional-binding pattern as the sidebar's button/switch pair).
- **The bug that drove two follow-up fixes**: `GtkPaned.position` means
  "size of the start child along the *current* axis" — reusing a height
  saved from `Vertical` mode as an x-position in `Horizontal` mode
  squeezed the package list down to whatever arbitrary pixel value was
  last saved, effectively hiding it. Fixed by giving right-dock mode its
  own position formula (`apply_panel_orientation`: pane width minus a
  fixed `VERTICAL_PANEL_DETAIL_WIDTH = 380`, so the list always keeps the
  majority of the space) and by tracking the bottom-dock height
  separately (`WindowState.default_detail_pos`) so switching back doesn't
  corrupt it either.
- **Icon-resolution gotcha**: `sidebar-show-right-symbolic` exists in
  Adwaita on disk but rendered as a broken/red icon here — GTK's icon
  lookup on this (non-GNOME) desktop only searches the *active* theme
  plus `hicolor`, never Adwaita as an implicit second fallback (same
  constraint `USED_SYMBOLIC_ICONS`/`ensure_icon_theme_fallback` already
  exists to work around, see `window.rs`). Fixed by bundling a mirrored
  copy at `data/icons/hicolor/scalable/actions/sidebar-show-right-symbolic.svg`
  and adding the name to `USED_SYMBOLIC_ICONS`. Any future custom-icon
  reference needs the same treatment — checking that the file merely
  exists somewhere in `/usr/share/icons` is not sufficient proof it will
  render.
- **FlowBox visibility gotcha** (see also
  [[project-caerus-gtk4-layout-gotchas]]): `gtk::FlowBox` wraps every
  inserted child in an implicit `FlowBoxChild`; hiding the card itself
  doesn't hide that wrapper, which otherwise leaves a gap. `detail_pane.rs`'s
  `set_card_visible` now also hides `card.parent()` when it downcasts to
  `FlowBoxChild`, so this is handled regardless of which container
  (`cards_col` or `cards_flow`) currently holds the card.

## 5. Rejected iterations log

Append an entry here **every time** a proposed direction gets vetoed —
include what was tried and the concrete reason, not just "didn't like it".
This is the part that actually prevents re-litigating the same idea.

| Date | What was tried | Why it was rejected |
|---|---|---|
| ~2026-07-20 | "More" popover menu bundling secondary actions | Replaced with always-visible segmented icon-button clusters — commit `2090a95` treats this as settled; menu diving was worse than a slightly busier header |
| ~2026-07-26 | `Popover`-based hover card for dependency rows | Popover's `autohide` grabs the pointer, which could eat the click meant for `row-activated` and could leave a dangling grab on list rebuild, freezing the app. Switched to GTK's built-in custom-tooltip mechanism instead. |
| ~2026-07-23 | Source info beside Size/Installation in a shared column | Moved Source to stack below instead — alignment/readability issue, not recorded in detail beyond the commit message |
| *(fill in from memory)* | *(the panel's been reworked 6+ times — sessions before this doc existed didn't record the "why", only the final accepted state. If you remember specific rejected directions from earlier sessions, add them here before we start the next redesign attempt.)* | |

## 6. Open questions — resolved 2026-08-11

- **Motivation**: fresh aesthetic direction, not a fix for a specific
  complaint about the current layout.
- **Structure**: challenging the pure "single stack" — top row pairs
  Name + Source side by side (§2a); everything else remains a
  single-column stack. This is a deliberate, explicit exception, not a
  slide back toward a general grid.
- **Still open**: tags placement (§1a) — pending an audit of how common
  tag data actually is across packages before deciding where/whether to
  show them.
