//! Persisted user-chosen display names for repositories, keyed by the
//! repository's own URL/URI (the same string `Package::repository`
//! carries). Purely a caerus UI convenience, independent of xbps
//! itself — same tiny hand-rolled persistence approach as
//! `ui::window::WindowGeometry`.

use std::collections::HashMap;
use std::path::PathBuf;

fn state_file_path() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config_home.join("caerus").join("repo-names.conf"))
}

pub struct RepoNames {
    map: HashMap<String, String>,
}

impl RepoNames {
    pub fn load() -> Self {
        let mut map = HashMap::new();
        if let Some(path) = state_file_path() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                // Tab-separated rather than "key=value": URLs never
                // contain a tab, but could in principle contain '='
                // (query strings), so that wouldn't reliably round-trip.
                for line in contents.lines() {
                    if let Some((url, name)) = line.split_once('\t') {
                        if !url.is_empty() && !name.is_empty() {
                            map.insert(url.to_string(), name.to_string());
                        }
                    }
                }
            }
        }
        RepoNames { map }
    }

    fn save(&self) {
        let Some(path) = state_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut contents = String::new();
        for (url, name) in &self.map {
            contents.push_str(url);
            contents.push('\t');
            contents.push_str(name);
            contents.push('\n');
        }
        let _ = std::fs::write(&path, contents);
    }

    pub fn get(&self, url: &str) -> Option<&str> {
        self.map.get(url).map(String::as_str)
    }

    /// Sets a custom display name, or clears it back to the default
    /// (scheme-stripped URL) if `name` is empty/whitespace-only.
    pub fn set(&mut self, url: &str, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.map.remove(url);
        } else {
            self.map.insert(url.to_string(), trimmed.to_string());
        }
        self.save();
    }
}

/// Strips a leading "https://" or "http://" — the scheme is rarely
/// useful clutter in a short sidebar row or the detail pane.
pub fn display_repo(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}
