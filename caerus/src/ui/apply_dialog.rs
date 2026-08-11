//! Shows a modal progress dialog and runs `commands` on `session` — an
//! existing, caller-owned `Transaction`. The dialog doesn't own
//! `session`; it stays alive after the dialog closes so the next call
//! reuses it without re-authenticating.

use crate::backend::transaction::{DisconnectReason, Transaction};
use crate::ui::dialog_util::modal_window;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    static AUTO_CLOSE_ON_SUCCESS: Cell<bool> = const { Cell::new(false) };
}

pub fn set_auto_close_on_success(enabled: bool) {
    AUTO_CLOSE_ON_SUCCESS.set(enabled);
}

/// How many packages this batch actually names, summed across every
/// package-name-taking command, for the "package N of M" counter.
fn count_target_packages(commands: &[String]) -> usize {
    commands
        .iter()
        .filter(|c| {
            for verb in ["INSTALL ", "REMOVE ", "PURGE ", "HOLD ", "UNHOLD "] {
                if c.starts_with(verb) {
                    return true;
                }
            }
            false
        })
        .map(|c| c.split_whitespace().skip(1).count())
        .sum()
}

/// The helper's own log lines look like:
///   LOG [*] Downloading packages
///   LOG foo-1.0_1: [*****     ] 42% ETA: 00:03
///   LOG foo-1.0_1: unpacking ...
///   OK
/// "LOG " and "[*]" are stripped for display only; the underlying
/// protocol line is untouched everywhere else.
fn strip_log_decoration(line: &str) -> String {
    let mut s = line;
    if let Some(rest) = s.strip_prefix("LOG ") {
        s = rest;
    }
    if let Some(rest) = s.strip_prefix("[*] ") {
        s = rest;
    } else if let Some(rest) = s.strip_prefix("[*]") {
        s = rest;
    }
    s.trim().to_string()
}

/// Extracts the leading `<pkgver>` identifier from an already
/// decoration-stripped xbps status line, e.g. `foo-1.0_1: unpacking
/// ...` -> `Some("foo-1.0_1")`. Returns `None` for banner lines that
/// don't name a specific package.
///
/// xbps-install emits several differently-worded lines per package over
/// its lifecycle; counting distinct raw lines instead of distinct
/// pkgver prefixes overcounts and makes "Package N of M" stall early.
fn extract_pkgver(line: &str) -> Option<&str> {
    let idx = line.find(": ")?;
    let candidate = &line[..idx];
    if candidate.is_empty() || candidate.contains(char::is_whitespace) {
        return None;
    }
    Some(candidate)
}

/// The helper emits one line per percentage tick while a package
/// downloads/installs/verifies (xbps-install format: `%s: [%s %d%%] %s
/// ETA: %s`). Returns the last `<digits>%` run in the line.
fn extract_percentage(line: &str) -> Option<u8> {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate().rev() {
        if b != b'%' {
            continue;
        }
        let mut start = i;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == i {
            continue; // '%' with no digits immediately before it
        }
        if let Ok(pct) = line[start..i].parse::<u32>() {
            return Some(pct.min(100) as u8);
        }
    }
    None
}

