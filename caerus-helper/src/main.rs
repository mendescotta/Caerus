//! caerus-helper — transaction executor, spawned by caerus via `pkexec`.
//!
//! Direct Rust translation of the original C helper (src/helper/
//! caerus-helper.c). caerus itself runs entirely unprivileged; only this
//! helper is ever elevated. It has no GTK, no libxbps FFI, and (by
//! design — see Cargo.toml) no dependencies at all: it is the one
//! privileged component in the project, so it stays as small and
//! auditable as possible, exactly like its C predecessor.
//!
//! Protocol (line-oriented stdin/stdout), unchanged from the original:
//!   READY            — helper ready, sent once at startup
//!   INSTALL p1 p2    — install (or upgrade) packages
//!   REMOVE  p1 p2    — remove packages
//!   PURGE   p1 p2    — recursive removal (also drops now-orphaned deps)
//!   UPGRADE          — full system upgrade (-Su)
//!   SYNC             — sync repository indexes (-S)
//!   HOLD    p1 p2    — pin package(s) at their current version
//!   UNHOLD  p1 p2    — release a previously-set hold
//!   REINSTALL p1 p2  — force re-installation of already-installed package(s)
//!   RECONFIGURE p1 p2 — re-run post-install configuration script(s)
//!   `RECONFIGURE_ALL`  — force re-run every installed package's
//!                      post-install configuration script (-fa)
//!   DOWNLOAD p1 p2   — fetch and verify package(s), don't install
//!   REPOLOCK p1 p2   — only ever upgrade from the currently-installed repo
//!   REPOUNLOCK p1 p2 — release a previously-set repo-lock
//!   MARKAUTO p1 p2   — mark package(s) as automatically installed
//!   MARKMANUAL p1 p2 — mark package(s) as explicitly/manually installed
//!   `INSTALL_FORCE` p1 p2 — install, ignoring detected file conflicts
//!   `REMOVE_FORCE` p1 p2  — remove despite unresolved revdeps/shared libs
//!   `PURGE_FORCE` p1 p2   — recursive removal, same override as `REMOVE_FORCE`
//!   ORPHANS          — remove packages no longer required by anything
//!   CLEANCACHE       — remove outdated files from the package cache
//!   VERIFY           — run pkgdb consistency checks
//!   ALTERNATIVE g p  — select pkg p as the provider for group g
//!   ADDREPO url      — add a repository (persisted to a caerus-owned
//!                      xbps.d conf file, never someone else's)
//!   REMOVEREPO url   — remove a repository previously added by ADDREPO
//!   VKPURGE v1 v2    — remove old kernel files/modules for the given
//!                      version(s), via `vkpurge rm` (not an xbps tool —
//!                      the standalone Void kernel-cleanup script)
//!   QUIT             — exit
//!
//! Responses:
//!   LOG <text>       — raw output line from the underlying xbps tool
//!   OK               — current command completed successfully
//!   ERROR <msg>      — current command failed

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

fn assert_root() {
    // SAFETY: getuid() takes no arguments and cannot fail; it is a
    // pure syscall wrapper. This is the one unsafe FFI call in the
    // whole binary (equivalent to the original's getuid() from
    // <unistd.h>), used only for this same-as-before startup check.
    let uid = unsafe { libc_getuid() };
    if uid != 0 {
        eprintln!("caerus-helper: must run as root");
        std::process::exit(1);
    }
}

