//! caerus-helper — transaction executor, spawned by caerus via `pkexec`.
//!
//! caerus itself runs entirely unprivileged; only this helper is ever
//! elevated. No GTK, no libxbps FFI, and (see Cargo.toml) no
//! dependencies at all — kept as small and auditable as possible.
//!
//! Protocol (line-oriented stdin/stdout):
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
//!   REMOVEREPO url   — strip the repository's line from /etc/xbps.d
//!   ENABLEREPO url   — uncomment a disabled repository in /etc/xbps.d
//!   DISABLEREPO url  — comment out a repository (vendor repos get a
//!                      same-name /etc/xbps.d override instead)
//!   VKPURGE v1 v2    — remove old kernel files/modules for the given
//!                      version(s), via `vkpurge rm` (not an xbps tool —
//!                      the standalone Void kernel-cleanup script)
//!   QUIT             — exit
//!
//! Responses:
//!   LOG <text>       — raw output line from the underlying xbps tool
//!   OK               — current command completed successfully
//!   ERROR <msg>      — current command failed

use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

fn assert_root() {
    // SAFETY: getuid() takes no arguments and cannot fail.
    let uid = unsafe { libc_getuid() };
    if uid != 0 {
        eprintln!("caerus-helper: must run as root");
        std::process::exit(1);
    }
}

// Minimal manual declarations instead of pulling in the `libc` crate for
// two syscalls — keeps this privileged binary's dependency graph at
// exactly zero external crates.
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

const LOCK_EX: i32 = 2;

/// Runs `argv`, streaming its stdout+stderr back to our own stdout as
/// `LOG <line>` lines. Returns the child's exit code, or `None` if it
/// could not be spawned.
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

    // Forward both streams concurrently via a channel + two reader
    // threads; interleave order between stdout/stderr isn't guaranteed,
    // which is fine since nothing downstream parses it.
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

/// Splits whitespace-separated package names out of `rest`.
///
/// Every argv built from these names includes a `--` right before them,
/// so a name starting with `-` isn't parsed as an xbps flag.
fn split_pkgnames(rest: &str) -> Vec<String> {
    rest.split_whitespace().map(str::to_owned).collect()
}

/// Maps a protocol verb to the base xbps argv it should run, before
/// package names are appended. Kept as a pure mapping so it's
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
/// responds OK/ERROR.
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

/// ADDREPO only appends to this caerus-owned file. REMOVEREPO/
/// ENABLEREPO/DISABLEREPO operate on any `/etc/xbps.d/*.conf`, only ever
/// touching exact `repository=<url>` lines. Vendor files under
/// /usr/share/xbps.d are never edited; a vendor repo is disabled via a
/// same-name override in /etc/xbps.d (the xbps.d(5) shadowing rule).
const MANAGED_REPO_CONF: &str = "/etc/xbps.d/90-caerus.conf";
const ETC_XBPS_D: &str = "/etc/xbps.d";
const VENDOR_XBPS_D: &str = "/usr/share/xbps.d";

/// Rejects control characters before either repo function touches the
/// conf file — this is the privileged component, so it shouldn't rely
/// entirely on the caller to keep a newline from smuggling a second
/// `repository=...` line into a file it writes as root.
fn has_control_char(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// Opens `path` read/write (creating it if needed) and holds an
/// exclusive `flock` for the lifetime of the returned `File` — the lock
/// releases on close/drop. Serializes concurrent caerus-helper instances
/// against a read-then-write race on the same conf file.
fn open_locked(path: &std::path::Path) -> Result<std::fs::File, String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false) // caller reads existing content before any write
        .open(path)
        .map_err(|e| e.to_string())?;
    // SAFETY: fd is a valid, open file descriptor for the file's lifetime.
    if unsafe { libc_flock(file.as_raw_fd(), LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(file)
}

fn add_repo(url: &str) -> Result<(), String> {
    if has_control_char(url) {
        return Err("refusing to add a repository URL with control characters".to_string());
    }
    let mut file = open_locked(std::path::Path::new(MANAGED_REPO_CONF))?;
    let mut existing = String::new();
    file.read_to_string(&mut existing)
        .map_err(|e| e.to_string())?;
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
    file.set_len(0).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.write_all(updated.as_bytes())
        .map_err(|e| e.to_string())
}

// Hidden dotfiles are skipped — xbps itself ignores them.
fn conf_paths(dir: &str) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().is_some_and(|e| e == "conf")
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| !n.starts_with('.'))
                })
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

/// Opens an already-existing file read/write, locked (see
/// `open_locked`), or `None` if it can't be opened — used where a
/// missing file is a normal "nothing to do" case, not an error.
fn open_locked_existing(path: &std::path::Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    // SAFETY: fd is a valid, open file descriptor for the file's lifetime.
    (unsafe { libc_flock(file.as_raw_fd(), LOCK_EX) } == 0).then_some(file)
}

