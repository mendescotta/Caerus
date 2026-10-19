use crate::backend::transaction::Transaction;
use crate::ui::dialog_util::{cancel_button_row, close_button, modal_window, present_focused};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

fn confirm_add_repo(parent: Option<&gtk::Window>, url: &str, cb: impl Fn(bool) + 'static) {
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);
    let (dlg, outer) = modal_window("Add Repository?", parent, false, (420, -1), 10);

    let heading = gtk::Label::new(Some(
        "Caerus doesn't verify custom repositories the way the official \
         Void mirrors are verified. If this repository is unsigned or \
         untrusted, packages installed from it could compromise your \
         system — Caerus always installs with automatic confirmation, so \
         no further warning will be shown at install time.",
    ));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let url_label = gtk::Label::new(Some(url));
    url_label.set_xalign(0.0);
    url_label.set_wrap(true);
    url_label.set_selectable(true);
    url_label.add_css_class("dim-label");
    outer.append(&url_label);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let add_btn = gtk::Button::with_label("Add Repository");
    add_btn.add_css_class("suggested-action");
    btn_box.append(&add_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&cancel_btn));

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            cb(false);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        add_btn.connect_clicked(move |_| {
            cb(true);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, &cancel_btn);
}

fn fingerprint_from_log(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("Fingerprint:")?;
    let fp = rest.trim().to_ascii_lowercase();
    let valid =
        fp.len() >= 8 && fp.contains(':') && fp.chars().all(|c| c.is_ascii_hexdigit() || c == ':');
    valid.then_some(fp)
}

fn trust_key_and_sync(inner: &Rc<Inner>, entry: &gtk::Entry, url: &str, fingerprint: &str) {
    let inner2 = inner.clone();
    let entry2 = entry.clone();
    let fp = fingerprint.to_string();
    confirm_trust_key(Some(&inner.dlg), url, fingerprint, move |confirmed| {
        if !confirmed {
            return;
        }
        let inner3 = inner2.clone();
        let entry3 = entry2.clone();
        let commands = vec![format!("SYNC_TRUST {fp}")];
        crate::ui::apply_dialog::run_recorded(
            Some(&inner2.dlg),
            &inner2.session,
            &commands,
            "Trusting Repository Key",
            move |_success| {
                entry3.set_text("");
                refresh(&inner3);
                (inner3.on_changed)();
            },
        );
    });
}

fn confirm_trust_key(
    parent: Option<&gtk::Window>,
    url: &str,
    fingerprint: &str,
    cb: impl Fn(bool) + 'static,
) {
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);
    let (dlg, outer) = modal_window("Trust Repository Key?", parent, false, (440, -1), 10);

    let heading = gtk::Label::new(Some(
        "This repository is signed with a key your system doesn't know yet. \
         Only continue if this fingerprint matches the one published by the \
         repository's maintainer. Caerus will import exactly this key and \
         refuse any other.",
    ));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    for text in [url, fingerprint] {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_selectable(true);
        label.add_css_class("dim-label");
        outer.append(&label);
    }

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let trust_btn = gtk::Button::with_label("Trust Key and Sync");
    trust_btn.add_css_class("suggested-action");
    btn_box.append(&trust_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&cancel_btn));

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            cb(false);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        trust_btn.connect_clicked(move |_| {
            cb(true);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, &cancel_btn);
}

fn confirm_remove_repo(parent: Option<&gtk::Window>, url: &str, cb: impl Fn(bool) + 'static) {
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);
    let (dlg, outer) = modal_window("Remove Repository?", parent, false, (420, -1), 10);

    let heading = gtk::Label::new(Some(
        "This repository will no longer be used for syncing or installing \
         packages. Packages already installed from it are not affected.",
    ));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    let url_label = gtk::Label::new(Some(url));
    url_label.set_xalign(0.0);
    url_label.set_wrap(true);
    url_label.set_selectable(true);
    url_label.add_css_class("dim-label");
    outer.append(&url_label);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let remove_btn = gtk::Button::with_label("Remove Repository");
    remove_btn.add_css_class("destructive-action");
    btn_box.append(&remove_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&cancel_btn));

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            cb(false);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        remove_btn.connect_clicked(move |_| {
            cb(true);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, &cancel_btn);
}

pub(crate) fn configured_repo_urls() -> std::collections::HashSet<String> {
    scan_configured_repos()
        .into_iter()
        .map(|(url, _, _)| url)
        .collect()
}

