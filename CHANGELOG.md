# Changelog

All notable changes to Caerus are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); dates are release dates,
not commit dates.

## [0.4.1] - 2026-07-14

Fresh-perspective audit pass: five correctness fixes, a responsiveness
rework, and a UI/UX consistency sweep.

### Fixed
- **Held essential packages keep their removal guard.** The pkgdb scan
  returned early for on-hold packages before reading the `essential`
  flag (and the recorded architecture), so a package that was both held
  and essential could be marked for removal from the UI.
- **Held dependencies are no longer treated as missing.** The
  install-time dependency check only counted `Installed`/`Upgradable`
  as present, so an on-hold dependency was listed as "additional
  package required" and marked for Install — and the resulting
  `xbps-install <pkg>` would have upgraded it, silently bypassing the
  hold (which only shields against `-Su`).
- **Version columns sort like xbps.** "Installed"/"Available" sorted
  lexicographically ("1.10" before "1.9"); they now use libxbps's own
  `xbps_cmpver`.
- **Launching Caerus while it's already running presents the existing
  window** instead of opening a duplicate window (with its own second
  worker thread and pkexec session) in the same process.
- **Full System Upgrade handles xbps updating itself.** When `xbps`
  itself has a pending update, `xbps-install -Su` updates only xbps and
  exits expecting a re-run; the helper now performs that re-run
  automatically instead of reporting a baffling failure.
- The adwaita build no longer sets `gtk-application-prefer-dark-theme`
  (unsupported under libadwaita, warned at runtime); it follows the
  system dark/light preference via `AdwStyleManager` instead.
- The repositories dialog now honors xbps.d(5) override semantics: a
  file in `/etc/xbps.d` shadows the same-named file in
  `/usr/share/xbps.d`, so repos disabled that way are no longer listed.

