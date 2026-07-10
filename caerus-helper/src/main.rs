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
            let mut argv: Vec<&str> = vec!["xbps-install", "-y"];
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
            let mut argv: Vec<&str> = vec!["xbps-remove", "-y"];
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
            let mut argv: Vec<&str> = vec!["xbps-remove", "-y", "-R"];
            argv.extend(pkgs.iter().map(String::as_str));
            let code = run_xbps(&argv);
            respond_ok_or(code == Some(0), "purge failed");
            continue;
        }

        println!("ERROR unknown command: {}", line);
        let _ = io::stdout().flush();
    }
}
