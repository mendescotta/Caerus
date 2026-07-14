# Changelog

All notable changes to Caerus are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); dates are release dates,
not commit dates.

## [0.3.1] - 2026-07-14

### Fixed
- The Apply/maintenance progress bar's embedded percentage text was
  actually rendering as a separate label *above* the bar rather than
  overlaid inside it — `GtkProgressBar`'s own `show-text`/`text`
  property lays that label out as a sibling of the trough, not on top
  of it, contrary to the previous fix's assumption. It's now a real
  `GtkOverlay` with a centered label stacked on top of the bar.
- After a batch finished, the overlay text could be left showing a
  stale percentage (e.g. "21%") even though the bar itself was full and
  the dialog said "Finished successfully." — the last percentage tick
  seen is rarely actually 100 (a package's final log lines often carry
  no percentage at all). It's now cleared/updated to just the package
  count on finish, not a stale in-flight percentage.

## [0.3.0] - 2026-07-14

### Added
- **Reconfigure All Packages** (`xbps-reconfigure -fa`) in the app menu's
  Maintenance section — force-reruns every installed package's post-install
  configuration script, for recovering from an interrupted transaction or a
  libc/shared-library upgrade that left packages unconfigured.
- **"Sync Repositories at Launch" toggle** in the app menu. Previously,
  Caerus always synced repositories (a privileged, `pkexec`-authenticated
  action) immediately on startup with no way to opt out and no explanation
  of why a password prompt had just appeared. Disabling it skips that
  prompt entirely at launch; manual sync via the header bar button still
  works as before. When it does run, the status bar now says so explicitly
  ("Requesting authentication to sync repositories…") instead of a generic
  "Syncing repositories…".
- Reinstall, Reconfigure, Download Only, Repo-Lock/Release Repo-Lock, and
  Mark as Manually/Automatically Installed actions (detail pane "More"
  menu), plus force-retry after a failed Apply and fuller package
  metadata.
- `CONTRIBUTING.md` covering build steps, the pre-PR check list, and the
  project's approach to testing.
- Unit test coverage for the previously-untested parts of the codebase
  most likely to silently produce the wrong real-world command: mark →
  xbps-argv mapping in `caerus-helper` (`PURGE` → `-R`, force variants,
  hold/unhold, repolock/repounlock, mark auto/manual), the force-retry
  verb mapping in the GUI, and the apply-progress-dialog line parsing
  described below.

### Fixed
- **The "Package N of M" counter in the Apply/maintenance progress dialog
  could get stuck** — it counted every *distinct raw log line* as a new
  package, but `xbps-install` emits several differently-worded lines per
  single package (`unpacking`, `configuring`, `installed successfully`,
  etc). That overcounted by 2-4x and made the counter hit its cap — and
  visibly stall there — long before a batch was actually finished. It now
  counts transitions between distinct package identifiers instead, parsed
  from the real `xbps-install` output format (confirmed against the
  literal format strings in the `xbps-install` binary itself, which turned
  out not to match an incorrect assumption in an earlier comment).
- About dialog's website link pointed at Void Linux instead of the Caerus
  repository.
- Stale checkbox/status-icon/marked-styling after marking a package
  through something other than its checkbox.
- Icon fallback, dialog focus, and search-result count issues from the
  full project audit below.

### Changed
- The Apply/maintenance progress bar is now visibly thicker and shows its
  own status text ("Package N of M", the live percentage, or both)
  overlaid inside the bar itself, replacing a separate label row above it.
- Full project audit: security hardening, correctness fixes, and
  DE-agnostic behavior improvements across the app (see `git log
  b8ce0e2` for the complete list — too broad to enumerate item-by-item
  here without drifting out of sync with the code).
- Now follows the GNOME dark/light mode setting via the Settings portal.
- CI (`.github/workflows/rust.yml`) now actually builds and tests the
  project. It previously ran on stock `ubuntu-latest`, which has neither
  GTK4 nor `libxbps` development headers, so every run failed at the first
  `pkg-config` step regardless of what changed — the CI badge had never
  once been meaningfully green. It now runs inside Void Linux's own
  official container image, matching what the app is actually built
  against, and additionally runs `cargo clippy` (denying warnings) and
  `cargo fmt --check`.

## [0.2.0] and earlier

Caerus started as a from-scratch Rust rewrite of an earlier C/GTK4
project of the same name, then was prepared and published as its own
standalone public project on 2026-07-10 — transaction preview, cascading
removal warnings, orphan/arch filters, transaction history, the shared
dialog-chrome helper (`dialog_util.rs`), an AI-assistance disclaimer, and
the initial `xbps-src` packaging template all predate this changelog's
start. See `git log --oneline` for the full history.
