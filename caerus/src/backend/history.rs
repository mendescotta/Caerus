use std::io::Write;
use std::path::PathBuf;

pub struct HistoryEntry {
    pub timestamp: String,
    pub commands: String,
    pub success: bool,
}

fn data_file_path() -> Option<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share"))
        })?;
    Some(data_home.join("caerus").join("history.log"))
}

const MAX_LOG_BYTES: u64 = 256 * 1024;

fn rotate_if_large(path: &std::path::Path, max_bytes: u64) {
    let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > max_bytes);
    if too_big {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
}

pub fn record(commands: &[String], success: bool) {
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
    let timestamp = now_local();
    let joined = commands.join(" | ").replace(['\t', '\n'], " ");
    let line = format!(
        "{}\t{}\t{}\n",
        timestamp,
        joined,
        if success { "OK" } else { "ERROR" }
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn load() -> Vec<HistoryEntry> {
    let Some(path) = data_file_path() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out: Vec<HistoryEntry> = contents
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let timestamp = parts.next()?.to_string();
            let commands = parts.next()?.to_string();
            let success = parts.next()? == "OK";
            Some(HistoryEntry {
                timestamp,
                commands,
                success,
            })
        })
        .collect();
    out.reverse();
    out
}

fn now_local() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|dt| dt.format("%Y-%m-%d %H:%M:%S").ok())
        .map_or_else(|| "unknown-time".to_string(), |s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("caerus-history-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn small_log_is_not_rotated() {
        let dir = scratch("small");
        let log = dir.join("history.log");
        std::fs::write(&log, "x").unwrap();
        rotate_if_large(&log, 10);
        assert!(log.exists());
        assert!(!dir.join("history.log.1").exists());
    }

    #[test]
    fn large_log_moves_to_backup() {
        let dir = scratch("large");
        let log = dir.join("history.log");
        std::fs::write(&log, "0123456789ab").unwrap();
        rotate_if_large(&log, 10);
        assert!(!log.exists());
        assert!(dir.join("history.log.1").exists());
    }
}