fn scan_configured_repos() -> Vec<(String, bool, bool)> {
    if is_apk_backend() {
        #[cfg(feature = "apk")]
        if let Some(repositories) = resolved_apk_repositories() {
            return repositories
                .into_iter()
                .map(|url| (url, false, true))
                .collect();
        }
        let mut repos = std::collections::BTreeMap::new();
        for (path, vendor) in apk_repository_files() {
            let Ok(contents) = std::fs::read_to_string(path) else {
                continue;
            };
            for line in contents.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') || line.contains('$') {
                    continue;
                }
                repos.entry(line.to_owned()).or_insert((!vendor, true));
            }
        }
        return repos
            .into_iter()
            .map(|(url, (editable, enabled))| (url, editable, enabled))
            .collect();
    }
    scan_configured_repos_in("/etc/xbps.d", "/usr/share/xbps.d")
}

fn scan_configured_repos_in(etc_dir: &str, vendor_dir: &str) -> Vec<(String, bool, bool)> {
    let mut map: std::collections::BTreeMap<String, (bool, bool, bool)> =
        std::collections::BTreeMap::new();
    let mut etc_names: std::collections::HashSet<std::ffi::OsString> =
        std::collections::HashSet::new();
    for (dir, is_etc) in [(etc_dir, true), (vendor_dir, false)] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "conf")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| !n.starts_with('.'))
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Some(name) = path.file_name() {
                if is_etc {
                    etc_names.insert(name.to_os_string());
                } else if etc_names.contains(name) {
                    continue;
                }
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in contents.lines() {
                let line = line.trim();
                let repository = line
                    .strip_prefix("repository=")
                    .map(|url| (url, true))
                    .or_else(|| {
                        is_etc
                            .then(|| line.strip_prefix("#repository="))
                            .flatten()
                            .map(|url| (url, false))
                    });
                if let Some((url, enabled)) = repository {
                    let url = url.trim();
                    if !url.is_empty() {
                        let state = map.entry(url.to_string()).or_insert((false, false, false));
                        state.0 |= is_etc;
                        state.1 |= !is_etc;
                        state.2 |= enabled;
                    }
                }
            }
        }
    }
    map.into_iter()
        .map(|(url, (has_etc, has_effective_vendor, enabled))| {
            (url, has_etc && !has_effective_vendor, enabled)
        })
        .collect()
}

#[cfg(feature = "apk")]
fn resolved_apk_repositories() -> Option<std::collections::BTreeSet<String>> {
    let output = std::process::Command::new("apk")
        .args([
            "query",
            "--from",
            "repositories",
            "--fields",
            "repositories",
            "--format",
            "json",
            "*",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let packages: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let mut urls = std::collections::BTreeSet::new();
    for package in packages.as_array()? {
        let Some(repositories) = package
            .get("repositories")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for repository in repositories {
            let Some(url) = repository.as_str() else {
                continue;
            };
            if url != "lib/apk/db/installed" {
                urls.insert(url.to_owned());
            }
        }
    }
    Some(urls)
}

fn is_apk_backend() -> bool {
    cfg!(feature = "apk")
}

fn apk_repository_files() -> Vec<(std::path::PathBuf, bool)> {
    let mut paths = Vec::new();
    for (path, vendor) in [
        ("/etc/apk/repositories", false),
        ("/etc/apk/repositories.d", false),
        ("/usr/lib/apk/repositories.d", true),
        ("/usr/share/apk/repositories.d", true),
    ] {
        let path = std::path::Path::new(path);
        if path.is_file() {
            paths.push((path.to_path_buf(), vendor));
        } else if let Ok(entries) = std::fs::read_dir(path) {
            paths.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|entry| entry.is_file())
                    .map(|entry| (entry, vendor)),
            );
        }
    }
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    paths
}

struct Inner {
    dlg: gtk::Window,
    session: Transaction,
    repos_list: gtk::ListBox,
    on_changed: Box<dyn Fn()>,
}

