# Discover coverage and maintenance

Discover resolves a curated catalog against packages available from the user's
enabled repositories. It does not infer categories from every repository package.
Consequently, an available application stays invisible until a catalog entry (with
the correct, case-sensitive package name) covers it. Category membership was also
inconsistent: several backup and password applications were filed under Utilities.

## Current coverage, 2026-10-30

The official Void **x86_64 glibc main and nonfree** binary repositories were queried
with `xbps-query -i -M`. Configured third-party repositories were ignored. No
packages were installed and no repository configuration was changed.

| Measure | 2026-10-29 start | 2026-10-29 | 2026-10-30 |
| --- | ---: | ---: | ---: |
| Catalog entries | 254 | 421 | 743 |
| Sections | 19 | 25 | 28 |
| Entries available in these repositories | 238 | 411 | 733 |

The second pass adds **322 verified applications/tools** and three sections:

| Section | Available entries | Examples |
| --- | ---: | --- |
| Emulation | 24 | RetroArch, Dolphin, DOSBox Staging, PPSSPP, mGBA, melonDS, ScummVM, MAME |
| Desktop & appearance | 17 | GNOME Tweaks, Extension Manager, nwg-look, qt5ct/qt6ct, Kvantum, Font Manager |
| Printing & scanning | 10 | Print Settings, KDE Print Manager, CUPS, HPLIP, Skanpage, XSane, Document Scanner |

It also fills out existing sections, for example Games (+28: Xonotic, OpenRA, OpenMW,
Prism Launcher, GNOME/KDE puzzle games), Sound (+17), Graphics (+16), Development
(+16), Network (+17), Security & privacy (+19), Office & documents (+14),
Virtualization & containers (+14: GNOME Boxes, VirtualBox, Incus, LXD) and Utilities
(+18: hardware, GPU and peripheral tools). Document Scanner moved from Utilities to
Printing & scanning. Libraries, daemons with no user-facing tool, plugins,
per-architecture QEMU system packages and duplicate versions were left out.

Evidence is saved in:

- [Selected packages and launcher verification](void-verification.json):
  exact binary versions, source repositories, launcher filenames and category/name
  review for every entry added in either pass.
- [Complete catalog availability audit](void-catalog.json): available
  packages, counts by section, missing aliases and unavailable applications.

Verification establishes binary availability and the packaged `.desktop` filenames
of the selected additions. It does **not** establish that an application installs
or runs successfully, nor that it exists on musl/ARM or Chimera. A package with no
launcher uses an empty desktop ID and a fallback icon; this includes both
command-line tools and GUIs such as Ghidra that ship no desktop file. Where a
package ships several launchers (for example VICE, ZynAddSubFX, Xonotic), the
general-purpose one was chosen.

Ten pre-existing entries have no matching alias in this repository snapshot:
Pinta, Pitivi, Tux Math, KGeography, Kalzium, Parley, GeoGebra, VSCodium, Warpinator
and KeePass. They remain in the cross-backend catalog and are filtered out when
unavailable.

## Repeatable discovery workflow

The reusable tool is [scripts/check-discover-void.py](../check-discover-void.py).
Its reviewed keyword rules live in
[scripts/discover-rules.json](../discover-rules.json).
Before the 2026-10-30 pass a scan of 14,898 packages produced 462 uncatalogued
suggestions; this is a backlog of keyword matches, not verified applications.

1. Scan the current official repository inventory for names/descriptions matching
   useful application groups:

   ```sh
   python3 scripts/check-discover-void.py --discover --nonfree --output /tmp/discover-candidates.json
   ```

   This excludes existing catalog aliases, common development/debug/data
   subpackages, libraries, plugins, extensions and transitional packages. Each
   suggestion records the matched terms and possible sections. These are review
   inputs: descriptions can mislead (for example a network scanner versus a
   document scanner), and some applications will not match any keyword.

2. Select actual standalone applications or useful end-user tools. Avoid adding
   libraries, content packs, plugins, compatibility packages and multiple versions
   of the same application merely to raise counts. Verify exact package names and
   launchers:

   ```sh
   python3 scripts/check-discover-void.py --nonfree --desktop-files --output /tmp/selected-apps.json cutter restic timeshift
   ```

   File-list requests can download additional binary package metadata; discovery
   intentionally does not fetch file lists for every suggestion. Choose the main
   launcher rather than an autostart, settings or helper launcher. Leave the ID
   empty if none is packaged. Review `NoDisplay`, `Hidden`, `Exec` and `Terminal`
   in desktop/AppStream metadata when assessing ambiguous candidates; launcher
   filename verification alone does not inspect those fields.

3. Add reviewed entries in `caerus/src/ui/catalog.rs`, with alternatives as aliases
   of one entry. Add a new section and icon/style only when a coherent group of
   verified tools warrants it. Then audit the full catalog:

   ```sh
   python3 scripts/check-discover-void.py --catalog --nonfree --output /tmp/catalog-audit.json
   python3 scripts/test-check-discover-void.py
   cargo test -p caerus ui::catalog::tests
   ```

   Exit status 0 means all requested aliases were found; 1 means some are missing;
   2 means the check failed. For `--catalog`, inspect `unavailable_apps` and
   `available_apps` as well as `missing`: a missing alternate alias is expected
   when another alias resolves. Do not treat a network/parser error as absence.

Omit `--nonfree` for main-only checks. Set `XBPS_ARCH=x86_64-musl` or
`XBPS_ARCH=aarch64` to check another binary architecture; the tool selects the
official repository path. Those checks must succeed before claiming coverage for
that architecture. The [Void repository documentation](https://docs.voidlinux.org/xbps/repositories/index.html)
describes the architecture paths and optional repositories.

For broader long-term coverage, supplement description searches with desktop and
AppStream metadata extracted during repository indexing/builds. Map that metadata
back to exact binary package names, merge aliases, and send uncatalogued apps to
review. This catches apps with terse descriptions and supplies categories and
human-readable names. Keep keyword matching as a candidate generator rather than
runtime automatic categorization, which would put libraries and unrelated tools
into Discover.