// Minimal manual declaration instead of pulling in the `libc` crate for
// a single syscall — keeps this privileged binary's dependency graph at
// exactly zero external crates.
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// Runs `argv`, streaming its stdout+stderr back to our own stdout as
/// `LOG <line>` lines (matching the original's combined-pipe behaviour
/// closely enough for a progress log: both streams are forwarded live,
/// each line as soon as it's flushed by the child). Returns the child's
/// exit code, or `None` if it could not even be spawned.
fn run_xbps(argv: &[&str]) -> Option<i32> {
    let mut child = match Command::new(argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            println!("ERROR spawn {}: {}", argv[0], e);
            let _ = io::stdout().flush();
            return None;
        }
    };

    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");

    // Forward both streams concurrently. A channel + two reader threads
    // is simpler and dependency-free compared to replicating the
    // original's dup2()-based fd merge, and is functionally equivalent
    // for our purposes: every line from either stream is relayed live,
    // just not guaranteed to be in exact chronological interleave order
    // relative to each other (irrelevant here — nothing downstream
    // parses ordering between stdout and stderr, only the presence of
    // each line).
    let (tx, rx) = mpsc::channel::<String>();

    let tx_out = tx.clone();
    let out_handle = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx_out.send(line).is_err() {
                break;
            }
        }
    });
    let tx_err = tx;
    let err_handle = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx_err.send(line).is_err() {
                break;
            }
        }
    });

    // rx yields lines until both sender clones (tx_out, tx_err) have
    // been dropped, i.e. once both reader threads finish.
    for line in rx {
        println!("LOG {line}");
        let _ = io::stdout().flush();
    }

    let _ = out_handle.join();
    let _ = err_handle.join();

    child.wait().map_or(None, |status| status.code())
}

/// Splits whitespace-separated package names out of `rest` (everything
/// after the command verb), owned `String`s.
///
/// Every `argv` built from this helper's output puts a `--` right
/// before these names (see the `INSTALL`/`REMOVE`/`PURGE`/`HOLD`/
/// `UNHOLD` handlers below) — package names ultimately come from repo
/// index data, not literal user input, and without `--` a name starting
/// with `-` would be parsed by the underlying xbps tool as one of its
/// own flags (e.g. something resembling `--rootdir=...`) instead of a
/// package name.
fn split_pkgnames(rest: &str) -> Vec<String> {
    rest.split_whitespace().map(str::to_owned).collect()
}

/// Maps a protocol verb (already stripped of its trailing space and
/// package-name argument, e.g. `"PURGE"`) to the base xbps argv it
/// should run, before package names are appended — the exact place a
/// mismatched flag would silently produce the wrong real-world
/// `xbps-remove`/`xbps-install`/`xbps-pkgdb` invocation. Kept as a pure
/// mapping, separate from actually running anything, so it's
/// unit-testable without spawning a privileged process.
fn argv_for(verb: &str) -> Option<&'static [&'static str]> {
    Some(match verb {
        "INSTALL" => &["xbps-install", "-y", "--"],
        "REMOVE" => &["xbps-remove", "-y", "--"],
        "PURGE" => &["xbps-remove", "-y", "-R", "--"],
        "INSTALL_FORCE" => &["xbps-install", "-y", "-I", "--"],
        "REMOVE_FORCE" => &["xbps-remove", "-y", "-F", "--"],
        "PURGE_FORCE" => &["xbps-remove", "-y", "-R", "-F", "--"],
        "HOLD" => &["xbps-pkgdb", "-m", "hold", "--"],
        "UNHOLD" => &["xbps-pkgdb", "-m", "unhold", "--"],
        "REINSTALL" => &["xbps-install", "-f", "-y", "--"],
        "RECONFIGURE" => &["xbps-reconfigure", "-f", "--"],
        "DOWNLOAD" => &["xbps-install", "-D", "-y", "--"],
        "REPOLOCK" => &["xbps-pkgdb", "-m", "repolock", "--"],
        "REPOUNLOCK" => &["xbps-pkgdb", "-m", "repounlock", "--"],
        "MARKAUTO" => &["xbps-pkgdb", "-m", "auto", "--"],
        "MARKMANUAL" => &["xbps-pkgdb", "-m", "manual", "--"],
        _ => return None,
    })
}

/// Runs `verb`'s mapped argv (see `argv_for`) against `pkgs` and
/// responds OK/ERROR — the shared body behind every pkg-name-taking
/// protocol verb below.
fn run_pkg_command(verb: &str, pkgs: &[String], err_msg: &str) {
    let base = argv_for(verb).expect("run_pkg_command called with a known verb");
    let mut argv: Vec<&str> = base.to_vec();
    argv.extend(pkgs.iter().map(String::as_str));
    let code = run_xbps(&argv);
    respond_ok_or(code == Some(0), err_msg);
}

