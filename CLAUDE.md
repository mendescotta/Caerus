@AGENTS.md

# caerus

GTK4 package manager GUI for Void (workspace: `xbps-sys`, `caerus`,
`caerus-helper`). Flagship project. Public repo `mendescotta/Caerus`.
Design rationale and gotchas: @DEVNOTES.md (local, gitignored). Resolved
bug writeups: `docs/fixed-bugs.md`.

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
- `ibus` stays pinned to 1.5.32_4 in the overlay until upstream 1.5.35
  reaches stable (fixes a GTK4-immodule segfault family — `docs/fixed-bugs.md`).

## Open
- xlint: template uses deprecated `wrksrc`; GPL vlicense hint.

## Fixed (root-cause detail: `docs/fixed-bugs.md`)
- 2026-10-13: apply-dialog crash during install/remove/sync transactions.
- 2026-10-13: "Remove Anyway" segfault (GTK4 ColumnView focus-recycle race).
- 2026-10-12: crash at launch (ibus-gtk4 immodule recursion).
