//! Persists a one-line-per-batch record of every privileged command
//! batch caerus has actually run, so past actions are visible somewhere
//! other than the (ephemeral, per-dialog) apply dialog's Details
//! expander. Deliberately not a log of every raw `LOG` line the helper
//! emits — that's already shown live during the batch and would make
//! this file grow unboundedly fast; one row per batch keeps it small and
//! scannable, matching "actionable, not verbose."
//!
//! Explicitly out of scope: rollback. This module only records what
//! happened, it doesn't know how to undo it.

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

/// Appends one tab-separated record: `local-timestamp\tjoined-commands\tOK|ERROR`.
/// `commands` are the raw protocol lines (e.g. `"INSTALL foo bar"`,
/// `"REMOVE baz"`) that made up this batch — joined with " | " for
/// display, since a single Apply can carry several.
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

/// Parses the history file back, newest first.
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

/// Local wall-clock time, human-readable — this string is shown verbatim
/// in the Transaction History dialog, and a UTC timestamp there (the
/// previous behavior) read hours off from when the user actually did the
/// thing. `glib` is already a dependency, so its `DateTime` replaces the
/// hand-rolled civil-from-days conversion this module used to carry.
/// Older history lines recorded in the previous `...Z` UTC format still
/// parse and display fine — they're plain strings either way.
fn now_local() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|dt| dt.format("%Y-%m-%d %H:%M:%S").ok())
        .map_or_else(|| "unknown-time".to_string(), |s| s.to_string())
}
