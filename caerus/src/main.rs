//! caerus — a Synaptic-inspired GTK4 package manager for Void Linux,
//! built directly on libxbps.
//!
//! caerus runs entirely unprivileged. Only `caerus-helper` is ever
//! pkexec'd (see `backend::transaction`).

mod backend;
mod ui;

use gio::prelude::*;
use gtk::prelude::*;

/// Also the icon name (matches `caerus/data/icons/hicolor/scalable/apps/
/// org.voidlinux.caerus.svg` and the `.desktop` file's `Icon=`).
pub const APP_ID: &str = "org.voidlinux.caerus";

/// Dev build: `caerus/data/icons/` lives in the source tree, not next to
/// the compiled binary. Returns `None` for an installed build, where the
/// icon is already reachable via the standard search path.
///
/// `GtkIconTheme::add_search_path` treats its argument as a directory of
/// *themes* (mirroring `/usr/share/icons`) and looks for
/// `<path>/<theme>/<size>/<context>/<icon>` under it — so this points at
/// `caerus/data/icons`, whose `hicolor/` subdir is what actually matches.
fn find_dev_icon_search_dir() -> Option<std::path::PathBuf> {
    let exe = std::fs::read_link("/proc/self/exe").ok()?;
    // exe = <repo>/target/{debug,release}/caerus; walk up to <repo>.
    let candidate = exe.parent()?.parent()?.parent()?.join("caerus/data/icons");
    candidate
        .join("hicolor/scalable/apps/org.voidlinux.caerus.svg")
        .is_file()
        .then_some(candidate)
}

/// Only used by the plain-GTK4 build — the adwaita build's
/// `AdwStyleManager` tracks the portal itself, and setting
/// `gtk-application-prefer-dark-theme` alongside it is unsupported.
///
/// Plain GTK4 never reads the desktop's dark/light preference on its own
/// (`gtk-application-prefer-dark-theme` only follows `XSettings`, which
/// most desktops don't populate), so ask the
/// `org.freedesktop.portal.Settings` portal directly and keep listening
/// for `SettingChanged`. Silently does nothing if the portal isn't
/// available.
#[cfg(not(feature = "adwaita"))]
fn sync_color_scheme_from_portal() {
    // GNOME's portal reply nests the value in an extra variant layer on
    // top of what the spec declares; keep unwrapping until nothing's left.
    fn unwrap_variant(mut value: glib::Variant) -> glib::Variant {
        // Checking the type ourselves avoids a GLib-CRITICAL from
        // `as_variant()` on the final, already-unwrapped call.
        while value.type_() == glib::VariantTy::VARIANT {
            let Some(inner) = value.as_variant() else {
                break;
            };
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
    // Constructing any libadwaita widget activates its global
    // `AdwStyleManager`, restyling the whole process's GTK4 widgets —
    // call `adw::init()` up front so the app is adwaita-styled from the
    // first frame, not just once some Adw widget happens to be built.
    #[cfg(feature = "adwaita")]
    {
        adw::init().expect("libadwaita init failed");
        // PreferLight = follow the system's dark/light preference
        // (default ColorScheme would mean always-light).
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferLight);
    }

    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::default());

    app.connect_startup(|_app| {
        // `set_default_icon_name` alone needs the icon theme to resolve
        // the name to a file; register the dev tree as an extra search
        // path so a bare `cargo run` isn't left with a blank icon.
        if let Some(dir) = find_dev_icon_search_dir() {
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::IconTheme::for_display(&display).add_search_path(dir);
            }
        }
        gtk::Window::set_default_icon_name(APP_ID);
        #[cfg(not(feature = "adwaita"))]
        sync_color_scheme_from_portal();
    });

    app.connect_activate(|app| {
        // GApplication is unique per app id; present the existing window
        // instead of building a second one (duplicate stores/threads).
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let window = ui::window::build_window(app);
        window.present();
    });

    app.run()
}
