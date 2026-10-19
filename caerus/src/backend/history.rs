use std::io::Write;
use std::path::PathBuf;

#[derive(Debug)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub commands: String,
    pub success: bool,
    pub changes: String,
    pub log: String,
}

impl HistoryEntry {
    pub fn summary(&self) -> String {
        self.commands
            .split(" | ")
            .map(|command| {
                let (verb, names) = command.split_once(' ').unwrap_or((command, ""));
                let action = match verb {
                    "INSTALL" => "Install / upgrade",
                    "UPDATE" => "Upgrade",
                    "REMOVE" => "Remove",
                    "PURGE" => "Remove with unused dependencies",
                    "UPGRADE" => "System upgrade",
                    "SYNC" => "Refresh repositories",
                    "CLEANCACHE" => "Clean package cache",
                    "VERIFY" => "Verify packages",
                    _ => verb,
                };
                format!("{action} {names}").trim().to_owned()
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        [&self.timestamp, &self.commands, &self.changes, &self.log]
            .iter()
            .any(|s| s.to_lowercase().contains(&query))
    }
}

fn data_file_path() -> Option<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data_home.join("caerus/history.log"))
}

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_TRANSACTION_LOG: usize = 128 * 1024;

fn rotate_if_large(path: &std::path::Path, max_bytes: u64) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > max_bytes) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
}

fn encode(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}
fn decode(text: &str) -> String {
    text.replace("%0D", "\r")
        .replace("%0A", "\n")
        .replace("%09", "\t")
        .replace("%25", "%")
}
fn parse(line: &str) -> Option<HistoryEntry> {
    let fields: Vec<_> = line.split('\t').collect();
    match fields.as_slice() {
        ["v2", time, commands, status, changes, log] => Some(HistoryEntry {
            timestamp: decode(time),
            commands: decode(commands),
            success: *status == "OK",
            changes: decode(changes),
            log: decode(log),
        }),
        [time, commands, status] => Some(HistoryEntry {
            timestamp: (*time).into(),
            commands: (*commands).into(),
            success: *status == "OK",
            changes: String::new(),
            log: String::new(),
        }),
        _ => None,
    }
}

pub fn record(commands: &[String], success: bool) {
    record_detailed(commands, success, "", "");
}

pub fn record_detailed(commands: &[String], success: bool, changes: &str, log: &str) {
    if commands.is_empty() {
        return;
    }
    let Some(path) = data_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    rotate_if_large(&path, MAX_LOG_BYTES);
    let timestamp = glib::DateTime::now_local()
        .ok()
        .and_then(|dt| dt.format("%Y-%m-%d %H:%M:%S").ok())
        .map_or_else(|| "unknown-time".into(), |s| s.to_string());
    let log: String = log.chars().take(MAX_TRANSACTION_LOG).collect();
    let line = format!(
        "v2\t{}\t{}\t{}\t{}\t{}\n",
        encode(&timestamp),
        encode(&commands.join(" | ")),
        if success { "OK" } else { "ERROR" },
        encode(changes),
        encode(&log)
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn load() -> Vec<HistoryEntry> {
    let Some(path) = data_file_path() else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for file in [path.with_extension("log.1"), path] {
        if let Ok(text) = std::fs::read_to_string(file) {
            entries.extend(text.lines().filter_map(parse));
        }
    }
    entries.reverse();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scratch directory under the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("caerus-history-{}-{name}", std::process::id()));
            if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                let _ = std::fs::remove_dir_all(&dir);
            }
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn small_log_is_not_rotated() {
        let scratch = Scratch::new("small");
        let dir = &scratch.0;
        let log = dir.join("history.log");
        std::fs::write(&log, "x").unwrap();
        rotate_if_large(&log, 10);
        assert!(log.exists());
        assert!(!dir.join("history.log.1").exists());
    }

    #[test]
    fn large_log_moves_to_backup() {
        let scratch = Scratch::new("large");
        let dir = &scratch.0;
        let log = dir.join("history.log");
        std::fs::write(&log, "0123456789ab").unwrap();
        rotate_if_large(&log, 10);
        assert!(!log.exists());
        assert!(dir.join("history.log.1").exists());
    }

    #[test]
    fn new_records_preserve_multiline_unicode_and_literal_escape_sequences() {
        let text = "foo 1 → 2\nERROR\t%0A\r";
        assert_eq!(decode(&encode(text)), text);
        let entry = parse(&format!(
            "v2\tnow\tINSTALL foo\tERROR\t{}\t{}",
            encode(text),
            encode("failed\nsecond line")
        ))
        .unwrap();
        assert!(entry.matches("foo 1"));
        assert!(entry.matches("second line"));
        assert!(!entry.success);
        assert_eq!(entry.summary(), "Install / upgrade foo");
    }
    #[test]
    fn legacy_records_remain_readable() {
        let entry = parse("2026-01-01\tSYNC\tOK").unwrap();
        assert!(entry.success);
        assert!(entry.log.is_empty());
        assert!(parse("truncated").is_none());
    }
    #[test]
    fn rotation_retains_previous_log() {
        let scratch = Scratch::new("retain");
        let path = scratch.0.join("history.log");
        std::fs::write(&path, "previous records").unwrap();
        rotate_if_large(&path, 100);
        assert!(path.exists());
        rotate_if_large(&path, 1);
        assert_eq!(
            std::fs::read_to_string(path.with_extension("log.1")).unwrap(),
            "previous records"
        );
    }
}
