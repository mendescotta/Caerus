//! Data model for a real, `libxbps`-computed transaction preview — the
//! same mechanism `xbps-install -n` itself uses: call
//! `xbps_transaction_install_pkg`/`_update_pkg`/`_remove_pkg` for every
//! marked package, then `xbps_transaction_prepare()` and read back
//! `xh.transd` *without* ever calling `xbps_transaction_commit()`.
//!
//! Property names and error-code mapping below were confirmed against
//! Void's own `bin/xbps-install/transaction.c`/`util.c` (upstream
//! `void-linux/xbps`), not guessed from the `xbps.h` doc comments alone.

/// One requested change, mirroring `PkgMark` but is what actually gets
/// fed to the `xbps_transaction_*` calls.
#[derive(Debug, Clone)]
pub enum PreviewOp {
    Install(String),
    Update(String),
    Remove(String),
    /// Recursive removal (also drops now-orphaned deps) — same meaning as
    /// `PURGE` in the helper's own protocol.
    Purge(String),
}

/// Mirrors `xbps_trans_type_t` in xbps.h (repr(u8) matches the
/// `"transaction"` dictionary property's on-disk type, read via
/// `xbps_dictionary_get_uint8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransAction {
    Unknown = 0,
    Install = 1,
    Reinstall = 2,
    Update = 3,
    Configure = 4,
    Remove = 5,
    Hold = 6,
    Download = 7,
}

impl TransAction {
    pub const fn from_raw(v: u8) -> Self {
        match v {
            1 => Self::Install,
            2 => Self::Reinstall,
            3 => Self::Update,
            4 => Self::Configure,
            5 => Self::Remove,
            6 => Self::Hold,
            7 => Self::Download,
            _ => Self::Unknown,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Reinstall => "reinstall",
            Self::Update => "update",
            Self::Configure => "configure",
            Self::Remove => "remove",
            Self::Hold => "hold",
            Self::Download => "download",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransactionPreviewItem {
    pub pkgname: String,
    pub pkgver: String,
    pub action: TransAction,
    pub arch: Option<String>,
    pub repository: Option<String>,
    pub installed_size: u64,
    pub download_size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TransactionPreview {
    pub items: Vec<TransactionPreviewItem>,
    pub total_download_size: u64,
    pub total_installed_size: u64,
    pub total_removed_size: u64,
    pub download_pkgs: u32,
    pub install_pkgs: u32,
    pub update_pkgs: u32,
    pub remove_pkgs: u32,
    pub hold_pkgs: u32,
}

impl TransactionPreview {
    /// Plain-text rendering suitable for pasting into a bug report — one
    /// line per package (name, version, action, arch, repo, sizes), same
    /// shape as `xbps-install -n`'s own dry-run output, plus a totals
    /// line.
    pub fn to_plain_text(&self) -> String {
        use crate::backend::package::pkg_format_size;
        use std::fmt::Write as _;
        let mut out = String::new();
        for item in &self.items {
            let _ = writeln!(
                out,
                "{} {} {} {} {} installed={} download={}",
                item.pkgname,
                item.pkgver,
                item.action.label(),
                item.arch.as_deref().unwrap_or("-"),
                item.repository.as_deref().unwrap_or("-"),
                pkg_format_size(item.installed_size),
                pkg_format_size(item.download_size),
            );
        }
        let _ = write!(
            out,
            "\n{} to install, {} to update, {} to remove, {} on hold, {} to download\n",
            self.install_pkgs,
            self.update_pkgs,
            self.remove_pkgs,
            self.hold_pkgs,
            self.download_pkgs
        );
        let _ = write!(
            out,
            "Total download size: {}\nTotal installed size: {}\nTotal removed size: {}\n",
            pkg_format_size(self.total_download_size),
            pkg_format_size(self.total_installed_size),
            pkg_format_size(self.total_removed_size),
        );
        out
    }
}

/// Mirrors the failure branches of `exec_transaction()` in Void's own
/// `bin/xbps-install/transaction.c`: `xbps_transaction_prepare()`'s
/// return value selects which array (if any) on `xh.transd` explains the
/// failure.
#[derive(Debug, Clone)]
pub enum TransactionError {
    MissingDeps(Vec<String>),
    MissingShlibs(Vec<String>),
    Conflicts(Vec<String>),
    NotEnoughSpace { need: u64, free: u64 },
    Other(String),
}

impl TransactionError {
    pub fn summary(&self) -> String {
        match self {
            Self::MissingDeps(_) => {
                "Transaction aborted due to unresolved dependencies.".to_string()
            }
            Self::MissingShlibs(_) => {
                "Transaction aborted due to unresolved shared libraries.".to_string()
            }
            Self::Conflicts(_) => "Transaction aborted due to conflicting packages.".to_string(),
            Self::NotEnoughSpace { need, free } => {
                use crate::backend::package::pkg_format_size;
                format!(
                    "Transaction aborted due to insufficient disk space (need {}, got {} free).",
                    pkg_format_size(*need),
                    pkg_format_size(*free)
                )
            }
            Self::Other(msg) => msg.clone(),
        }
    }

    pub fn details(&self) -> &[String] {
        match self {
            Self::MissingDeps(v) | Self::MissingShlibs(v) | Self::Conflicts(v) => v,
            Self::NotEnoughSpace { .. } | Self::Other(_) => &[],
        }
    }
}
