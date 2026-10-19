<img src="caerus/data/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg" width="96" height="96" align="center">

# Caerus

A GTK4 front end for [Void Linux's](https://voidlinux.org/) XBPS.

<img src="screenshots/main-window.png" width="100%">

## Features

- Search, sort and bulk-mark packages
- Filter by state, repository or custom exclude patterns
- Install, upgrade, remove, purge, hold and repo-lock
- Full system upgrade, orphan removal, cache cleanup, kernel purge
- Repository and alternatives management
- Transaction preview and history

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
dependency changes and `install_if` packages. The package list shows installed
packages, including dependencies; it does not currently distinguish world
roots from dependencies. Marked upgrades use `apk upgrade` so they do not add
the selected package to the world file; new installs use `apk add`.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
