use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

fn assert_root() {
    let uid = unsafe { libc_getuid() };
    if uid != 0 {
        eprintln!("caerus-helper: must run as root");
        std::process::exit(1);
    }
}

extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

const LOCK_EX: i32 = 2;

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
            eprintln!("caerus-helper: spawn {}: {}", argv[0], e);
            return None;
        }
    };

    let Some(stdout) = child.stdout.take() else {
        eprintln!("caerus-helper: child stdout was not piped");
        return None;
    };
    let Some(stderr) = child.stderr.take() else {
        eprintln!("caerus-helper: child stderr was not piped");
        return None;
    };

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

    for line in rx {
        println!("LOG {line}");
        let _ = io::stdout().flush();
    }

    let _ = out_handle.join();
    let _ = err_handle.join();

    child.wait().map_or(None, |status| status.code())
}

fn split_pkgnames(rest: &str) -> Vec<String> {
    rest.split_whitespace().map(str::to_owned).collect()
}

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

fn run_pkg_command(verb: &str, pkgs: &[String], err_msg: &str) {
    let Some(base) = argv_for(verb) else {
        eprintln!("caerus-helper: unknown verb: {verb}");
        respond_ok_or(false, err_msg);
        return;
    };
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

const MANAGED_REPO_CONF: &str = "/etc/xbps.d/90-caerus.conf";
const ETC_XBPS_D: &str = "/etc/xbps.d";
const VENDOR_XBPS_D: &str = "/usr/share/xbps.d";

fn has_control_char(s: &str) -> bool {
    s.chars().any(char::is_control)
}

fn open_locked(path: &std::path::Path) -> Result<std::fs::File, String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    if unsafe { libc_flock(file.as_raw_fd(), LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(file)
}

fn valid_repo_url(url: &str) -> bool {
    !url.chars().any(|c| c.is_control() || c.is_whitespace())
        && ["https://", "http://", "ftp://", "file://", "/"]
            .iter()
            .any(|scheme| url.starts_with(scheme))
}

fn add_repo(url: &str) -> Result<(), String> {
    if has_control_char(url) {
        return Err("refusing to add a repository URL with control characters".to_string());
    }
    if !valid_repo_url(url) {
        return Err("repository must be an http(s)/ftp/file URL or an absolute path".to_string());
    }
    let mut file = open_locked(std::path::Path::new(MANAGED_REPO_CONF))?;
    let mut existing = String::new();
    file.read_to_string(&mut existing)
        .map_err(|e| e.to_string())?;
    let line = format!("repository={url}");
    if existing.lines().any(|l| l == line) {
        return Ok(());
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

fn open_locked_existing(path: &std::path::Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    (unsafe { libc_flock(file.as_raw_fd(), LOCK_EX) } == 0).then_some(file)
}

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

const PKG_VERBS: &[(&str, &str)] = &[
    ("INSTALL", "install failed"),
    ("REMOVE", "remove failed"),
    ("PURGE", "purge failed"),
    ("INSTALL_FORCE", "forced install failed"),
    ("REMOVE_FORCE", "forced remove failed"),
    ("PURGE_FORCE", "forced purge failed"),
    ("HOLD", "hold failed"),
    ("UNHOLD", "unhold failed"),
    ("REINSTALL", "reinstall failed"),
    ("RECONFIGURE", "reconfigure failed"),
    ("DOWNLOAD", "download failed"),
    ("REPOLOCK", "repo-lock failed"),
    ("REPOUNLOCK", "repo-unlock failed"),
    ("MARKAUTO", "marking automatic failed"),
    ("MARKMANUAL", "marking manual failed"),
];

const FIXED_VERBS: &[(&str, &[&str], &str)] = &[
    ("SYNC", &["xbps-install", "-S"], "sync failed"),
    (
        "ORPHANS",
        &["xbps-remove", "-y", "-o"],
        "orphan removal failed",
    ),
    ("CLEANCACHE", &["xbps-remove", "-O"], "cache cleanup failed"),
    (
        "RECONFIGURE_ALL",
        &["xbps-reconfigure", "-f", "-a"],
        "reconfigure-all failed",
    ),
    (
        "VERIFY",
        &[
            "xbps-pkgdb",
            "-a",
            "--checks",
            "files,dependencies,alternatives,pkgdb",
        ],
        "verification failed",
    ),
];

fn reply(line: &str) {
    println!("{line}");
    let _ = io::stdout().flush();
}

fn respond_result(result: Result<(), String>) {
    match result {
        Ok(()) => respond_ok_or(true, ""),
        Err(e) => respond_ok_or(false, &e),
    }
}

fn handle_upgrade() {
    const EBUSY: i32 = 16;
    let mut code = run_xbps(&["xbps-install", "-y", "-Su"]);
    if code == Some(EBUSY) {
        println!("LOG xbps updated itself; re-running the system upgrade\u{2026}");
        let _ = io::stdout().flush();
        code = run_xbps(&["xbps-install", "-y", "-Su"]);
    }
    respond_ok_or(code == Some(0), "upgrade failed");
}

fn handle_vkpurge(rest: &str) {
    let versions = split_pkgnames(rest);
    if versions.is_empty() {
        reply("ERROR no kernel versions specified");
    } else if versions.iter().any(|v| v.starts_with('-')) {
        reply("ERROR kernel version must not start with '-'");
    } else {
        let mut argv: Vec<&str> = vec!["vkpurge", "rm"];
        argv.extend(versions.iter().map(String::as_str));
        let code = run_xbps(&argv);
        respond_ok_or(code == Some(0), "kernel purge failed");
    }
}

fn handle_alternative(rest: &str) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 2 {
        reply("ERROR expected: ALTERNATIVE <group> <pkgname>");
    } else if parts.iter().any(|p| p.starts_with('-')) {
        reply("ERROR group/pkgname must not start with '-'");
    } else {
        let code = run_xbps(&["xbps-alternatives", "-g", parts[0], "-s", parts[1]]);
        respond_ok_or(code == Some(0), "setting alternative failed");
    }
}

fn handle_repo(verb: &str, url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() {
        reply("ERROR no url specified");
        return true;
    }
    match verb {
        "ADDREPO" => respond_result(add_repo(url)),
        "REMOVEREPO" => respond_result(remove_repo(url)),
        "ENABLEREPO" => respond_result(toggle_repo(url, true)),
        "DISABLEREPO" => respond_result(toggle_repo(url, false)),
        _ => return false,
    }
    true
}

fn handle_line(line: &str) {
    if line == "UPGRADE" {
        handle_upgrade();
        return;
    }
    if let Some((_, argv, err)) = FIXED_VERBS.iter().find(|(v, _, _)| *v == line) {
        respond_ok_or(run_xbps(argv) == Some(0), err);
        return;
    }
    let Some((verb, rest)) = line.split_once(' ') else {
        reply(&format!("ERROR unknown command: {line}"));
        return;
    };
    if let Some((_, err)) = PKG_VERBS.iter().find(|(v, _)| *v == verb) {
        let pkgs = split_pkgnames(rest);
        if pkgs.is_empty() {
            reply("ERROR no packages specified");
        } else {
            run_pkg_command(verb, &pkgs, err);
        }
        return;
    }
    match verb {
        "VKPURGE" => handle_vkpurge(rest),
        "ALTERNATIVE" => handle_alternative(rest),
        _ if handle_repo(verb, rest) => {}
        _ => reply(&format!("ERROR unknown command: {line}")),
    }
}

fn main() {
    assert_root();

    reply("READY");

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim_end();

        if line == "QUIT" {
            reply("OK");
            break;
        }
        handle_line(line);
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
    fn repo_urls_are_validated() {
        assert!(valid_repo_url("https://repo-default.voidlinux.org/current"));
        assert!(valid_repo_url("/var/cache/local-repo"));
        assert!(!valid_repo_url("javascript:alert(1)"));
        assert!(!valid_repo_url("https://a b"));
        assert!(!valid_repo_url(""));
    }

    #[test]
    fn every_pkg_verb_has_an_argv() {
        for (verb, _) in PKG_VERBS {
            assert!(argv_for(verb).is_some(), "{verb}");
        }
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
