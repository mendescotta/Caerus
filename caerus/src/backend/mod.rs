pub mod custom_filters;
pub mod history;
pub mod package;
#[cfg(all(feature = "apk", feature = "xbps"))]
compile_error!("features `apk` and `xbps` are mutually exclusive; select one package backend");

#[cfg(feature = "xbps")]
#[path = "package_store.rs"]
pub mod package_store;
#[cfg(feature = "apk")]
#[path = "package_store_apk.rs"]
pub mod package_store;
pub mod repo_names;
pub mod transaction;
pub mod transaction_preview;