### Changed
- **The UI can no longer freeze on a busy backend.** Every xbps worker
  query (detail-pane info/deps/reverse-deps/files, both confirmation
  dialogs' checks, and the Apply/Full-Upgrade dry-run previews) is now
  asynchronous. Previously they blocked the main loop, and one landing
  behind an in-flight reload froze the whole window until the rescan
  finished. Stale replies are discarded when the selection has moved on.
- The status bar shows visible-row counts ("N shown — …") whenever
  anything narrows the list — sidebar preset or repository filter, not
  just search text — so the numbers always describe what's on screen.
- **Remove Orphaned Packages asks first**, listing the packages it
  would remove (menu entry gained the "…" it deserved); Reconfigure All
  Packages also confirms before force-rerunning every install script.
  "Verify Package Database" lost its misleading ellipsis. Menu
  convention now: "…" means a dialog opens before anything runs.
- The detail pane's Repository row uses the same custom repository
  display names as the sidebar (set via right-click → rename).
- Transaction History records local wall-clock time instead of UTC
  (old UTC entries still display as recorded).
- The Delete key now acts on the whole selection: one row keeps the
  usual reverse-dependency confirmation; a multi-row selection applies
  a bulk Remove mark, matching the context menu's bulk action.
- Apply-dialog polish: the progress bar's overlay text always shows a
  known percentage (it previously vanished until the first per-package
  status line), and the Details log is styled for scanning — errors in
  red, per-package completions in green, phase banners bold, protocol
  chatter dimmed.
- The pre-Apply summary's heading counts everything the transaction
  will actually touch (marked packages plus dependencies pulled in by
  the dry-run), matching the list below it.
- Dependencies/Reverse-Dependencies placeholders now distinguish "no
  selection", "loading", and "none" instead of always claiming "Select
  a package".
- "Mark All Upgrades" applies its marks in a single pass instead of one
  full-list scan per package.
- On the plain-GTK4 build, transient status-bar messages self-dismiss
  after a few seconds instead of persisting until the next reload.

## [0.4.0] - 2026-07-14

### Added
- **`get-caerus.sh`**, a one-line install script (`curl -fsSL
  .../get-caerus.sh | sh`): clones the repo, offers to install missing
  build dependencies via `xbps-install`, builds with `cargo build
  --release`, then prompts to run it locally or install system-wide.
  Always builds from source on the user's own machine — no prebuilt
  binary to trust.
- **Optional `adwaita` Cargo feature** (`--features caerus/adwaita`,
  needs `libadwaita-devel`): a build-time choice, not runtime detection,
  that swaps in libadwaita widgets where available: the About window
  uses `AdwAboutWindow`'s proper GNOME-standard chrome instead of plain
  `GtkAboutDialog`, transient notifications (sync failed, changes
  applied, a batch finished, ...) show as an auto-dismissing `AdwToast`
  instead of overwriting the status bar's persistent package count, and
  `adw::init()` runs at startup so the whole UI is consistently
  adwaita-styled from launch. CI builds and lints both configurations so
  this doesn't silently bit-rot.
- **Purge Old Kernels** window (app menu → Maintenance): lists removable
  kernel versions (`vkpurge list`, unprivileged, run straight from the
  GUI like Find Owning Package) in a checkbox table, with Reload, Select
  All, and Purge Selected (`vkpurge rm`, via `caerus-helper`). Not an
  xbps tool — the standalone Void kernel-cleanup script.
- **Settings dialog** (app menu → Settings…): a "Sync Repositories at
  Launch" checkbox (moved here from its own inline menu section) and a
  new "Search by Name Only by Default" checkbox controlling what the
  header's name-only search toggle starts as next launch.
- **Collapsible sidebar** — a header bar toggle button hides/shows the
  filter/repository sidebar for a wider package table.

### Changed
- "Sync Repositories at Launch" now defaults to **off**. A fresh install
  shouldn't greet a first-time user with an unexplained authentication
  prompt before they've seen a single package; the toggle in Settings
  still turns it back on for anyone who wants it.

### Fixed
- `get-caerus.sh`'s dependency check used to grep `xbps-query -l` output
  for a name prefix, which could false-positive on an installed package
  that merely starts with the same prefix (e.g. `clang-analyzer18`
  satisfying a check for plain `clang`). Now uses `xbps-query <pkgname>`'s
  exit code — an exact, unambiguous check.
- `caerus-helper`'s `ADDREPO`/`REMOVEREPO` handlers now reject control
  characters themselves, instead of relying entirely on the GUI having
  already sanitized the URL — defense-in-depth for the one privileged
  component in the project.

### Documentation
- Noted that only Void's glibc variant is built/tested/covered by CI —
  musl is untested, not confirmed working.

## [0.3.1] - 2026-07-14

### Fixed
- The Apply/maintenance progress bar's percentage text rendered *above*
  the bar instead of on top of it — `GtkProgressBar`'s own `show-text`
  lays that label out as a sibling of the trough, not overlaid, contrary
  to the previous fix's assumption. Now a real `GtkOverlay` with a
  centered label stacked on the bar.
- That text could also freeze on a stale percentage (e.g. "21%") after a
  batch finished, since the last tick seen is rarely actually 100. Now
  cleared to just the package count on finish.

## [0.3.0] - 2026-07-14

### Added
- **Reconfigure All Packages** (`xbps-reconfigure -fa`) in the app menu —
  force-reruns every installed package's post-install configuration
  script, for recovering from an interrupted transaction or a
  libc/shared-library upgrade that left packages unconfigured.
- **"Sync Repositories at Launch" toggle**. Previously Caerus always
  synced repositories (a `pkexec`-authenticated action) on startup with
  no way to opt out and no explanation for the sudden password prompt.
  Disabling it skips that prompt at launch; manual sync still works as
  before, and the status bar now names the prompt when it does fire.
- Reinstall, Reconfigure, Download Only, Repo-Lock/Release Repo-Lock, and
  Mark as Manually/Automatically Installed actions (detail pane "More"
  menu), plus force-retry after a failed Apply and fuller package
  metadata.
- `CONTRIBUTING.md`: build steps, pre-PR checklist, testing approach.
- Unit tests for the parts of the codebase most likely to silently
  produce the wrong real-world command: mark → xbps-argv mapping in
  `caerus-helper` (`PURGE` → `-R`, force variants, hold/unhold,
  repolock/repounlock, mark auto/manual), the GUI's force-retry mapping,
  and the progress-dialog line parsing fixed below.

### Fixed
- **The "Package N of M" counter in the Apply/maintenance progress
  dialog could get stuck.** It counted every *distinct raw log line* as
  a new package, but `xbps-install` emits several differently-worded
  lines per package (`unpacking`, `configuring`, `installed
  successfully`), overcounting 2-4x and hitting its cap long before a
  batch actually finished. Now counts transitions between real package
  identifiers, parsed from `xbps-install`'s actual output format.
- About dialog's website link pointed at Void Linux instead of Caerus.
- Stale checkbox/status-icon/marked-styling after marking a package
  through something other than its checkbox.
- Icon fallback, dialog focus, and search-result count issues from the
  full project audit below.

### Changed
- The Apply/maintenance progress bar is now visibly thicker and shows
  its own status text ("Package N of M", the live percentage, or both)
  instead of a separate label row above it.
- Full project audit: security hardening, correctness fixes, and
  DE-agnostic behavior improvements across the app (see `git log
  b8ce0e2` for the complete list).
- Now follows the GNOME dark/light mode setting via the Settings portal.
- CI now actually builds and tests the project. It previously ran on
  stock `ubuntu-latest`, which has neither GTK4 nor `libxbps` headers, so
  every run failed at the first `pkg-config` step regardless of what
  changed — the badge had never once been meaningfully green. It now
  runs inside Void Linux's own container image and additionally runs
  `cargo clippy` (denying warnings) and `cargo fmt --check`.

## [0.2.0] and earlier

Caerus started as a from-scratch Rust rewrite of an earlier C/GTK4
project of the same name, then was prepared and published as its own
standalone public project on 2026-07-10 — transaction preview, cascading
removal warnings, orphan/arch filters, transaction history, the shared
dialog-chrome helper (`dialog_util.rs`), an AI-assistance disclaimer, and
the initial `xbps-src` packaging template all predate this changelog's
start. See `git log --oneline` for the full history.
