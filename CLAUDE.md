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
- Intermittent segfault after "Remove Anyway" (see `../CONTINUE.md` in the
  workspace). Confirmed unrelated to the ibus-gtk4 launch-crash regression
  below (reported before it existed); still unreproduced. `scripts/debug-run.sh`
  runs the `daily` cargo profile with core dumps and full backtraces enabled,
  so the next occurrence should leave a core under
  `~/.local/share/caerus/debug/`.
- xlint: template uses deprecated `wrksrc`; GPL vlicense hint.

## Fixed
- 2026-10-12: crash at launch. Not a caerus bug — `ibus-gtk4` 1.5.34's GTK4
  immodule recurses infinitely in `g_object_new` under gtk4 4.24.x, aborting
  every GTK4 app on launch. Fixed by pinning `ibus` to 1.5.32_4 in the overlay
  (`voidlab/srcpkgs/ibus`, voidlab-testing commit `171d6e9`).
