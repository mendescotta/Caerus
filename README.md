<img src="caerus/data/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg" width="96" height="96" align="center">

# Caerus

A GTK4 front end for [Void Linux's](https://voidlinux.org/) XBPS.

<img src="screenshots/main-window.png" width="100%">

More views: [Updates](screenshots/updates.png), [Discover](screenshots/discover.png),
[package details in a narrow window](screenshots/package-details-narrow.png).

## Features

- Packages, Updates, Discover and Maintenance views, with searchable history
- Search by relevance, sort and bulk-mark packages
- Filter by state, repository or custom exclude patterns
- Install, upgrade, remove, purge, hold and repo-lock
- Full system upgrade, orphan removal, cache cleanup, kernel purge
- Repository and alternatives management
- Pending-change queue with individual unmarking and dependency/version previews
- Transaction stages, package outcomes, copyable logs and detailed history
- Bundled application catalog with local desktop icons (Discover can be hidden)
- Compact navigation and a separate package-details page in narrow windows

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

The display smoke test covers task pages, discovery availability, package
search and a simulated partial transaction with history. Run it on a test
display with `cargo test -p caerus -- --ignored --test-threads=1`. Its helper
fixture runs unprivileged and never modifies host packages.

The helper emits versioned `EVENT 1` messages for command boundaries, observed
package stages and completion, alongside the existing `LOG`, `OK` and `ERROR`
responses. Stage observations are distinct from successful completion. The UI
also accepts older helpers with textual output. History retains old records
and stores new records with escaped multiline logs and version changes.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
