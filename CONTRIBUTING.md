# Contributing to Caerus

Caerus is young (see [DISCLAIMER.md](DISCLAIMER.md) for context on how it
was built) and still finding its contributor process, but the basics
below should cover most changes.

## Building

On Void Linux (glibc; musl is untested):

```sh
xbps-install -S cargo gtk4-devel libxbps-devel glib-devel polkit clang pkg-config
cargo build --release
```

Runtime needs `gtk4`, `libxbps`, `glib` and `polkit` with an authentication
agent running. Add `--features caerus/adwaita` (needs `libadwaita-devel`) for
libadwaita widgets.

`./target/release/caerus` (or `debug`) runs straight out of the build tree.
To install:

```sh
sudo ./install.sh                      # /usr/bin, /usr/libexec, desktop file, polkit policy
./install.sh --user                    # register this checkout under ~/.local/share, no root
sudo ./install.sh --uninstall          # or --user --uninstall
```

Or the quick installer, which builds from source:
`curl -fsSL https://raw.githubusercontent.com/mendescotta/Caerus/main/get-caerus.sh | sh`.

## Before opening a PR

Run these from the repo root; CI runs the same checks and will fail the
build otherwise:

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

`cargo fmt` (no `--check`) will fix formatting for you. The workspace
enables `clippy::all` as a warning (see `[workspace.lints.clippy]` in the
root `Cargo.toml`) — CI additionally denies warnings outright, so treat any
clippy output as something to fix, not skip.

Caerus also has an optional `adwaita` Cargo feature (`--features
caerus/adwaita`, needs `libadwaita-devel`) that swaps in libadwaita
widgets where available — build and clippy both configurations if you
touch anything gated behind `#[cfg(feature = "adwaita")]`, since CI does.

## Code hardening

Keep active work on `main` and patch small code paths in isolated commits.
`scripts/audit.sh` (also run by the audit workflow) reports risky patterns
such as `unwrap()` on GTK downcasts; review its output rather than
rewriting code automatically.

## Testing changes

Most of the app's *logic* — mark-to-command mapping, progress-line
parsing, force-retry mapping — lives in small, pure functions specifically
so it's unit-testable without a live GTK window or a privileged helper
process; see the `#[cfg(test)] mod tests` blocks in `caerus/src/ui/
apply_dialog.rs`, `caerus/src/ui/window.rs`, and `caerus-helper/src/
main.rs` for the existing pattern and add to it if you touch that kind of
code. If you're changing something in `caerus-helper` in particular —
the one privileged component, run via `pkexec` — prefer expressing the
change as a pure, testable mapping (verb/mark → xbps argv) over inline
logic in the stdin-reading loop, the same way `argv_for`/`run_pkg_command`
already do.

*UI/interaction* changes (dialogs, layout, keyboard shortcuts) generally
aren't practical to unit test — build the app and click through the
change yourself. Screenshot tooling for automated visual verification
isn't set up in this repo; a manual pass is the current expectation.

## Reporting bugs / proposing features

Open a GitHub issue. Since Caerus talks to `libxbps` and shells out to
`xbps-*` tools directly, a useful bug report usually includes:

- What you did (exact steps) and what you expected vs. what happened
- The `xbps-*` command Caerus would have run — see
  [Actions and their xbps commands](#actions-and-their-xbps-commands) — if
  you suspect it ran the wrong one
- Whether the failure came from the GUI itself or from `caerus-helper`
  (visible in the Apply/maintenance dialog's "Details" expander, which
  shows the underlying command's raw output)

## Scope notes

- The privilege boundary (unprivileged GUI, `caerus-helper` as the only
  thing ever run via `pkexec`) is a hard architectural line — don't add
  code paths that let the GUI touch `libxbps` write operations or shell
  out to `xbps-*` directly for anything privileged.
- Exactly one dedicated OS thread ever touches the `xbps_handle` — see the
  comment at the top of `caerus/src/backend/package_store.rs`. This was a
  deliberate fix for a crash class in an earlier version of the project;
  don't reintroduce a second thread touching `libxbps`.

## Actions and their xbps commands

| Caerus Action | Where in UI | Underlying xbps command |
|---|---|---|
| Sync repositories | Header sync button / at launch | `xbps-install -S` |
| Full System Upgrade | App menu | `xbps-install -y -Su` |
| Install / Upgrade (Apply) | Checkbox, context menu, detail pane, Apply | `xbps-install -y -- pkg...` |
| Remove | Checkbox, context menu, detail pane, Apply | `xbps-remove -y -- pkg...` |
| Purge | Checkbox, context menu, detail pane, Apply | `xbps-remove -y -R -- pkg...` |
| Install (force retry) | "Retry With Force" after a failed Apply | `xbps-install -y -I -- pkg...` |
| Remove (force retry) | "Retry With Force" after a failed Apply | `xbps-remove -y -F -- pkg...` |
| Purge (force retry) | "Retry With Force" after a failed Apply | `xbps-remove -y -R -F -- pkg...` |
| Reinstall | Detail pane | `xbps-install -f -y -- pkg...` |
| Reconfigure | Detail pane | `xbps-reconfigure -f -- pkg...` |
| Download Only | Detail pane | `xbps-install -D -y -- pkg...` |
| Hold | Detail pane | `xbps-pkgdb -m hold -- pkg...` |
| Release Hold | Detail pane | `xbps-pkgdb -m unhold -- pkg...` |
| Repo-Lock | Detail pane | `xbps-pkgdb -m repolock -- pkg...` |
| Release Repo-Lock | Detail pane | `xbps-pkgdb -m repounlock -- pkg...` |
| Mark as Automatically Installed | Detail pane | `xbps-pkgdb -m auto -- pkg...` |
| Mark as Manually Installed | Detail pane | `xbps-pkgdb -m manual -- pkg...` |
| Remove Orphaned Packages | App menu | `xbps-remove -y -o` |
| Clean Package Cache | App menu | `xbps-remove -O` |
| Verify Package Database | App menu | `xbps-pkgdb -a --checks files,dependencies,alternatives,pkgdb` |
| Reconfigure All Packages | App menu | `xbps-reconfigure -fa` |
| List removable kernels | Purge Old Kernels window | `vkpurge list` (not xbps — runs unprivileged, straight from the GUI) |
| Purge Old Kernels | Purge Old Kernels window | `vkpurge rm <version...>` (not xbps — the one part of this row that's privileged) |
| Switch Alternative | Alternatives dialog | `xbps-alternatives -g <group> -s <pkg>` |
| Add Repository | Repositories dialog | writes `/etc/xbps.d/90-caerus.conf` (no xbps CLI), then queues `xbps-install -S` |
| Remove Repository | Repositories dialog | edits the same conf file, then `xbps-install -S` |
| Transaction preview / dry-run | Apply confirmation dialog | `xbps_transaction_prepare()` via libxbps directly — equivalent to `xbps-install -n` |
| Find Owning Package | App menu → Find Owning Package | `xbps-query -o <path>` (the only literal `xbps-query` subprocess call in the app) |
| Package details, deps, reverse-deps, files, provides/conflicts/replaces, shlib info | Detail pane | via libxbps directly (`xbps_pkgdb_get_pkg`/`xbps_rpool_get_pkg` + dictionary reads) — equivalent to `xbps-query -S/-x/-X/-f` |
