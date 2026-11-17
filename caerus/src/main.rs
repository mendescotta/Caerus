mod backend;
mod renderer;
mod startup;
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

fn main() -> glib::ExitCode {
    startup::trace("process started");
    renderer::configure_at_startup();
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
    });

    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let window = ui::window::build_window(app);
        window.present();
        startup::trace("window presented");
    });

    app.run()
}
