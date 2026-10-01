mod backend;
mod ui;

use gio::prelude::*;
use gtk::prelude::*;

pub const APP_ID: &str = "org.voidlinux.caerus";

fn find_dev_icon_search_dir() -> Option<std::path::PathBuf> {
    let exe = std::fs::read_link("/proc/self/exe").ok()?;
    let candidate = exe.parent()?.parent()?.parent()?.join("caerus/data/icons");
    candidate
        .join("hicolor/scalable/apps/org.voidlinux.caerus.svg")
        .is_file()
        .then_some(candidate)
}

#[cfg(not(feature = "adwaita"))]
fn sync_color_scheme_from_portal() {
    fn unwrap_variant(mut value: glib::Variant) -> glib::Variant {
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
    #[cfg(feature = "adwaita")]
    {
        adw::init().expect("libadwaita init failed");
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferLight);
    }

    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::default());

    app.connect_startup(|_app| {
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
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let window = ui::window::build_window(app);
        window.present();
    });

    app.run()
}