fn refresh(inner: &Rc<Inner>) {
    while let Some(child) = inner.repos_list.first_child() {
        inner.repos_list.remove(&child);
    }
    for (url, in_etc, enabled) in scan_configured_repos() {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);

        let l = gtk::Label::new(Some(&url));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        l.set_selectable(true);
        if !enabled {
            l.add_css_class("dim-label");
        }
        row_box.append(&l);

        if !in_etc {
            let badge = gtk::Label::new(Some("vendor"));
            badge.add_css_class("dim-label");
            badge.set_tooltip_text(Some(if is_apk_backend() {
                "System package configuration"
            } else {
                "Void default repository — can be disabled, but not removed here"
            }));
            row_box.append(&badge);
        }

        if is_apk_backend() {
            inner.repos_list.append(&repo_row(row_box));
            continue;
        }
        let switch = gtk::Switch::new();
        switch.set_state(enabled);
        switch.set_active(enabled);
        switch.set_valign(gtk::Align::Center);
        switch.set_tooltip_text(Some("Enable/disable this repository"));
        {
            let inner = inner.clone();
            let url = url.clone();
            switch.connect_state_set(move |switch, want_enabled| {
                let inner2 = inner.clone();
                let switch = switch.clone();
                let verb = if want_enabled {
                    "ENABLEREPO"
                } else {
                    "DISABLEREPO"
                };
                let commands = vec![format!("{verb} {url}"), "SYNC".to_string()];
                let title = if want_enabled {
                    "Enabling Repository"
                } else {
                    "Disabling Repository"
                };
                crate::ui::apply_dialog::run_recorded(
                    Some(&inner.dlg),
                    &inner.session,
                    &commands,
                    title,
                    move |success| {
                        if success {
                            switch.set_state(want_enabled);
                        }
                        refresh(&inner2);
                        (inner2.on_changed)();
                    },
                );
                glib::Propagation::Stop
            });
        }
        row_box.append(&switch);

        if in_etc {
            let btn = gtk::Button::from_icon_name("user-trash-symbolic");
            btn.set_tooltip_text(Some("Remove this repository"));
            btn.add_css_class("flat");
            let inner = inner.clone();
            let url = url.clone();
            btn.connect_clicked(move |_| {
                let inner2 = inner.clone();
                let url2 = url.clone();
                confirm_remove_repo(Some(&inner.dlg), &url, move |confirmed| {
                    if !confirmed {
                        return;
                    }
                    let inner3 = inner2.clone();
                    let commands = vec![format!("REMOVEREPO {}", url2), "SYNC".to_string()];
                    crate::ui::apply_dialog::run_recorded(
                        Some(&inner2.dlg),
                        &inner2.session,
                        &commands,
                        "Removing Repository",
                        move |_success| {
                            refresh(&inner3);
                            (inner3.on_changed)();
                        },
                    );
                });
            });
            row_box.append(&btn);
        }

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.set_child(Some(&row_box));
        inner.repos_list.append(&row);
    }
}

fn repo_row(row_box: gtk::Box) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&row_box));
    row
}