/// Rewrites `line` occurrences in the file via `map`; Ok(true) if the
/// file contained the line and was rewritten.
fn rewrite_conf(
    path: &std::path::Path,
    map: impl Fn(&str) -> Option<String>,
) -> Result<bool, String> {
    use std::fmt::Write as _;
    let Some(mut file) = open_locked_existing(path) else {
        return Ok(false);
    };
    let mut existing = String::new();
    file.read_to_string(&mut existing)
        .map_err(|e| e.to_string())?;
    let mut hit = false;
    let mut updated = String::new();
    for l in existing.lines() {
        match map(l) {
            Some(replacement) => {
                hit = true;
                if !replacement.is_empty() {
                    let _ = writeln!(updated, "{replacement}");
                }
            }
            None => {
                let _ = writeln!(updated, "{l}");
            }
        }
    }
    if hit {
        file.set_len(0).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        file.write_all(updated.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Ok(hit)
}

fn toggle_repo(url: &str, enable: bool) -> Result<(), String> {
    use std::fmt::Write as _;

    if has_control_char(url) {
        return Err("refusing to toggle a repository URL with control characters".to_string());
    }
    let active = format!("repository={url}");
    let disabled = format!("#{active}");
    let (from, to) = if enable {
        (&disabled, &active)
    } else {
        (&active, &disabled)
    };

    for path in conf_paths(ETC_XBPS_D) {
        if rewrite_conf(&path, |l| (l == *from).then(|| to.clone()))? {
            return Ok(());
        }
    }
    if enable {
        return Ok(());
    }

    // Not in /etc — disable a vendor repo by shadowing its file with an
    // /etc copy that has the line commented.
    for path in conf_paths(VENDOR_XBPS_D) {
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !contents.lines().any(|l| l == active) {
            continue;
        }
        let mut copy = String::new();
        for l in contents.lines() {
            let _ = writeln!(copy, "{}", if l == active { &disabled } else { l });
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let target = std::path::Path::new(ETC_XBPS_D).join(name);
        let mut file = open_locked(&target)?;
        file.set_len(0).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        return file.write_all(copy.as_bytes()).map_err(|e| e.to_string());
    }
    Ok(())
}

/// Strips the repo's line (active or disabled) from every /etc/xbps.d
/// conf. Vendor files are left alone — vendor repos can't be removed,
/// only disabled.
fn remove_repo(url: &str) -> Result<(), String> {
    if has_control_char(url) {
        return Err("refusing to remove a repository URL with control characters".to_string());
    }
    let active = format!("repository={url}");
    let disabled = format!("#{active}");
    for path in conf_paths(ETC_XBPS_D) {
        rewrite_conf(&path, |l| (l == active || l == disabled).then(String::new))?;
    }
    Ok(())
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
            // xbps-install -Su updates only xbps first and exits EBUSY
            // (16) if it self-updated, expecting a re-run — see
            // xbps-install(1). Do that re-run automatically.
            const EBUSY: i32 = 16;
            let mut code = run_xbps(&["xbps-install", "-y", "-Su"]);
            if code == Some(EBUSY) {
                println!("LOG xbps updated itself; re-running the system upgrade\u{2026}");
                let _ = io::stdout().flush();
                code = run_xbps(&["xbps-install", "-y", "-Su"]);
            }
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
            // xbps has no dpkg-style "purge config files"; -R recursively
            // drops packages orphaned by this removal instead.
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
            // -I: ignore detected file conflicts.
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
            // -F: force removal even with unresolved revdeps/shared libs.
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
            // version (doubling it would also drop files for
            // not-installed packages — kept non-destructive).
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
            // -f forces reinstall of a package xbps considers up to date.
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
            // -f forces re-run; xbps-reconfigure has no -y — never prompts.
            run_pkg_command("RECONFIGURE", &pkgs, "reconfigure failed");
            continue;
        }

        if line == "RECONFIGURE_ALL" {
            // -f forces re-run; -a means every installed package.
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
            if versions.iter().any(|v| v.starts_with('-')) {
                println!("ERROR kernel version must not start with '-'");
                let _ = io::stdout().flush();
                continue;
            }
            // Not an xbps tool — `vkpurge` re-validates each version
            // against its own removable-kernel list before touching
            // anything.
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
            if parts.iter().any(|p| p.starts_with('-')) {
                println!("ERROR group/pkgname must not start with '-'");
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

        if let Some(rest) = line.strip_prefix("ENABLEREPO ") {
            let url = rest.trim();
            if url.is_empty() {
                println!("ERROR no url specified");
                let _ = io::stdout().flush();
                continue;
            }
            match toggle_repo(url, true) {
                Ok(()) => respond_ok_or(true, ""),
                Err(e) => respond_ok_or(false, &e),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("DISABLEREPO ") {
            let url = rest.trim();
            if url.is_empty() {
                println!("ERROR no url specified");
                let _ = io::stdout().flush();
                continue;
            }
            match toggle_repo(url, false) {
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
