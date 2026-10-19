#[derive(Debug, Clone)]
pub enum PreviewOp {
    #[cfg(feature = "xbps")]
    UpdateAll,
    Install(String),
    Update(String),
    Remove(String),
    Purge(String),
    #[cfg(feature = "apk")]
    FullUpgrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransAction {
    #[cfg(feature = "xbps")]
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
    #[cfg(feature = "xbps")]
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
}

#[derive(Debug, Clone)]
pub struct TransactionPreviewItem {
    pub pkgname: String,
    pub pkgver: String,
    pub previous_version: Option<String>,
    pub requested: bool,
    pub action: TransAction,
    pub installed_size: u64,
    pub download_size: u64,
}

impl TransactionPreviewItem {
    pub fn description(&self) -> String {
        let versions = match (&self.previous_version, self.action) {
            (Some(old), TransAction::Remove) => format!("{old} → removed"),
            (Some(old), _) => format!("{old} → {}", self.pkgver),
            (None, _) => self.pkgver.clone(),
        };
        format!(
            "{} · {} · {}",
            self.pkgname,
            versions,
            if self.requested {
                "Requested"
            } else {
                "Dependency / solver change"
            }
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct TransactionPreview {
    pub items: Vec<TransactionPreviewItem>,
    pub total_download_size: u64,
    pub total_installed_size: u64,
    pub total_removed_size: u64,
    pub disk_size_change: i128,
    pub download_pkgs: u32,
    pub install_pkgs: u32,
    pub update_pkgs: u32,
    pub remove_pkgs: u32,
    #[cfg(feature = "xbps")]
    pub hold_pkgs: u32,
}

impl TransactionPreview {
    pub fn disk_change_text(&self) -> String {
        let size = u64::try_from(self.disk_size_change.unsigned_abs()).unwrap_or(u64::MAX);
        format!(
            "{}{}",
            if self.disk_size_change < 0 {
                "−"
            } else if self.disk_size_change > 0 {
                "+"
            } else {
                ""
            },
            crate::backend::package::pkg_format_size(size)
        )
    }
    pub fn changes(&self) -> String {
        self.items
            .iter()
            .map(|item| format!("{:?} · {}", item.action, item.description()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Disk changes compare complete installed payload sizes, not archive sizes.
pub fn installed_delta(action: TransAction, previous: Option<u64>, target: u64) -> i128 {
    match action {
        TransAction::Install | TransAction::Reinstall | TransAction::Update => {
            i128::from(target) - i128::from(previous.unwrap_or(0))
        }
        TransAction::Remove => -i128::from(previous.unwrap_or(target)),
        _ => 0,
    }
}

#[derive(Debug, Clone)]
pub enum TransactionError {
    #[cfg(feature = "xbps")]
    MissingDeps(Vec<String>),
    #[cfg(feature = "xbps")]
    MissingShlibs(Vec<String>),
    #[cfg(feature = "xbps")]
    Conflicts(Vec<String>),
    #[cfg(feature = "xbps")]
    NotEnoughSpace {
        need: u64,
        free: u64,
    },
    Other(String),
}

impl TransactionError {
    pub fn summary(&self) -> String {
        match self {
            #[cfg(feature = "xbps")]
            Self::MissingDeps(_) => {
                "Transaction aborted due to unresolved dependencies.".to_string()
            }
            #[cfg(feature = "xbps")]
            Self::MissingShlibs(_) => {
                "Transaction aborted due to unresolved shared libraries.".to_string()
            }
            #[cfg(feature = "xbps")]
            Self::Conflicts(_) => "Transaction aborted due to conflicting packages.".to_string(),
            #[cfg(feature = "xbps")]
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
            #[cfg(feature = "xbps")]
            Self::MissingDeps(v) | Self::MissingShlibs(v) | Self::Conflicts(v) => v,
            #[cfg(feature = "xbps")]
            Self::NotEnoughSpace { .. } => &[],
            Self::Other(_) => &[],
        }
    }
}

#[cfg(all(test, feature = "xbps"))]
mod tests {
    use super::*;

    #[test]
    fn disk_delta_counts_growth_shrinkage_and_removal_instead_of_total_payload() {
        assert_eq!(installed_delta(TransAction::Update, Some(100), 120), 20);
        assert_eq!(installed_delta(TransAction::Update, Some(100), 80), -20);
        assert_eq!(installed_delta(TransAction::Install, None, 40), 40);
        assert_eq!(installed_delta(TransAction::Remove, Some(100), 100), -100);
        assert_eq!(installed_delta(TransAction::Download, Some(100), 120), 0);
        let preview = TransactionPreview {
            disk_size_change: -1024,
            ..TransactionPreview::default()
        };
        assert_eq!(preview.disk_change_text(), "−1.0 KiB");
    }

    #[test]
    fn change_descriptions_preserve_old_versions_and_solver_reasons() {
        let item = TransactionPreviewItem {
            pkgname: "foo".into(),
            pkgver: "2.0_1".into(),
            previous_version: Some("1.0_1".into()),
            requested: false,
            action: TransAction::Update,
            installed_size: 0,
            download_size: 0,
        };
        assert!(item.description().contains("1.0_1 → 2.0_1"));
        assert!(item.description().contains("Dependency / solver change"));
        let removed = TransactionPreviewItem {
            action: TransAction::Remove,
            requested: true,
            ..item
        };
        assert!(removed.description().contains("1.0_1 → removed"));
        assert!(removed.description().contains("Requested"));
    }

    #[test]
    fn error_summaries_name_the_failure_class() {
        assert!(TransactionError::MissingDeps(vec![])
            .summary()
            .contains("unresolved dependencies"));
        assert!(TransactionError::MissingShlibs(vec![])
            .summary()
            .contains("shared libraries"));
        assert!(TransactionError::Conflicts(vec![])
            .summary()
            .contains("conflicting packages"));
        let space = TransactionError::NotEnoughSpace {
            need: 2_147_483_648,
            free: 1_048_576,
        };
        assert_eq!(
            space.summary(),
            "Transaction aborted due to insufficient disk space (need 2.0 GiB, got 1.0 MiB free)."
        );
        assert_eq!(
            TransactionError::Other("boom".to_string()).summary(),
            "boom"
        );
    }

    #[test]
    fn error_details_expose_lists_only_where_they_exist() {
        let deps = vec!["libfoo>=1.0".to_string(), "libbar>=2.0".to_string()];
        assert_eq!(
            TransactionError::MissingDeps(deps.clone()).details(),
            deps.as_slice()
        );
        assert!(TransactionError::NotEnoughSpace { need: 1, free: 0 }
            .details()
            .is_empty());
        assert!(TransactionError::Other("x".to_string())
            .details()
            .is_empty());
    }

    #[test]
    fn trans_action_raw_roundtrip() {
        for v in 0u8..=8 {
            let a = TransAction::from_raw(v);
            match v {
                1..=7 => assert_ne!(a, TransAction::Unknown),
                _ => assert_eq!(a, TransAction::Unknown),
            }
        }
        assert_eq!(TransAction::from_raw(5), TransAction::Remove);
    }
}
