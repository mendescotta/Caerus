//! Plain data model for a single xbps package, plus the `PackageObject`
//! `GObject` wrapper needed to put `Package` values into a `gio::ListStore`
//! (GTK4's list widgets — `gtk::ColumnView` here — only work with
//! `glib::Object`-derived items).

use glib::subclass::prelude::*;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PkgState {
    #[default]
    NotInstalled,
    Installed,
    Upgradable,
    OnHold,
    Broken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PkgMark {
    #[default]
    None,
    Install,
    Remove,
    Upgrade,
    Purge,
}

/// Row index in the filter sidebar's preset list maps directly onto
/// this enum's discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum FilterMode {
    All = 0,
    Installed = 1,
    NotInstalled = 2,
    Upgradable = 3,
    OnHold = 4,
    Marked = 5,
    Orphaned = 6,
}

impl FilterMode {
    pub const fn from_row_index(i: i32) -> Self {
        match i {
            1 => Self::Installed,
            2 => Self::NotInstalled,
            3 => Self::Upgradable,
            4 => Self::OnHold,
            5 => Self::Marked,
            6 => Self::Orphaned,
            _ => Self::All,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: String,
    pub version_installed: Option<String>,
    pub version_available: Option<String>,
    pub short_desc: String,
    pub long_desc: Option<String>,
    /// xbps "tags" property, joined with ", " if it was an array.
    pub tags: String,
    pub maintainer: String,
    pub install_size: u64,
    pub download_size: u64,
    /// The repository this package's data came from — the source
    /// repo's URI for anything found in `xbps_rpool_foreach`, or the
    /// pkgdb's own recorded "repository" property (which takes
    /// precedence, since it's what an installed package actually came
    /// from) for installed packages. `None` for local/orphan pkgdb
    /// entries not backed by any configured repo.
    pub repository: Option<String>,
    pub state: PkgState,
    pub mark: PkgMark,
    pub essential: bool,
    /// xbps "architecture" property (e.g. "`x86_64`", "noarch").
    pub arch: Option<String>,
    /// Computed once per reload via `xbps_find_pkg_orphans` — true if
    /// this package is installed but nothing else depends on it anymore.
    pub is_orphan: bool,
    /// xbps "repolock" property (installed packages only) — true if this
    /// package is pinned to only ever upgrade from the repository it was
    /// originally installed from. Set via `xbps-pkgdb -m repolock`.
    pub is_repolocked: bool,
}

/// On-demand metadata not loaded during the bulk scan.
/// `install_date`/`automatic_install` are only ever populated for
/// installed packages.
#[derive(Debug, Clone, Default)]
pub struct PackageExtraInfo {
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub install_date: Option<String>,
    pub automatic_install: bool,
    pub has_automatic_install: bool,
    pub download_size: u64,
    /// Virtual packages/symlinked commands this package provides.
    pub provides: Vec<String>,
    /// Other packages this package can't be installed alongside.
    pub conflicts: Vec<String>,
    /// Other packages this package supersedes/replaces.
    pub replaces: Vec<String>,
    /// Shared library sonames this package needs at runtime.
    pub shlib_requires: Vec<String>,
    /// Shared library sonames this package makes available to others.
    pub shlib_provides: Vec<String>,
}

pub const fn pkg_state_icon(state: PkgState, mark: PkgMark) -> Option<&'static str> {
    match mark {
        PkgMark::Install => return Some("list-add-symbolic"),
        PkgMark::Remove => return Some("list-remove-symbolic"),
        PkgMark::Purge => return Some("edit-delete-symbolic"),
        PkgMark::Upgrade => return Some("software-update-available-symbolic"),
        PkgMark::None => {}
    }
    match state {
        PkgState::Installed => Some("object-select-symbolic"),
        PkgState::Upgradable => Some("software-update-available-symbolic"),
        PkgState::OnHold => Some("media-playback-pause-symbolic"),
        PkgState::Broken => Some("dialog-warning-symbolic"),
        PkgState::NotInstalled => None,
    }
}

pub const fn pkg_state_tooltip(state: PkgState, mark: PkgMark) -> &'static str {
    match mark {
        PkgMark::Install => return "Marked for installation",
        PkgMark::Remove => return "Marked for removal",
        PkgMark::Purge => return "Marked for purge",
        PkgMark::Upgrade => return "Marked for upgrade",
        PkgMark::None => {}
    }
    match state {
        PkgState::Installed => "Installed",
        PkgState::Upgradable => "Upgrade available",
        PkgState::OnHold => "On hold",
        PkgState::Broken => "Broken",
        PkgState::NotInstalled => "Not installed",
    }
}

pub fn pkg_format_size(bytes: u64) -> String {
    const GIB: f64 = 1_073_741_824.0;
    const MIB: f64 = 1_048_576.0;
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if bytes as f64 >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else if bytes as f64 >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

// ── GObject wrapper ─────────────────────────────────────────────────
// A thin `glib::Object` subclass holding one `Package` in a `RefCell`.
// gio::ListStore requires items to be GObjects, so every `Package` gets
// wrapped in one of these before going into the store.

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PackageObject {
        pub pkg: RefCell<Package>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PackageObject {
        const NAME: &'static str = "CaerusPackageObject";
        type Type = super::PackageObject;
    }

    impl ObjectImpl for PackageObject {}
}

glib::wrapper! {
    pub struct PackageObject(ObjectSubclass<imp::PackageObject>);
}

impl PackageObject {
    pub fn new(pkg: Package) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().pkg.replace(pkg);
        obj
    }

    /// Borrow the underlying `Package`. Cheap (`RefCell::borrow`); do
    /// not hold across a call that might re-borrow it mutably.
    pub fn pkg(&self) -> std::cell::Ref<'_, Package> {
        self.imp().pkg.borrow()
    }

    pub fn name(&self) -> String {
        self.imp().pkg.borrow().name.clone()
    }
}