pub fn run(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    done_cb: impl Fn(bool) + 'static,
) {
    let (dlg, outer) = modal_window(title, parent, true, (520, 200), 8);

    let total_pkgs = count_target_packages(commands);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spinner = gtk::Spinner::new();
    spinner.start();
    let status_label = gtk::Label::new(Some("Applying changes\u{2026}"));
    status_label.set_xalign(0.0);
    status_label.set_hexpand(true);
    header.append(&spinner);
    header.append(&status_label);
    outer.append(&header);

    let action_label = gtk::Label::new(Some("\u{2026}"));
    action_label.set_xalign(0.0);
    action_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    action_label.add_css_class("dim-label");
    outer.append(&action_label);

    // GtkProgressBar's own `show-text` lays the label as a sibling of
    // the trough, not on top of it — a real `GtkOverlay` is needed to
    // get text genuinely inside the bar.
    let progress_bar = gtk::ProgressBar::new();
    progress_bar.add_css_class("apply-progress");
    let bar_text_label = gtk::Label::new(None);
    bar_text_label.add_css_class("apply-progress-text");
    bar_text_label.set_halign(gtk::Align::Center);
    bar_text_label.set_valign(gtk::Align::Center);
    let progress_overlay = gtk::Overlay::new();
    progress_overlay.set_child(Some(&progress_bar));
    progress_overlay.add_overlay(&bar_text_label);
    outer.append(&progress_overlay);

    let expander = gtk::Expander::new(Some("Details"));
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_size_request(-1, 220);
    scroll.set_vexpand(true);
    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_monospace(true);
    text_view.set_wrap_mode(gtk::WrapMode::WordChar);
    text_view.set_margin_start(4);
    text_view.set_margin_end(4);
    scroll.set_child(Some(&text_view));
    expander.set_child(Some(&scroll));
    outer.append(&expander);

    let close_btn = gtk::Button::with_label("Close");
    close_btn.set_halign(gtk::Align::End);
    close_btn.set_sensitive(false);
    outer.append(&close_btn);

    // Indeterminate pulse until `append_log` sees a real percentage tick
    // (some commands like SYNC/HOLD/ORPHANS never report one).
    let pulsing = Rc::new(Cell::new(true));
    {
        let pulsing = pulsing.clone();
        let progress_bar = progress_bar.clone();
        glib::source::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if !pulsing.get() {
                return glib::ControlFlow::Break;
            }
            progress_bar.pulse();
            glib::ControlFlow::Continue
        });
    }

    // Package counter: counts transitions between distinct `pkgver`
    // prefixes (see `extract_pkgver`), capped at `total_pkgs`.
    let seen_count = Rc::new(Cell::new(0usize));
    let last_pkgver: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let last_pct: Rc<Cell<Option<u8>>> = Rc::new(Cell::new(None));
    // A multi-command batch can repeat the exact same status line back
    // to back; suppress the repeat instead of showing it twice.
    let last_logged_line: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    let set_bar_text = {
        let bar_text_label = bar_text_label.clone();
        let seen_count = seen_count.clone();
        let last_pct = last_pct.clone();
        move || {
            let n = seen_count.get();
            let text = match (total_pkgs > 1 && n > 0, last_pct.get()) {
                (true, Some(p)) => format!("Package {n} of {total_pkgs} — {p}%"),
                (true, None) => format!("Package {n} of {total_pkgs}"),
                (false, Some(p)) => format!("{p}%"),
                (false, None) => String::new(),
            };
            bar_text_label.set_text(&text);
        }
    };

    // Styled log rendering: errors red, per-package completions green,
    // phase banners bold, protocol chatter dimmed.
    let buf = text_view.buffer();
    let tag_error = gtk::TextTag::builder().foreground("#ed333b").build();
    let tag_success = gtk::TextTag::builder().foreground("#2ec27e").build();
    let tag_banner = gtk::TextTag::builder().weight(700).build(); // Pango bold
    let tag_dim = gtk::TextTag::builder().foreground("#88898f").build();
    for tag in [&tag_error, &tag_success, &tag_banner, &tag_dim] {
        buf.tag_table().add(tag);
    }

    let append_log: Rc<dyn Fn(&str)> = {
        let text_view = text_view;
        let action_label = action_label;
        let pulsing = pulsing.clone();
        let progress_bar = progress_bar.clone();
        let seen_count = seen_count.clone();
        let last_logged_line = last_logged_line.clone();
        Rc::new(move |line: &str| {
            if let Some(pct) = extract_percentage(line) {
                // Not logged to the Details pane individually — there
                // can be dozens per file.
                pulsing.set(false);
                progress_bar.set_fraction(f64::from(pct) / 100.0);
                last_pct.set(Some(pct));
                set_bar_text();
                return;
            }
            if *last_logged_line.borrow() == line {
                return;
            }
            *last_logged_line.borrow_mut() = line.to_string();
            let clean = strip_log_decoration(line);
            let (display, tag) = if line == "OK" {
                ("\u{2713} Command completed".to_string(), Some(&tag_success))
            } else if line == "READY" {
                ("Privileged session ready.".to_string(), Some(&tag_dim))
            } else if line.starts_with("ERROR") {
                (clean.clone(), Some(&tag_error))
            } else if line.contains("[*]") {
                (clean.clone(), Some(&tag_banner))
            } else if clean.ends_with("successfully.") {
                (clean.clone(), Some(&tag_success))
            } else if clean.starts_with('(') {
                // This dialog's own annotations.
                (clean.clone(), Some(&tag_dim))
            } else {
                (clean.clone(), None)
            };
            let buf = text_view.buffer();
            let mut end = buf.end_iter();
            match tag {
                Some(tag) => buf.insert_with_tags(&mut end, &display, &[tag]),
                None => buf.insert(&mut end, &display),
            }
            buf.insert(&mut end, "\n");
            let mark = buf.get_insert();
            let end = buf.end_iter();
            buf.move_mark(&mark, &end);
            text_view.scroll_mark_onscreen(&mark);

            if line != "OK" && line != "READY" && !clean.is_empty() {
                if total_pkgs > 1 {
                    if let Some(pkgver) = extract_pkgver(&clean) {
                        if *last_pkgver.borrow() != pkgver {
                            let n = (seen_count.get() + 1).min(total_pkgs);
                            seen_count.set(n);
                            *last_pkgver.borrow_mut() = pkgver.to_string();
                            last_pct.set(None); // stale once the package changes
                            set_bar_text();
                        }
                    }
                }
                action_label.set_text(&clean);
            }
        })
    };

    // These listeners live on the shared, long-lived `session`, so they
    // must be detached once this batch finishes or they'd keep firing
    // against a destroyed dialog for the rest of the app's lifetime.
    let log_id = {
        let append_log = append_log.clone();
        session.connect_log(move |line| append_log(line))
    };
    let auth_failed = Rc::new(Cell::new(false));
    let disconnected_id = {
        let append_log = append_log.clone();
        let auth_failed = auth_failed.clone();
        session.connect_disconnected(move |reason| match reason {
            DisconnectReason::Expected => append_log("(privileged session ended)"),
            DisconnectReason::Unexpected => append_log("(privileged session ended unexpectedly)"),
            DisconnectReason::AuthFailed => {
                auth_failed.set(true);
                append_log(
                    "(could not authenticate as root — no polkit authentication agent \
                     responded; a bare window manager setup may need one added to its \
                     startup, e.g. polkit-gnome, lxqt-policykit, polkit-mate)",
                );
            }
        })
    };
    let on_finished = {
        let spinner = spinner;
        let progress_bar = progress_bar;
        let bar_text_label = bar_text_label;
        let status_label = status_label;
        let close_btn = close_btn.clone();
        let session_for_cleanup = session.clone();
        let dlg_for_close = dlg.clone();
        move |success: bool| {
            pulsing.set(false);
            spinner.stop();
            progress_bar.set_fraction(1.0);
            // The last percentage tick seen is rarely actually 100, so
            // drop it from the overlay text; the count is still meaningful.
            bar_text_label.set_text(&if total_pkgs > 1 {
                format!("Package {} of {}", seen_count.get(), total_pkgs)
            } else {
                String::new()
            });
            status_label.set_text(if success {
                "Finished successfully."
            } else if auth_failed.get() {
                "Could not authenticate as root — see details below."
            } else {
                "Finished with errors."
            });
            close_btn.set_sensitive(true);
            done_cb(success);

            session_for_cleanup.disconnect_log(log_id);
            session_for_cleanup.disconnect_disconnected(disconnected_id);

            if success && AUTO_CLOSE_ON_SUCCESS.get() {
                dlg_for_close.destroy();
            }
        }
    };

    {
        let dlg_c = dlg.clone();
        close_btn.connect_clicked(move |_| dlg_c.destroy());
    }
    {
        // Disabling the Close button doesn't stop the WM's own close
        // affordance; block the close request outright while busy so
        // the dialog (and its finished listener) can't disappear early.
        let close_btn = close_btn;
        dlg.connect_close_request(move |_| {
            if close_btn.is_sensitive() {
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            }
        });
    }

    dlg.present();
    session.run_batch(commands.to_vec(), on_finished);
}