fn respond_ok_or(success: bool, err_msg: &str) {
    if success {
        println!("OK");
    } else {
        println!("ERROR {err_msg}");
    }
    let _ = io::stdout().flush();
}

/// A dedicated xbps.d conf file this helper exclusively owns for
/// repositories added through caerus — ADDREPO/REMOVEREPO only ever
/// touch this one file, never anything a user or another tool set up
/// under /etc/xbps.d/, so there's no risk of clobbering unrelated
/// config or losing track of what caerus itself is responsible for.
const MANAGED_REPO_CONF: &str = "/etc/xbps.d/90-caerus.conf";

/// Rejects control characters before either repo function ever touches
/// the conf file. The GUI's own repo-manager dialog and `Transaction::
/// add_command`'s blanket check already keep these out of anything sent
/// down this protocol today, but this is the one privileged component in
/// the project — it shouldn't rely entirely on a well-behaved caller to
/// keep an embedded newline from smuggling a second, unintended
/// `repository=...` line into a file it writes as root.
fn has_control_char(s: &str) -> bool {
    s.chars().any(char::is_control)
}

fn add_repo(url: &str) -> Result<(), String> {
    if has_control_char(url) {
        return Err("refusing to add a repository URL with control characters".to_string());
    }
    let existing = std::fs::read_to_string(MANAGED_REPO_CONF).unwrap_or_default();
    let line = format!("repository={url}");
    if existing.lines().any(|l| l == line) {
        return Ok(()); // already present, nothing to do
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&line);
    updated.push('\n');
    std::fs::write(MANAGED_REPO_CONF, updated).map_err(|e| e.to_string())
}

fn remove_repo(url: &str) -> Result<(), String> {
    if has_control_char(url) {
        return Err("refusing to remove a repository URL with control characters".to_string());
    }
    let Ok(existing) = std::fs::read_to_string(MANAGED_REPO_CONF) else {
        return Ok(()); // file doesn't exist, nothing to remove
    };
    use std::fmt::Write as _;
    let line = format!("repository={url}");
    let updated: String =
        existing
            .lines()
            .filter(|l| *l != line)
            .fold(String::new(), |mut acc, l| {
                let _ = writeln!(acc, "{l}");
                acc
            });
    std::fs::write(MANAGED_REPO_CONF, updated).map_err(|e| e.to_string())
}

