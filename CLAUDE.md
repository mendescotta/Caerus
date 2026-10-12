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
