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

/// Plain GTK4 (unlike libadwaita) never reads the desktop's dark/light
/// preference on its own outside of a Flatpak sandbox — `GtkSettings`'s
/// `gtk-application-prefer-dark-theme` only follows XSettings/
/// `settings.ini`, which most desktops (including GNOME since it moved
/// dark-mode to the separate `color-scheme` key) don't populate for it.
/// So, like `libadwaita` itself does internally, ask the
/// `org.freedesktop.portal.Settings` portal directly and apply its
/// answer, then keep listening for `SettingChanged` so toggling dark
/// mode system-wide is picked up live instead of only at next launch.
/// Silently does nothing if the portal isn't available (e.g. no
/// `xdg-desktop-portal` running) — the app just falls back to whatever
/// GTK would otherwise have picked.
fn sync_color_scheme_from_portal() {
    // The `Read` reply and `SettingChanged`'s `value` param are declared
    // as `variant` in the portal spec, but GNOME's implementation nests
    // the actual value inside an *extra* variant layer on top of that
    // (confirmed by printing a real reply: `(<<uint32 1>>,)` — two levels
    // of `<>` boxing, not one). Keep unwrapping until nothing's left.
    fn unwrap_variant(mut value: glib::Variant) -> glib::Variant {
        while let Some(inner) = value.as_variant() {
            value = inner;
        }
        value
    }

    let apply = |value: u32| {
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(value == 1);
        }
    };

    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return;
    };

    if let Ok(reply) = connection.call_sync(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
        "Read",
        Some(&("org.freedesktop.appearance", "color-scheme").to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
    ) {
        if let Some(value) = unwrap_variant(reply.child_value(0)).get::<u32>() {
            apply(value);
        }
    }

    connection.signal_subscribe(
        Some("org.freedesktop.portal.Desktop"),
        Some("org.freedesktop.portal.Settings"),
        Some("SettingChanged"),
        Some("/org/freedesktop/portal/desktop"),
        None,
        gio::DBusSignalFlags::NONE,
        move |_conn, _sender, _path, _iface, _signal, params| {
            // SettingChanged's params are (namespace, key, value).
            if params.n_children() == 3
                && params.child_value(0).str() == Some("org.freedesktop.appearance")
                && params.child_value(1).str() == Some("color-scheme")
            {
                if let Some(value) = unwrap_variant(params.child_value(2)).get::<u32>() {
                    apply(value);
                }
            }
        },
    );
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
        sync_color_scheme_from_portal();
    });

    app.connect_activate(|app| {
        let window = ui::window::build_window(app);
        window.present();
    });

    app.run()
}