fn main() {
    assert_root();

    println!("READY");
    let _ = io::stdout().flush();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        // EOF/read error — same as the C loop's fgets() failing.
        let Ok(line) = line else { break };
        let line = line.trim_end();

        if line == "QUIT" {
            println!("OK");
            let _ = io::stdout().flush();
            break;
        }

        if line == "SYNC" {
            let code = run_xbps(&["xbps-install", "-S"]);
            respond_ok_or(code == Some(0), "sync failed");
            continue;
        }

        if line == "UPGRADE" {
            let code = run_xbps(&["xbps-install", "-y", "-Su"]);
            respond_ok_or(code == Some(0), "upgrade failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("INSTALL ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("INSTALL", &pkgs, "install failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REMOVE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("REMOVE", &pkgs, "remove failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("PURGE ") {
            // xbps has no dpkg-style "purge config files" concept; -R
            // recursively drops packages that become orphaned as a
            // result of this removal, the closest equivalent — same
            // rationale as the original C helper.
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("PURGE", &pkgs, "purge failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("INSTALL_FORCE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            // -I: ignore detected file conflicts — a fallback for when a
            // plain INSTALL failed because of one, not offered up front.
            run_pkg_command("INSTALL_FORCE", &pkgs, "forced install failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REMOVE_FORCE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            // -F: force removal even with unresolved revdeps/shared
            // libraries — a fallback for when a plain REMOVE failed.
            run_pkg_command("REMOVE_FORCE", &pkgs, "forced remove failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("PURGE_FORCE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("PURGE_FORCE", &pkgs, "forced purge failed");
            continue;
        }

        if line == "ORPHANS" {
            // -o computes the orphan set itself; no package names needed.
            let code = run_xbps(&["xbps-remove", "-y", "-o"]);
            respond_ok_or(code == Some(0), "orphan removal failed");
            continue;
        }

        if line == "CLEANCACHE" {
            // Single -O: drop only cache files superseded by a newer
            // version. (Doubling it would also drop cached files for
            // packages that aren't installed at all — not done here to
            // keep this action's effect predictable/non-destructive.)
            let code = run_xbps(&["xbps-remove", "-O"]);
            respond_ok_or(code == Some(0), "cache cleanup failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("HOLD ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("HOLD", &pkgs, "hold failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("UNHOLD ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("UNHOLD", &pkgs, "unhold failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REINSTALL ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            // -f forces re-installation of a package xbps otherwise
            // considers already up to date and does nothing for.
            run_pkg_command("REINSTALL", &pkgs, "reinstall failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("RECONFIGURE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            // -f forces the reconfigure scripts to actually re-run for a
            // package xbps otherwise considers already configured.
            // xbps-reconfigure has no -y/--yes — it never prompts.
            run_pkg_command("RECONFIGURE", &pkgs, "reconfigure failed");
            continue;
        }

        if line == "RECONFIGURE_ALL" {
            // -f forces every package to be reconfigured even if xbps
            // considers it already configured; -a means "every installed
            // package" rather than a specific list — the system-wide
            // counterpart to the per-package RECONFIGURE above.
            let code = run_xbps(&["xbps-reconfigure", "-f", "-a"]);
            respond_ok_or(code == Some(0), "reconfigure-all failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("VKPURGE ") {
            let versions = split_pkgnames(rest);
            if versions.is_empty() {
                println!("ERROR no kernel versions specified");
                let _ = io::stdout().flush();
                continue;
            }
            // Not an xbps tool — `vkpurge` re-validates each version
            // against its own removable-kernel list before touching
            // anything, so passing exactly what our own prior `vkpurge
            // list` produced (see caerus/src/ui/vkpurge_dialog.rs, an
            // unprivileged read run directly from the GUI) is safe.
            let mut argv: Vec<&str> = vec!["vkpurge", "rm"];
            argv.extend(versions.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "kernel purge failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("DOWNLOAD ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("DOWNLOAD", &pkgs, "download failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REPOLOCK ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("REPOLOCK", &pkgs, "repo-lock failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REPOUNLOCK ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("REPOUNLOCK", &pkgs, "repo-unlock failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("MARKAUTO ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("MARKAUTO", &pkgs, "marking automatic failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("MARKMANUAL ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            run_pkg_command("MARKMANUAL", &pkgs, "marking manual failed");
            continue;
        }

        if line == "VERIFY" {
            let code = run_xbps(&[
                "xbps-pkgdb",
                "-a",
                "--checks",
                "files,dependencies,alternatives,pkgdb",
            ]);
            respond_ok_or(code == Some(0), "verification failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("ALTERNATIVE ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() != 2 {
                println!("ERROR expected: ALTERNATIVE <group> <pkgname>");
                let _ = io::stdout().flush();
                continue;
            }
            let code = run_xbps(&["xbps-alternatives", "-g", parts[0], "-s", parts[1]]);
            respond_ok_or(code == Some(0), "setting alternative failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("ADDREPO ") {
            let url = rest.trim();
            if url.is_empty() {
                println!("ERROR no url specified");
                let _ = io::stdout().flush();
                continue;
            }
            match add_repo(url) {
                Ok(()) => respond_ok_or(true, ""),
                Err(e) => respond_ok_or(false, &e),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("REMOVEREPO ") {
            let url = rest.trim();
            if url.is_empty() {
                println!("ERROR no url specified");
                let _ = io::stdout().flush();
                continue;
            }
            match remove_repo(url) {
                Ok(()) => respond_ok_or(true, ""),
                Err(e) => respond_ok_or(false, &e),
            }
            continue;
        }

        println!("ERROR unknown command: {line}");
        let _ = io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_uses_recursive_removal_flag() {
        assert_eq!(
            argv_for("PURGE"),
            Some(["xbps-remove", "-y", "-R", "--"].as_slice())
        );
    }

    #[test]
    fn purge_force_combines_recursive_and_force_flags() {
        assert_eq!(
            argv_for("PURGE_FORCE"),
            Some(["xbps-remove", "-y", "-R", "-F", "--"].as_slice())
        );
    }

    #[test]
    fn remove_vs_remove_force() {
        assert_eq!(
            argv_for("REMOVE"),
            Some(["xbps-remove", "-y", "--"].as_slice())
        );
        assert_eq!(
            argv_for("REMOVE_FORCE"),
            Some(["xbps-remove", "-y", "-F", "--"].as_slice())
        );
    }

    #[test]
    fn install_vs_install_force() {
        assert_eq!(
            argv_for("INSTALL"),
            Some(["xbps-install", "-y", "--"].as_slice())
        );
        assert_eq!(
            argv_for("INSTALL_FORCE"),
            Some(["xbps-install", "-y", "-I", "--"].as_slice())
        );
    }

    #[test]
    fn hold_and_unhold_are_distinct_pkgdb_modes() {
        assert_eq!(
            argv_for("HOLD"),
            Some(["xbps-pkgdb", "-m", "hold", "--"].as_slice())
        );
        assert_eq!(
            argv_for("UNHOLD"),
            Some(["xbps-pkgdb", "-m", "unhold", "--"].as_slice())
        );
    }

    #[test]
    fn repolock_and_repounlock_are_distinct_pkgdb_modes() {
        assert_eq!(
            argv_for("REPOLOCK"),
            Some(["xbps-pkgdb", "-m", "repolock", "--"].as_slice())
        );
        assert_eq!(
            argv_for("REPOUNLOCK"),
            Some(["xbps-pkgdb", "-m", "repounlock", "--"].as_slice())
        );
    }

    #[test]
    fn markauto_and_markmanual_are_distinct_pkgdb_modes() {
        assert_eq!(
            argv_for("MARKAUTO"),
            Some(["xbps-pkgdb", "-m", "auto", "--"].as_slice())
        );
        assert_eq!(
            argv_for("MARKMANUAL"),
            Some(["xbps-pkgdb", "-m", "manual", "--"].as_slice())
        );
    }

    #[test]
    fn reinstall_forces_reinstallation() {
        assert_eq!(
            argv_for("REINSTALL"),
            Some(["xbps-install", "-f", "-y", "--"].as_slice())
        );
    }

    #[test]
    fn reconfigure_forces_reconfiguration() {
        assert_eq!(
            argv_for("RECONFIGURE"),
            Some(["xbps-reconfigure", "-f", "--"].as_slice())
        );
    }

    #[test]
    fn download_does_not_pass_yes_alone_but_fetch_flag() {
        assert_eq!(
            argv_for("DOWNLOAD"),
            Some(["xbps-install", "-D", "-y", "--"].as_slice())
        );
    }

    #[test]
    fn unknown_verb_has_no_mapping() {
        assert_eq!(argv_for("NOT_A_REAL_VERB"), None);
    }

    #[test]
    fn split_pkgnames_splits_on_whitespace() {
        assert_eq!(
            split_pkgnames("foo bar baz"),
            vec!["foo".to_string(), "bar".to_string(), "baz".to_string()]
        );
        assert_eq!(split_pkgnames(""), Vec::<String>::new());
        assert_eq!(split_pkgnames("  foo   bar  "), vec!["foo", "bar"]);
    }

    #[test]
    fn control_chars_detected() {
        assert!(has_control_char("http://evil\nrepository=http://also-evil"));
        assert!(has_control_char("http://example.org/\r"));
        assert!(!has_control_char(
            "https://repo-default.voidlinux.org/current"
        ));
        assert!(!has_control_char(""));
    }
}
