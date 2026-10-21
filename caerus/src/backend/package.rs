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
    RepoLocked = 7,
    Unmaintained = 8,
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
            7 => Self::RepoLocked,
            8 => Self::Unmaintained,
            _ => Self::All,
        }
    }
}

impl Package {
    /// Returns whether the available package version is newer than the installed version.
    ///
    /// Versions are compared using the package backend's ordering; missing versions and equal or older
    /// available versions return `false`.
    pub fn has_newer_available_version(&self) -> bool {
        matches!(
            (&self.version_installed, &self.version_available),
            (Some(installed), Some(available))
                if crate::backend::package_store::compare_versions(available, installed)
                    == std::cmp::Ordering::Greater
        )
    }

    pub fn is_held_back(&self) -> bool {
        self.state == PkgState::OnHold && self.has_newer_available_version()
    }
}

pub const ORPHANED_MAINTAINER: &str = "Orphaned <orphan@voidlinux.org>";

#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: String,
    pub version_installed: Option<String>,
    pub version_available: Option<String>,
    pub short_desc: String,
    pub long_desc: Option<String>,
    pub tags: String,
    pub maintainer: String,
    pub install_size: u64,
    pub download_size: u64,
    pub repository: Option<String>,
    pub state: PkgState,
    pub mark: PkgMark,
    pub essential: bool,
    pub arch: Option<String>,
    pub is_orphan: bool,
    pub is_repolocked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PackageExtraInfo {
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub install_date: Option<String>,
    pub automatic_install: bool,
    pub has_automatic_install: bool,
    pub download_size: u64,
    pub provides: Vec<String>,
    pub conflicts: Vec<String>,
    pub replaces: Vec<String>,
    pub shlib_requires: Vec<String>,
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

    pub fn pkg(&self) -> std::cell::Ref<'_, Package> {
        self.imp().pkg.borrow()
    }

    pub fn name(&self) -> String {
        self.imp().pkg.borrow().name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(state: PkgState, installed: Option<&str>, available: Option<&str>) -> Package {
        Package {
            state,
            version_installed: installed.map(str::to_owned),
            version_available: available.map(str::to_owned),
            ..Package::default()
        }
    }

    #[test]
    fn held_package_with_newer_version_is_held_back() {
        assert!(pkg(PkgState::OnHold, Some("1.0_1"), Some("1.1_1")).is_held_back());
    }

    #[test]
    fn held_package_at_latest_or_unheld_is_not_held_back() {
        assert!(!pkg(PkgState::OnHold, Some("1.0_1"), Some("1.0_1")).is_held_back());
        assert!(!pkg(PkgState::OnHold, Some("1.0_1"), None).is_held_back());
        assert!(!pkg(PkgState::Upgradable, Some("1.0_1"), Some("1.1_1")).is_held_back());
    }

    #[cfg(feature = "xbps")]
    #[test]
    fn held_back_uses_xbps_version_ordering() {
        assert!(pkg(PkgState::OnHold, Some("1.9_1"), Some("1.10_1")).is_held_back());
        assert!(!pkg(PkgState::OnHold, Some("1.10_1"), Some("1.9_1")).is_held_back());
        assert!(pkg(PkgState::OnHold, Some("1.0_2"), Some("1.0_10")).is_held_back());
    }

    #[cfg(feature = "xbps")]
    #[test]
    fn newer_available_version_uses_xbps_version_ordering() {
        assert!(
            pkg(PkgState::Installed, Some("1.9_1"), Some("1.10_1")).has_newer_available_version()
        );
        assert!(
            !pkg(PkgState::Installed, Some("1.10_1"), Some("1.9_1")).has_newer_available_version()
        );
        assert!(
            pkg(PkgState::Installed, Some("1.0_2"), Some("1.0_10")).has_newer_available_version()
        );
        assert!(
            !pkg(PkgState::Installed, Some("1.0_1"), Some("1.0_1")).has_newer_available_version()
        );
        assert_eq!(
            crate::backend::package_store::compare_versions("1.01_1", "1.1_1"),
            std::cmp::Ordering::Equal
        );
        assert!(
            !pkg(PkgState::Installed, Some("1.01_1"), Some("1.1_1")).has_newer_available_version()
        );
        assert!(!pkg(PkgState::NotInstalled, None, Some("1.1_1")).has_newer_available_version());
    }
}
