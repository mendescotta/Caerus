# Contributing to Caerus

Caerus is young (see [DISCLAIMER.md](DISCLAIMER.md) for context on how it
was built) and still finding its contributor process, but the basics
below should cover most changes.

## Building

See the README's [Dependencies](README.md#dependencies) and
[Build and install](README.md#build-and-install) sections for the full
package list and setup. The short version, on Void Linux:

```sh
xbps-install -S gtk4-devel libxbps-devel glib-devel polkit clang pkg-config
cargo build
```

`./target/debug/caerus` (or `--release`) runs straight out of the build
tree — see the README's "Running without installing" section.

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
- The `xbps-*` command Caerus would have run — see the README's
  "Every Caerus action and its underlying xbps command" table — if you
  suspect it ran the wrong one
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