pub fn show(parent: Option<&gtk::Window>, session: &Transaction, on_changed: impl Fn() + 'static) {
    let (dlg, outer) = modal_window("Repositories", parent, true, (500, 380), 8);

    let repos_scroll = gtk::ScrolledWindow::new();
    repos_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    repos_scroll.set_vexpand(true);
    let repos_list = gtk::ListBox::new();
    repos_list.set_selection_mode(gtk::SelectionMode::None);
    repos_scroll.set_child(Some(&repos_list));
    outer.append(&repos_scroll);

    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_row.set_margin_top(8);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://repo-default.voidlinux.org/current"));
    entry.set_hexpand(true);
    entry.set_activates_default(true);
    let add_btn = gtk::Button::with_label("Add");
    add_btn.add_css_class("suggested-action");
    add_row.append(&entry);
    add_row.append(&add_btn);
    outer.append(&add_row);
    if is_apk_backend() {
        entry.set_sensitive(false);
        add_btn.set_sensitive(false);
        let note = gtk::Label::new(Some("APK repositories are shown for inspection. Editing sources is unavailable with this backend."));
        note.set_xalign(0.0);
        note.set_wrap(true);
        outer.append(&note);
        add_btn.set_tooltip_text(Some("Repository editing is unavailable with APK"));
        entry.set_tooltip_text(Some(
            "Repository editing is not supported by the APK backend",
        ));
    }

    close_button(&outer, &dlg, 4);

    dlg.set_default_widget(Some(&add_btn));

    let inner = Rc::new(Inner {
        dlg: dlg.clone(),
        session: session.clone(),
        repos_list,
        on_changed: Box::new(on_changed),
    });

    {
        let inner = inner.clone();
        let entry = entry.clone();
        add_btn.connect_clicked(move |_| {
            let url = entry.text().trim().to_string();
            if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return;
            }
            let inner2 = inner.clone();
            let entry2 = entry.clone();
            let url_for_run = url.clone();
            confirm_add_repo(Some(&inner.dlg), &url, move |confirmed| {
                if !confirmed {
                    return;
                }
                let inner3 = inner2.clone();
                let entry3 = entry2.clone();
                let url3 = url_for_run.clone();
                let commands = vec![format!("ADDREPO {}", url_for_run), "SYNC".to_string()];
                let seen_fp: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
                let log_id = inner2.session.connect_log({
                    let seen_fp = seen_fp.clone();
                    move |line| {
                        if let Some(fp) = fingerprint_from_log(line) {
                            *seen_fp.borrow_mut() = Some(fp);
                        }
                    }
                });
                crate::ui::apply_dialog::run_recorded(
                    Some(&inner2.dlg),
                    &inner2.session,
                    &commands,
                    "Adding Repository",
                    move |success| {
                        inner3.session.disconnect_log(log_id);
                        let pending_key = seen_fp.borrow_mut().take();
                        if let (false, Some(fp)) = (success, pending_key) {
                            trust_key_and_sync(&inner3, &entry3, &url3, &fp);
                            return;
                        }
                        entry3.set_text("");
                        refresh(&inner3);
                        (inner3.on_changed)();
                    },
                );
            });
        });
    }

    refresh(&inner);

    present_focused(&dlg, &entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RepoFixture(std::path::PathBuf);

    impl RepoFixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "caerus-ui-repos-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_nanos())
            ));
            std::fs::create_dir_all(path.join("etc")).expect("create fixture etc directory");
            std::fs::create_dir_all(path.join("vendor")).expect("create fixture vendor directory");
            Self(path)
        }

        fn etc(&self) -> std::path::PathBuf {
            self.0.join("etc")
        }

        fn vendor(&self) -> std::path::PathBuf {
            self.0.join("vendor")
        }
    }

    impl Drop for RepoFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn repo_scan_aggregates_enabled_duplicates_and_normalizes_whitespace() {
        let fixture = RepoFixture::new();
        let url = "https://repo.example/current";
        std::fs::write(
            fixture.etc().join("10-first.conf"),
            format!("#repository={url}\n"),
        )
        .expect("write first local fixture");
        std::fs::write(
            fixture.etc().join("90-shadow.conf"),
            format!("repository={url}  \nlocal-setting=keep\n"),
        )
        .expect("write shadowing local fixture");
        std::fs::write(
            fixture.vendor().join("90-shadow.conf"),
            format!("repository={url}\n"),
        )
        .expect("write vendor fixture");

        let repos = scan_configured_repos_in(
            fixture.etc().to_str().expect("fixture path is UTF-8"),
            fixture.vendor().to_str().expect("fixture path is UTF-8"),
        );

        assert_eq!(repos, vec![(url.to_string(), true, true)]);
    }

    #[test]
    fn repo_with_effective_vendor_duplicate_is_not_removable_from_local_copy() {
        let fixture = RepoFixture::new();
        let url = "https://repo.example/current";
        std::fs::write(
            fixture.etc().join("10-local.conf"),
            format!("#repository={url}\n"),
        )
        .expect("write local fixture");
        std::fs::write(
            fixture.vendor().join("20-vendor.conf"),
            format!("repository={url}\n"),
        )
        .expect("write vendor fixture");

        let repos = scan_configured_repos_in(
            fixture.etc().to_str().expect("fixture path is UTF-8"),
            fixture.vendor().to_str().expect("fixture path is UTF-8"),
        );

        assert_eq!(repos, vec![(url.to_string(), false, true)]);
    }

    #[test]
    fn fingerprint_parsed_from_helper_log() {
        assert_eq!(
            fingerprint_from_log("LOG Fingerprint: 3D:22:2E:EB:26:A5:15:A8").as_deref(),
            Some("3d:22:2e:eb:26:a5:15:a8")
        );
        assert_eq!(fingerprint_from_log("LOG Fingerprint: nope"), None);
        assert_eq!(fingerprint_from_log("LOG syncing"), None);
    }
}
