//! Plain data model for a single xbps package, plus the `PackageObject`
//! GObject wrapper needed to put `Package` values into a `gio::ListStore`
//! (GTK4's list widgets — `gtk::ColumnView` here — only work with
//! `glib::Object`-derived items).
//!
//! Direct translation of backend/package.h + backend/package.c. The
//! plain-old-data `Package` struct maps 1:1 onto the original C struct;
//! `PackageObject` maps onto `CaerusPackageObject`.

use glib::subclass::prelude::*;
use std::cell::RefCell;

/// Mirrors `PkgState` in the original `package.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PkgState {
    #[default]
    NotInstalled,
    Installed,
    Upgradable,
    OnHold,
    Broken,
}

/// Mirrors `PkgMark`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PkgMark {
    #[default]
    None,
    Install,
    Remove,
    Upgrade,
    Purge,
}

/// Mirrors `FilterMode`. Row index in the filter sidebar's preset list
/// maps directly onto this enum's discriminant, exactly as it did in
/// the original (`ui/filter_sidebar.c`'s `on_preset_selected`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum FilterMode {
    All = 0,
    Installed = 1,
    NotInstalled = 2,
    Upgradable = 3,
    OnHold = 4,
    Marked = 5,
}

impl FilterMode {
    pub fn from_row_index(i: i32) -> Self {
        match i {
            1 => FilterMode::Installed,
            2 => FilterMode::NotInstalled,
            3 => FilterMode::Upgradable,
            4 => FilterMode::OnHold,
            5 => FilterMode::Marked,
            _ => FilterMode::All,
        }
    }
}

/// Plain package record. Mirrors `Package` in package.h field-for-field.
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
    pub state: PkgState,
    pub mark: PkgMark,
    pub essential: bool,
}

/// On-demand metadata not loaded during the bulk scan. Mirrors
/// `PackageExtraInfo`. `install_date`/`automatic_install` are only ever
/// populated for installed packages.
#[derive(Debug, Clone, Default)]
pub struct PackageExtraInfo {
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub install_date: Option<String>,
    pub automatic_install: bool,
    pub has_automatic_install: bool,
    pub download_size: u64,
}

pub fn pkg_state_icon(state: PkgState, mark: PkgMark) -> Option<&'static str> {
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

pub fn pkg_state_tooltip(state: PkgState, mark: PkgMark) -> &'static str {
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
    const GIB: f64 = 1073741824.0;
    const MIB: f64 = 1048576.0;
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if bytes as f64 >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else if bytes as f64 >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{} B", bytes)
    }
}

// ── GObject wrapper ─────────────────────────────────────────────────
//
// A thin `glib::Object` subclass holding one `Package` in a `RefCell`.
// This is the Rust equivalent of `CaerusPackageObject`: gio::ListStore
// (and therefore gtk::ColumnView/SingleSelection/FilterListModel/
// SortListModel) requires items to be GObjects, so every `Package`
// gets wrapped in one of these before going into the store.

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
    /// not hold across a call that might re-borrow mutably (`set_mark`)
    /// on the same object.
    pub fn pkg(&self) -> std::cell::Ref<'_, Package> {
        self.imp().pkg.borrow()
    }

    pub fn name(&self) -> String {
        self.imp().pkg.borrow().name.clone()
    }

    pub fn set_mark(&self, mark: PkgMark) {
        self.imp().pkg.borrow_mut().mark = mark;
    }

    /// Replaces the whole record in place (used when a reload delivers
    /// fresh data for a package that's still selected/visible).
    pub fn replace(&self, pkg: Package) {
        self.imp().pkg.replace(pkg);
    }
}
