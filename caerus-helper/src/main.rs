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
//!   DOWNLOAD p1 p2   — fetch and verify package(s), don't install
//!   REPOLOCK p1 p2   — only ever upgrade from the currently-installed repo
//!   REPOUNLOCK p1 p2 — release a previously-set repo-lock
//!   MARKAUTO p1 p2   — mark package(s) as automatically installed
//!   MARKMANUAL p1 p2 — mark package(s) as explicitly/manually installed
//!   INSTALL_FORCE p1 p2 — install, ignoring detected file conflicts
//!   REMOVE_FORCE p1 p2  — remove despite unresolved revdeps/shared libs
//!   PURGE_FORCE p1 p2   — recursive removal, same override as REMOVE_FORCE
//!   ORPHANS          — remove packages no longer required by anything
//!   CLEANCACHE       — remove outdated files from the package cache
//!   VERIFY           — run pkgdb consistency checks
//!   ALTERNATIVE g p  — select pkg p as the provider for group g
//!   ADDREPO url      — add a repository (persisted to a caerus-owned
//!                      xbps.d conf file, never someone else's)
//!   REMOVEREPO url   — remove a repository previously added by ADDREPO
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
        println!("LOG {}", line);
        let _ = io::stdout().flush();
    }

    let _ = out_handle.join();
    let _ = err_handle.join();

    match child.wait() {
        Ok(status) => status.code(),
        Err(_) => None,
    }
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

fn respond_ok_or(success: bool, err_msg: &str) {
    if success {
        println!("OK");
    } else {
        println!("ERROR {}", err_msg);
    }
    let _ = io::stdout().flush();
}

/// A dedicated xbps.d conf file this helper exclusively owns for
/// repositories added through caerus — ADDREPO/REMOVEREPO only ever
/// touch this one file, never anything a user or another tool set up
/// under /etc/xbps.d/, so there's no risk of clobbering unrelated
/// config or losing track of what caerus itself is responsible for.
const MANAGED_REPO_CONF: &str = "/etc/xbps.d/90-caerus.conf";

fn add_repo(url: &str) -> Result<(), String> {
    let existing = std::fs::read_to_string(MANAGED_REPO_CONF).unwrap_or_default();
    let line = format!("repository={}", url);
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
    let Ok(existing) = std::fs::read_to_string(MANAGED_REPO_CONF) else {
        return Ok(()); // file doesn't exist, nothing to remove
    };
    let line = format!("repository={}", url);
    let updated: String = existing
        .lines()
        .filter(|l| *l != line)
        .map(|l| format!("{}\n", l))
        .collect();
    std::fs::write(MANAGED_REPO_CONF, updated).map_err(|e| e.to_string())
}

fn main() {
    assert_root();

    println!("READY");
    let _ = io::stdout().flush();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // EOF/read error — same as the C loop's fgets() failing
        };
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
            let mut argv: Vec<&str> = vec!["xbps-install", "-y", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "install failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REMOVE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-remove", "-y", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "remove failed");
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
            let mut argv: Vec<&str> = vec!["xbps-remove", "-y", "-R", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "purge failed");
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
            let mut argv: Vec<&str> = vec!["xbps-install", "-y", "-I", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "forced install failed");
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
            let mut argv: Vec<&str> = vec!["xbps-remove", "-y", "-F", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "forced remove failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("PURGE_FORCE ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-remove", "-y", "-R", "-F", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "forced purge failed");
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
            let mut argv: Vec<&str> = vec!["xbps-pkgdb", "-m", "hold", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "hold failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("UNHOLD ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-pkgdb", "-m", "unhold", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "unhold failed");
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
            let mut argv: Vec<&str> = vec!["xbps-install", "-f", "-y", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "reinstall failed");
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
            let mut argv: Vec<&str> = vec!["xbps-reconfigure", "-f", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "reconfigure failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("DOWNLOAD ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-install", "-D", "-y", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "download failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REPOLOCK ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-pkgdb", "-m", "repolock", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "repo-lock failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("REPOUNLOCK ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-pkgdb", "-m", "repounlock", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "repo-unlock failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("MARKAUTO ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-pkgdb", "-m", "auto", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "marking automatic failed");
            continue;
        }

        if let Some(rest) = line.strip_prefix("MARKMANUAL ") {
            let pkgs = split_pkgnames(rest);
            if pkgs.is_empty() {
                println!("ERROR no packages specified");
                let _ = io::stdout().flush();
                continue;
            }
            let mut argv: Vec<&str> = vec!["xbps-pkgdb", "-m", "manual", "--"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "marking manual failed");
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

        println!("ERROR unknown command: {}", line);
        let _ = io::stdout().flush();
    }
}
