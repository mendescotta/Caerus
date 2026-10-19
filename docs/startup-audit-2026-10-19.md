# Caerus startup audit — 2026-10-19

The B layout did not change the XBPS repository reader or intentionally require
a manual reload. It kept the asynchronous local load at launch. The extracted
`build_window_state` function receives `load_packages=true` in production; only
the GUI fixture passes false. The optional repository sync preference was also
retained. The app opened for the user was `target/debug/caerus`, an unoptimized
development build with the Adwaita feature. That was inappropriate for judging
interactive performance against an installed optimized build.

The larger row widgets, extra page margins and permanent details panel were
layout changes, and substantially reduced the number of visible packages.
D · Compact Focus in `design/mockups/compact/` was subsequently selected and
implemented: single-line rows, header navigation and a smaller inspector. The
measurements below describe the loading fixes before that layout change.

## Actual loading behavior

The instrumented pre-fix B build automatically loaded **20,724** packages:
20,719 distinct repository names plus installed-only packages. All 20,724 were
visible without a repository refresh or manual reload. The revised build did
the same. An opt-in GUI check then performed a local reload and confirmed that
it returned the same inventory count. The reported need for a manual reload
was **not reproduced** here.

No privileged helper operations, package changes or repository synchronizations
were run for these measurements. An existing cached repository inventory is
required. This is a read-only check of this host, not a claim about all systems.

## Measurements

Fresh process launches on the same host, libadwaita enabled, Cairo renderer,
isolated Xephyr X11 display and 1280×900 test window. Three runs per build;
values are process spawn to the first completed package-list frame, in ms.
Filesystem caches were already warm. No system caches were dropped. These
are process startup measurements, **not disk-cold boot measurements** or
measurements of an unidentified historical installed Caerus version.

| Build | Runs (ms) | Median (ms) |
| --- | --- | ---: |
| before-debug | 1755.2, 1789.9, 1794.5 | 1789.9 |
| before-release | 1283.9, 1286.6, 1295.5 | 1286.6 |
| final-debug | 1836.6, 1876.7, 1694.9 | 1836.6 |
| final-release | 1192.5, 1210.6, 1224.7 | 1210.6 |

The optimized current build took approximately 1.21 s,
compared with 1.29 s for the optimized pre-fix B snapshot
(about 6% faster). Debug timings
remain variable and were not materially improved in these samples. There is
no evidence for a dramatic cold-start speedup; the main practical correction
is to use an optimized build when assessing the UI.

[Raw traces and timing samples](startup-measurements-2026-10-19.json) retain all
four groups. The first unprimed debug launch took about 2.8 s; it is excluded
from the comparison because it had different cache conditions.

In representative optimized runs, secondary-page preparation fell from about
46 ms to 7 ms despite the larger catalog. Cached XBPS repository and installed
metadata reads still occupy most of the total startup time. The original
reader, version comparison, repository priority and 30 ms result polling
remain in place.

## Changes made after the audit

- Existing package rows remain visible while reloading and after a failed
  reload, with loading/error context in the results summary. Initial loading
  and genuinely empty/error states still have a dedicated message.
- XBPS now replaces the old model in a single `ListStore::splice`, preserving
  valid queued marks. It no longer removes all rows before constructing their
  replacements. The APK backend already used an atomic replacement.
- Startup publishes local cached data first, even when sync at launch is
  enabled. That optional sync is scheduled only after the initial local load
  completes or reports an error. Its result still triggers a local reload.
- Status counts are collected in one inventory pass instead of repeated full
  scans. Preparation of Updates/Discover copies only relevant packages rather
  than cloning and sorting the complete inventory.
- Hidden Discover app widgets and icon lookups are deferred until the tab is
  opened. Only catalog availability is indexed during initial loading.
- `CAERUS_TRACE_STARTUP=1` logs optional timestamps around process startup,
  repository reading, model publication and the first package-list frame.
  The diagnostic is disabled by default.

## Discover

The catalog grows from 10 entries in 5 categories to **174 applications in 13
sections**. On this host **158** resolve to available package names. The
section names adapt [Debian's section index](https://packages.debian.org/stable/):
Web, Mail & communication, Office & documents, Editors, Graphics, Sound, Video,
Games, Education, Science & mathematics, Development, Network and Utilities.

This is a curated application index, not a claim that every package in the
repository has Debian section metadata. The app does not add Debian repos,
fetch Debian packages, fetch icon services or create ratings. Package names
and aliases resolve exclusively against the active XBPS/APK inventory.
Unavailable packages are omitted; aliases have deterministic preference order.

Discover now has its own search and section selector. Its current implemented
presentation is a compact grouped list. The category sidebar/cards image is
an additional design option, not a screenshot of that implementation.

## Reproducing the checks

Build and launch an optimized binary with startup diagnostics:

```sh
cargo build --release --features adwaita
CAERUS_TRACE_STARTUP=1 target/release/caerus
```

Run the normal display smoke test and the optional real cached-inventory check
on a test display, with an isolated config directory containing
`sync_at_launch=0`:

```sh
XDG_CONFIG_HOME=/tmp/caerus-test-config \
CAERUS_TEST_LOCAL_STARTUP=1 \
cargo test -p caerus -- --ignored --test-threads=1
```

The layout fixture uses sample data; transaction tests use an unprivileged fake
helper. The opt-in inventory check reads cached metadata and performs a local reload; it never synchronizes
repositories or applies packages. It also captures the expanded Discover tab
when `CAERUS_SCREENSHOT_DIR` is set. Both plain GTK and optional Adwaita builds
are exercised during this review.

## D implementation checkpoint

After D was selected, the compact table, header navigation and smaller inspector
were implemented. Dependency and relationship sections collapse, and an empty
selection leaves the full table width available. Narrow windows retain column
choices and open details on their own page.

The real-inventory GUI checks passed for plain GTK and Adwaita, including saved
narrow geometry, row density, filter resets, queue preservation and cached
startup/reload counts. The optimized D build reached the package-list frame in
1219.9, 1188.9, 1195.9 ms
(median **1195.9 ms**, with the same warm-cache measurement method above).
All 20,724 packages loaded automatically.

[Implemented D with the real inventory](../design/mockups/compact/implemented/compact-inventory.png).