/// Same as [`run`], but also records the batch to
/// `crate::backend::history` before `done_cb` fires.
pub fn run_recorded(
    parent: Option<&gtk::Window>,
    session: &Transaction,
    commands: &[String],
    title: &str,
    done_cb: impl Fn(bool) + 'static,
) {
    let commands_for_history = commands.to_vec();
    run(parent, session, commands, title, move |success| {
        crate::backend::history::record(&commands_for_history, success);
        done_cb(success);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkgver_extracted_from_status_lines() {
        assert_eq!(
            extract_pkgver("foo-1.0_1: unpacking ..."),
            Some("foo-1.0_1")
        );
        assert_eq!(
            extract_pkgver("foo-1.0_1: configuring ..."),
            Some("foo-1.0_1")
        );
        assert_eq!(
            extract_pkgver("foo-1.0_1: installed successfully."),
            Some("foo-1.0_1")
        );
        assert_eq!(extract_pkgver("bar-2.3_1: removing ..."), Some("bar-2.3_1"));
    }

    #[test]
    fn pkgver_not_extracted_from_banner_lines() {
        assert_eq!(extract_pkgver("Downloading packages"), None);
        assert_eq!(extract_pkgver("Verifying package integrity"), None);
        assert_eq!(extract_pkgver(""), None);
        // Two space-separated words before any ':' isn't a pkgver either.
        assert_eq!(extract_pkgver("some words: not a pkgver"), None);
    }

    #[test]
    fn same_package_does_not_recount() {
        // The bug this guards against: xbps prints several differently
        // worded lines for the *same* package, which must not each be
        // treated as a new package by the caller comparing consecutive
        // `extract_pkgver` results.
        let lines = [
            "foo-1.0_1: unpacking ...",
            "foo-1.0_1: configuring ...",
            "foo-1.0_1: installed successfully.",
        ];
        let pkgvers: Vec<_> = lines.iter().map(|l| extract_pkgver(l)).collect();
        assert!(pkgvers.iter().all(|p| *p == Some("foo-1.0_1")));
    }

    #[test]
    fn percentage_extracted_from_progress_lines() {
        assert_eq!(
            extract_percentage("foo-1.0_1: [*****     ] 42% ETA: 00:03"),
            Some(42)
        );
        assert_eq!(
            extract_percentage("foo-1.0_1: [**********] 100%"),
            Some(100)
        );
        assert_eq!(extract_percentage("no percentage here"), None);
        assert_eq!(extract_percentage("just a % with no digits"), None);
    }

    #[test]
    fn percentage_clamped_to_100() {
        // Defensive only — xbps never actually emits over 100%.
        assert_eq!(extract_percentage("foo: 150%"), Some(100));
    }

    #[test]
    fn count_target_packages_sums_only_package_taking_verbs() {
        let commands = vec![
            "SYNC".to_string(),
            "INSTALL foo bar".to_string(),
            "REMOVE baz".to_string(),
            "UPGRADE".to_string(),
            "HOLD qux".to_string(),
        ];
        assert_eq!(count_target_packages(&commands), 4);
    }

    #[test]
    fn strip_log_decoration_removes_prefixes() {
        assert_eq!(
            strip_log_decoration("LOG [*] Downloading packages"),
            "Downloading packages"
        );
        assert_eq!(
            strip_log_decoration("LOG foo-1.0_1: unpacking ..."),
            "foo-1.0_1: unpacking ..."
        );
        assert_eq!(strip_log_decoration("OK"), "OK");
    }
}
