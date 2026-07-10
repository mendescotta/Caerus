//! caerus — a Synaptic-inspired GTK4 package manager for Void Linux,
//! built directly on libxbps. Rust translation of src/main.c.
//!
//! caerus runs entirely unprivileged. Only `caerus-helper` is ever
//! pkexec'd (see backend::transaction) — the GUI itself never needs
//! root.

mod backend;
mod ui;

use gio::prelude::*;
use gtk::prelude::*;

/// Also the icon name (matches `caerus/data/icons/hicolor/scalable/apps/
/// org.voidlinux.caerus.svg` and the installed `.desktop` file's `Icon=`)
/// — reused by `ui::window`'s About dialog rather than duplicating the
/// literal.
pub const APP_ID: &str = "org.voidlinux.caerus";

/// Dev build: `caerus/data/icons/` lives in the source tree, not next
/// to the compiled binary (unlike `caerus-helper`, it's not a build
/// artifact `cargo` produces) — walk up from `target/{debug,release}/
/// caerus` to the repo root and into `caerus/data/icons`. Returns
/// `None` for an installed build (where the icon is already reachable
/// through the standard `/usr/share/icons/hicolor` search path), so
/// this is purely an *additional* search path, never a replacement.
///
/// `GtkIconTheme::add_search_path` treats its argument as a directory
/// of *themes* (mirroring `/usr/share/icons`, which contains `hicolor/`,
/// `Adwaita/`, etc. as siblings) and looks for `<path>/<theme>/<size>/
/// <context>/<icon>` under it — not as the icon tree itself. So the
/// source layout mirrors the installed one
/// (`caerus/data/icons/hicolor/scalable/apps/...`, matching
/// `$datadir/icons/hicolor/scalable/apps/...`) and this points
/// `add_search_path` at `caerus/data/icons` — the `hicolor/` directory
/// it contains is what actually gets matched against the `hicolor`
/// fallback theme every `GtkIconTheme` already checks.
fn find_dev_icon_search_dir() -> Option<std::path::PathBuf> {
    let exe = std::fs::read_link("/proc/self/exe").ok()?;
    // exe             = <repo>/target/{debug,release}/caerus
    // .parent()       = <repo>/target/{debug,release}   (exe's directory)
    // .parent()       = <repo>/target
    // .parent()       = <repo>                           <- the one we want
    let candidate = exe.parent()?.parent()?.parent()?.join("caerus/data/icons");
    candidate
        .join("hicolor/scalable/apps/org.voidlinux.caerus.svg")
        .is_file()
        .then_some(candidate)
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::default());

    app.connect_startup(|_app| {
        // `set_default_icon_name` alone only picks *which name* to look
        // up — it still needs the icon theme to actually resolve that
        // name to a file. When run straight out of the build tree
        // (before `install.sh` has copied the icon into
        // $datadir/icons/hicolor/...), no search path contains it, so
        // every icon-name lookup in the process (this window, and the
        // About dialog's logo) silently comes up blank. Registering the
        // source tree's icons/ directory as an extra search path fixes
        // that for dev builds without affecting an installed one.
        if let Some(dir) = find_dev_icon_search_dir() {
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::IconTheme::for_display(&display).add_search_path(dir);
            }
        }
        gtk::Window::set_default_icon_name(APP_ID);
    });

    app.connect_activate(|app| {
        let window = ui::window::build_window(app);
        window.present();
    });

    app.run()
}
