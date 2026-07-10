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

const APP_ID: &str = "org.voidlinux.caerus";

fn main() -> glib::ExitCode {
    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::default());

    app.connect_startup(|_app| {
        // The icon is installed into $datadir/icons/hicolor/scalable/apps/
        // by the packaging (see data/org.voidlinux.caerus.desktop) — that
        // copy is what the desktop launcher / window manager taskbar look
        // up via the .desktop file's Icon=. Setting the default window
        // icon name here additionally covers this process's own windows
        // even when run straight out of a build directory before
        // installing, exactly like the original's on_startup().
        gtk::Window::set_default_icon_name(APP_ID);
    });

    app.connect_activate(|app| {
        let window = ui::window::build_window(app);
        window.present();
    });

    app.run()
}
