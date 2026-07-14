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

/// Appends one tab-separated record: `RFC3339-timestamp\tjoined-commands\tOK|ERROR`.
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
    let timestamp = now_rfc3339();
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

/// No `chrono`/`time` dependency in this crate — a hand-rolled UTC
/// RFC3339 timestamp from `SystemTime` is a handful of lines and this is
/// the only place that needs one.
fn now_rfc3339() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Civil-from-days algorithm (Howard Hinnant's public-domain
    // `civil_from_days`), converting a day count since the Unix epoch
    // into a proleptic-Gregorian (year, month, day) — avoids pulling in
    // a date/time crate for this one conversion.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}
