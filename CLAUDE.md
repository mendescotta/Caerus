@AGENTS.md

# caerus

GTK4 package manager GUI for Void (workspace: `xbps-sys`, `caerus`,
`caerus-helper`). Flagship project. Public repo `mendescotta/Caerus`.
Design rationale and gotchas: @DEVNOTES.md (local, gitignored).

## Check before claiming done
- `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- Optional `adwaita` feature: also build with `--features adwaita`.
- `scripts/audit.sh` (ripgrep audit of risky patterns; there is no autofix).

## Rules
- The GUI never runs as root. Only `caerus-helper` is elevated (pkexec); it
  stays GTK-free with zero external crates. Protocol: DEVNOTES.
- Only `backend::package_store` calls `xbps-sys`; one worker thread owns the
  `xbps_handle`. Do not call libxbps from anywhere else.
- Sources live in `caerus/caerus/src/...` (workspace member), not `caerus/src`.
- Version compare with `xbps_cmpver`, never string order.
- Release: bump Cargo version + CHANGELOG, tag, then bump the template in
  the overlay (`voidlab/srcpkgs/caerus`, `./voidlab update caerus`). The overlay
  template is the only copy; there is none in this repo.
- New design/plan docs: `docs/superpowers/{specs,plans}/` (gitignored, local).

## Open
- xlint: template uses deprecated `wrksrc`; GPL vlicense hint.

## Fixed
- 2026-10-13: apply-dialog crash during transactions (install/remove/sync).
  The log `GtkTextView` in `apply_dialog.rs` moves the buffer's `insert`
  mark on every appended line to keep the view scrolled to the newest
  text. GTK forwards that mark-set unconditionally to the view's
  input-method context, regardless of focus. The first log line of a
  freshly-presented dialog can arrive before the view is actually rooted
  (confirmed: `text_view.root()`/`.native()` both `None` on the crashing
  call) — ibus-gtk4's immodule (`client/gtk3/ibusimcontext.c`,
  `_set_cursor_location_internal()`) doesn't guard against that: it calls
  `gtk_widget_get_root()`/`get_native()`, gets nothing, and then
  `gdk_x11_surface_get_xid()` on a NULL `GdkSurface*` (confirmed via core
  dump: `rdi=0` at the fault). Reproduced identically on the commit before
  the "Remove Anyway" fix below, so it's unrelated to that change — a
  pre-existing latent bug, just not hit until now. Fix: skip the mark-move
  and `scroll_mark_onscreen` while `text_view.root()` is `None` — nothing
  is on screen to scroll to yet anyway, so this changes no visible
  behavior. Same upstream bug family as the ColumnView issue below
  (missing NULL/validity guards around GTK4's native-surface APIs).
- 2026-10-13: "Remove Anyway" segfault. Root cause confirmed with a local
  debug build of gtk4 4.24.1 (symbols; stock Void gtk4 ships stripped) and
  two real core dumps with byte-identical crash offsets. The checkbox that
  triggers removal lives inside a `GtkColumnViewRowWidget`; `set_mark()`
  replaces that package's list item via `GListStore::splice()` (the
  documented "replace, don't mutate" rebind pattern — see DEVNOTES), which
  unparents/recycles the row widget that still has GTK keyboard focus. If
  the window regains X11 focus right as the confirm dialog closes (it
  always does) before GTK's deferred stale-focus cleanup runs, GTK tries to
  restore focus to the now-parentless row and crashes in
  `gtk_column_view_row_widget_get_column_view()` — upstream gtk4 4.24.1,
  `gtk/gtkcolumnviewrowwidget.c:45`, forces a second
  `_gtk_widget_get_parent()` call with no NULL guard. Not a caerus logic
  bug, but caerus controls the race window: `remove_confirm.rs`'s
  "Remove Anyway" handler now clears the parent window's focus
  (`GtkWindowExt::set_focus(None)`) before `dlg.destroy()`, so there's
  nothing stale left for GTK to restore. Verified against the exact repro
  (search → toggle a row's checkbox → Remove Anyway) and ~30 other
  mark/unmark cycles with zero crashes after the fix, versus 2/2
  reproductions before it. Worth reporting upstream (same bug family as
  gtk4-rs#1673 / GTK MR!7095, MR!7253 — recycled list-view row widgets
  racing with GTK's focus/selection bookkeeping).
- 2026-10-12: crash at launch. Not a caerus bug — `ibus-gtk4` 1.5.34's GTK4
  immodule recurses infinitely in `g_object_new` under gtk4 4.24.x, aborting
  every GTK4 app on launch. Fixed by pinning `ibus` to 1.5.32_4 in the overlay
  (`voidlab/srcpkgs/ibus`, voidlab-testing commit `171d6e9`).
