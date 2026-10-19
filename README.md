<img src="caerus/data/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg" width="96" height="96" align="center">

# Caerus

A GTK4 front end for [Void Linux's](https://voidlinux.org/) XBPS.

<img src="design/mockups/compact/implemented/compact-inventory.png" width="100%">

More views: [Updates](screenshots/updates.png), [Discover](design/mockups/compact/implemented/discover-sections.png),
[package details in a narrow window](design/mockups/compact/implemented/compact-narrow-details.png).

## Features

- Packages, Updates, Discover and Maintenance views, with searchable history
- Search by relevance, sort and bulk-mark packages
- Filter by state, repository or custom exclude patterns
- Install, upgrade, remove, purge, hold and repo-lock
- Full system upgrade, orphan removal, cache cleanup, kernel purge
- Repository and alternatives management
- Pending-change queue with individual unmarking and dependency/version previews
- Transaction stages, package outcomes, copyable logs and detailed history
- Searchable Discover catalog: 174 applications across 13 sections, limited to
  applications available from enabled repositories, in wrapping columns
  (Discover can be hidden)
- Compact package table with descriptions, versions and state, a desktop filter/tool sidebar,
  optional size columns, and a smaller inspector with collapsible sections
- Adaptive task navigation and a separate package-details page in narrow windows

Caerus loads cached repository metadata automatically at launch. Existing
package rows remain visible during a local reload. If sync at launch is enabled,
local data loads before authentication or network access begins.

Refresh repositories before reviewing a system upgrade. Applying an upgrade
uses the repository metadata you reviewed; it does not refresh again during
execution. Incomplete or failed previews block Apply. Mixed operation types
are reviewed in separate steps, preserving remaining marks for the next review.
Failed changes reload package state before a new review; Caerus does not offer
an automatic force retry.

## Package backend builds

The default build uses XBPS and retains the Void Linux behavior:

```sh
cargo build --release
```

For Chimera Linux, build without the XBPS FFI dependency and select APK:

```sh
cargo build --release -p caerus --no-default-features --features apk
cargo build --release -p caerus-helper
```

The helper detects Chimera through `/etc/os-release` and routes supported
package actions to APK. APK builds include package search and details, the
configured repository list, previews, install/remove, repository updates, and
system upgrades. Void-only operations such as recursive purge, hold, repo-lock,
kernel purge, alternatives, orphan removal, cache cleanup, and reconfiguration
are rejected by the helper.

APK treats `/etc/apk/world` as a set of package constraints. Install and remove
actions add or remove a world constraint, then APK solves the full dependency
graph. Removing a package therefore does not guarantee its files are removed if
another world constraint still requires it. APK's solver preview includes
dependency changes and `install_if` packages. Package details distinguish
explicit world roots from dependencies when the world file is readable. Marked upgrades use `apk upgrade` so they do not add
the selected package to the world file; new installs use `apk add`.

## Development checks

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, and `bash scripts/audit.sh`. Also build `--features adwaita` and check
APK with `cargo clippy -p caerus --no-default-features --features apk --all-targets -- -D warnings`.

Set `CAERUS_TRACE_STARTUP=1` to log startup milestones. For performance checks,
use `cargo build --release`; an unoptimized debug build is not representative.
See [the startup audit](docs/startup-audit-2026-10-19.md).

The display smoke test covers task pages, discovery availability, package
search and a simulated partial transaction with history. Run it on a test
display with `cargo test -p caerus -- --ignored --test-threads=1`. Its helper
fixture runs unprivileged and never modifies host packages. Set
`CAERUS_TEST_LOCAL_STARTUP=1` for the additional check that loads the host’s
cached inventory without a repository refresh and compares it to a local reload.
This opt-in test requires existing repository metadata; it never applies packages.

The helper emits versioned `EVENT 1` messages for command boundaries, observed
package stages and completion, alongside the existing `LOG`, `OK` and `ERROR`
responses. Stage observations are distinct from successful completion. The UI
also accepts older helpers with textual output. History retains old records
and stores new records with escaped multiline logs and version changes.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
