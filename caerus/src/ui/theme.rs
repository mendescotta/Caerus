//! Appearance: the color scheme (System / Light / Dark) and the optional
//! built-in Caerus theme.
//!
//! Everything that touches `GtkSettings`, the display style providers or the
//! desktop's dark/light preference lives here. The scheme-resolution order is a
//! pure function (`resolve_system`) so it can be tested without a display.
//!
//! The approach to the built-in theme (taking the desktop GTK theme out of the
//! picture, loading libgtk's embedded base stylesheet, scoping every component
//! rule under a root class) is adapted from Shelly's `services/theme.zig`
//! (Seafoam-Labs/Shelly-ALPM, GPL-3.0).

/// Window class present on every toplevel while the built-in theme is active.
/// All built-in component rules are scoped under it.
#[cfg_attr(feature = "adwaita", allow(dead_code))]
pub const ROOT_CLASS: &str = "caerus-themed";

/// Whether this build offers the built-in theme (the libadwaita build does not).
pub const BUILTIN_THEME_SUPPORTED: bool = cfg!(not(feature = "adwaita"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

impl ColorScheme {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_index(index: u32) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BuiltinTheme {
    /// Render with whatever GTK theme the desktop has (today's look).
    #[default]
    System,
    Caerus,
}

impl BuiltinTheme {
    pub const ALL: [Self; 2] = [Self::System, Self::Caerus];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "system" => Some(Self::System),
            "caerus" => Some(Self::Caerus),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Caerus => "caerus",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Caerus => "Caerus",
        }
    }

    pub fn from_index(index: u32) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as u32
    }
}

/// Pure scheme resolution, kept free of GTK so the order is unit-tested.
#[cfg_attr(feature = "adwaita", allow(dead_code))]
pub mod resolve {
    use super::ColorScheme;

    /// Where the "system" preference came from.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Source {
        GtkInterfaceScheme,
        Portal,
        GnomeSettings,
        PreferDarkSetting,
    }

    /// What each source reports: `Some(dark)` when it has an opinion,
    /// `None` when it is missing or has no preference.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Reports {
        pub gtk_interface_scheme: Option<bool>,
        pub portal: Option<bool>,
        pub gnome_settings: Option<bool>,
        pub prefer_dark_setting: bool,
    }

    /// `GtkInterfaceColorScheme`: 0 unsupported, 1 default, 2 dark, 3 light.
    pub fn gtk_interface_scheme(value: i32) -> Option<bool> {
        match value {
            2 => Some(true),
            3 => Some(false),
            _ => None,
        }
    }

    /// `org.freedesktop.appearance color-scheme`: 0 no preference, 1 dark, 2 light.
    pub fn portal(value: u32) -> Option<bool> {
        match value {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    /// `org.gnome.desktop.interface color-scheme`.
    pub fn gnome_settings(value: &str) -> Option<bool> {
        match value {
            "prefer-dark" => Some(true),
            "prefer-light" => Some(false),
            _ => None,
        }
    }

    /// First source with an opinion wins; the legacy prefer-dark setting is
    /// the last resort and always answers.
    pub fn system(reports: Reports) -> (bool, Source) {
        if let Some(dark) = reports.gtk_interface_scheme {
            return (dark, Source::GtkInterfaceScheme);
        }
        if let Some(dark) = reports.portal {
            return (dark, Source::Portal);
        }
        if let Some(dark) = reports.gnome_settings {
            return (dark, Source::GnomeSettings);
        }
        (reports.prefer_dark_setting, Source::PreferDarkSetting)
    }

    /// The variant to draw: an explicit choice beats the system preference.
    pub fn effective_dark(scheme: ColorScheme, system_dark: bool) -> bool {
        match scheme {
            ColorScheme::System => system_dark,
            ColorScheme::Light => false,
            ColorScheme::Dark => true,
        }
    }
}

#[cfg(test)]
pub use imp::smoke_test_theme_switching;
pub use imp::{apply, style_toplevel};

#[cfg(not(feature = "adwaita"))]
mod imp {
    use super::resolve::{self, Reports, Source};
    use super::{BuiltinTheme, ColorScheme, ROOT_CLASS};
    use gio::prelude::*;
    use glib::translate::{ToGlibPtr, ToGlibPtrMut};
    use gtk::prelude::*;
    use std::cell::{Cell, RefCell};

    const INTERFACE_SCHEME: &str = "gtk-interface-color-scheme";
    const PREFER_DARK: &str = "gtk-application-prefer-dark-theme";
    const THEME_NAME: &str = "gtk-theme-name";

    const DARK_PALETTE: &str = include_str!("themes/caerus-dark.css");
    const LIGHT_PALETTE: &str = include_str!("themes/caerus-light.css");
    const COMPONENTS: &str = include_str!("themes/caerus-components.css");

    /// A base stylesheet embedded in the running libgtk. It travels with the
    /// GTK build, so it always matches the widget internals.
    #[derive(Clone, Copy)]
    struct Base {
        resource: &'static str,
        /// Separate dark sheet for GTK before 4.16, which has no
        /// `prefers-color-scheme` media block in its base stylesheet.
        dark_resource: Option<&'static str>,
        media_driven: bool,
        pin: &'static str,
    }

    /// GTK 4.23 renamed the embedded Adwaita base to "Default".
    const DEFAULT_BASE: &str = "/org/gtk/libgtk/theme/Default/gtk.css";
    const ADWAITA_BASE: &str = "/org/gtk/libgtk/theme/Adwaita/gtk.css";
    const ADWAITA_BASE_DARK: &str = "/org/gtk/libgtk/theme/Adwaita/gtk-dark.css";
    /// Blank placeholder theme GTK embeds; pinning it empties GTK's own
    /// theme slot so nothing from the desktop selection loads.
    const EMPTY_THEME: &str = "/org/gtk/libgtk/theme/Empty/gtk.css";

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Forced {
        InterfaceScheme,
        PreferDark,
    }

    struct State {
        scheme: ColorScheme,
        theme: BuiltinTheme,
        dark: bool,
        forced: Option<Forced>,
        base: Option<Base>,
        base_resolved: bool,
        base_provider: Option<gtk::CssProvider>,
        base_loaded_dark: Option<bool>,
        theme_provider: Option<gtk::CssProvider>,
        theme_loaded_dark: Option<bool>,
        pinned: bool,
        watchers_started: bool,
        gnome: Option<gio::Settings>,
        portal: Option<u32>,
        portal_connection: Option<gio::DBusConnection>,
    }

    thread_local! {
        static STATE: RefCell<State> = const { RefCell::new(State {
            scheme: ColorScheme::System,
            theme: BuiltinTheme::System,
            dark: false,
            forced: None,
            base: None,
            base_resolved: false,
            base_provider: None,
            base_loaded_dark: None,
            theme_provider: None,
            theme_loaded_dark: None,
            pinned: false,
            watchers_started: false,
            gnome: None,
            portal: None,
            portal_connection: None,
        }) };
        /// Set while this module writes GtkSettings, so the notify handlers
        /// it triggers do not re-enter.
        static APPLYING: Cell<bool> = const { Cell::new(false) };
    }

    /// Apply the saved choices (first call) or a live change from settings.
    pub fn apply(scheme: ColorScheme, theme: BuiltinTheme) {
        let first = STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.scheme = scheme;
            st.theme = theme;
            !st.watchers_started
        });
        if first {
            start_watchers();
        }
        refresh();
    }

    /// Give a new toplevel (a dialog window) the root class when the
    /// built-in theme is active.
    pub fn style_toplevel(window: &impl IsA<gtk::Window>) {
        if STATE.with(|s| s.borrow().theme == BuiltinTheme::Caerus) {
            window.upcast_ref::<gtk::Window>().add_css_class(ROOT_CLASS);
        }
    }

    /// Parse errors from loading both variants into throw-away providers.
    #[cfg(test)]
    pub fn sheet_errors() -> Vec<String> {
        let errors = std::rc::Rc::new(RefCell::new(Vec::new()));
        for (name, dark) in [("light", false), ("dark", true)] {
            let provider = gtk::CssProvider::new();
            let sink = errors.clone();
            provider.connect_parsing_error(move |_, section, error| {
                sink.borrow_mut().push(format!("{name} {section}: {error}"));
            });
            provider.load_from_string(&theme_sheet(dark));
        }
        let result = errors.borrow().clone();
        result
    }

    #[cfg(test)]
    pub fn is_dark() -> bool {
        STATE.with(|s| s.borrow().dark)
    }

    #[cfg(test)]
    pub fn raw_interface_scheme() -> Option<i32> {
        gtk::Settings::default().and_then(|s| read_interface_scheme(&s))
    }

    /// Runtime switching of theme and scheme, including the restore path.
    /// Runs inside the single display test (GTK stays on one thread).
    #[cfg(test)]
    pub fn smoke_test_theme_switching() {
        let settings = gtk::Settings::default().expect("display settings");
        assert_eq!(
            sheet_errors(),
            Vec::<String>::new(),
            "theme sheets must parse"
        );
        apply(ColorScheme::System, BuiltinTheme::System);
        let desktop_theme = settings.gtk_theme_name();
        let desktop_scheme = raw_interface_scheme();
        let desktop_dark = is_dark();

        let early = gtk::Window::new();
        assert!(
            !early.has_css_class(ROOT_CLASS),
            "system mode adds no class"
        );

        apply(ColorScheme::Light, BuiltinTheme::Caerus);
        assert!(
            early.has_css_class(ROOT_CLASS),
            "existing toplevels get the class"
        );
        assert!(!is_dark());
        let (late, _) =
            crate::ui::dialog_util::modal_window("Theme probe", None, false, (200, 100), 4);
        assert!(
            late.has_css_class(ROOT_CLASS),
            "dialogs opened later are themed"
        );
        if resource_exists(EMPTY_THEME) {
            assert_eq!(settings.gtk_theme_name().as_deref(), Some("Empty"));
        }
        if has_interface_scheme(&settings) {
            assert_eq!(read_interface_scheme(&settings), Some(3));
        }

        apply(ColorScheme::Dark, BuiltinTheme::Caerus);
        assert!(is_dark());
        assert!(late.has_css_class(ROOT_CLASS));
        if has_interface_scheme(&settings) {
            assert_eq!(read_interface_scheme(&settings), Some(2));
        }

        // A forced scheme without the built-in theme leaves the desktop theme alone.
        apply(ColorScheme::Light, BuiltinTheme::System);
        assert!(!early.has_css_class(ROOT_CLASS));
        assert!(!late.has_css_class(ROOT_CLASS));
        assert_eq!(
            settings.gtk_theme_name(),
            desktop_theme,
            "the theme pin is released"
        );

        apply(ColorScheme::System, BuiltinTheme::Caerus);
        assert_eq!(is_dark(), desktop_dark, "System follows the desktop again");
        apply(ColorScheme::System, BuiltinTheme::System);
        assert_eq!(settings.gtk_theme_name(), desktop_theme);
        assert_eq!(
            raw_interface_scheme(),
            desktop_scheme,
            "scheme handed back to the desktop"
        );
        assert_eq!(is_dark(), desktop_dark);
        late.destroy();
        early.destroy();
    }

    fn set_root_class(on: bool) {
        for toplevel in gtk::Window::list_toplevels() {
            if on {
                toplevel.add_css_class(ROOT_CLASS);
            } else {
                toplevel.remove_css_class(ROOT_CLASS);
            }
        }
    }

    fn refresh() {
        if APPLYING.replace(true) {
            return;
        }
        let Some(display) = gtk::gdk::Display::default() else {
            APPLYING.set(false);
            return;
        };
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            apply_scheme(&mut st);
            apply_theme(&mut st, &display);
        });
        APPLYING.set(false);
    }

    // ---- color scheme ----------------------------------------------------

    fn has_interface_scheme(settings: &gtk::Settings) -> bool {
        settings.find_property(INTERFACE_SCHEME).is_some()
    }

    fn read_interface_scheme(settings: &gtk::Settings) -> Option<i32> {
        settings.find_property(INTERFACE_SCHEME)?;
        let value = settings.property_value(INTERFACE_SCHEME);
        // SAFETY: the property is an enum, checked by `find_property` above;
        // `value` holds that enum type.
        Some(unsafe { glib::gobject_ffi::g_value_get_enum(value.to_glib_none().0) })
    }

    fn write_interface_scheme(settings: &gtk::Settings, enum_value: i32) {
        let Some(pspec) = settings.find_property(INTERFACE_SCHEME) else {
            return;
        };
        let mut value = glib::Value::from_type(pspec.value_type());
        // SAFETY: `value` was created with the property's own enum type.
        unsafe {
            glib::gobject_ffi::g_value_set_enum(value.to_glib_none_mut().0, enum_value);
        }
        settings.set_property_from_value(INTERFACE_SCHEME, &value);
    }

    /// Pin the dark/light variant for an explicit Light/Dark choice, or for a
    /// system preference GTK itself cannot see (portal / GSettings on GTK
    /// without a portal reader).
    fn force_dark(st: &mut State, settings: &gtk::Settings, dark: bool) {
        if has_interface_scheme(settings) {
            let want = if dark { 2 } else { 3 };
            if read_interface_scheme(settings) != Some(want) {
                write_interface_scheme(settings, want);
            }
            st.forced = Some(Forced::InterfaceScheme);
        } else {
            if settings.is_gtk_application_prefer_dark_theme() != dark {
                settings.set_gtk_application_prefer_dark_theme(dark);
            }
            st.forced = Some(Forced::PreferDark);
        }
    }

    /// Hand the property back to the settings backends. Never write a value
    /// back: a property set by the application stops following the desktop.
    fn unforce(st: &mut State, settings: &gtk::Settings) {
        match st.forced.take() {
            Some(Forced::InterfaceScheme) => settings.reset_property(INTERFACE_SCHEME),
            Some(Forced::PreferDark) => settings.reset_property(PREFER_DARK),
            None => {}
        }
    }

    fn apply_scheme(st: &mut State) {
        let Some(settings) = gtk::Settings::default() else {
            st.dark = resolve::effective_dark(st.scheme, false);
            return;
        };
        match st.scheme {
            ColorScheme::Light | ColorScheme::Dark => {
                st.dark = st.scheme == ColorScheme::Dark;
                force_dark(st, &settings, st.dark);
            }
            ColorScheme::System => {
                unforce(st, &settings);
                let reports = Reports {
                    gtk_interface_scheme: read_interface_scheme(&settings)
                        .and_then(resolve::gtk_interface_scheme),
                    portal: st.portal.and_then(resolve::portal),
                    gnome_settings: st
                        .gnome
                        .as_ref()
                        .and_then(|g| resolve::gnome_settings(g.string("color-scheme").as_str())),
                    // GTK that has the interface color scheme deprecates this
                    // flag (reading it logs a warning), so only ask without it.
                    prefer_dark_setting: !has_interface_scheme(&settings)
                        && settings.is_gtk_application_prefer_dark_theme(),
                };
                let (dark, source) = resolve::system(reports);
                st.dark = dark;
                // GTK follows its own setting and the legacy prefer-dark
                // flag; the other two need to be pushed into it.
                if matches!(source, Source::Portal | Source::GnomeSettings) {
                    force_dark(st, &settings, dark);
                }
            }
        }
    }

    fn start_watchers() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.watchers_started = true;
            // GNOME's schema only when installed: Settings::new aborts otherwise.
            if let Some(schema) = gio::SettingsSchemaSource::default()
                .and_then(|src| src.lookup("org.gnome.desktop.interface", true))
            {
                if schema.has_key("color-scheme") {
                    let gnome = gio::Settings::new("org.gnome.desktop.interface");
                    gnome.connect_changed(Some("color-scheme"), |_, _| on_external_change());
                    st.gnome = Some(gnome);
                }
            }
        });
        if let Some(settings) = gtk::Settings::default() {
            if has_interface_scheme(&settings) {
                settings.connect_notify_local(Some(INTERFACE_SCHEME), |_, _| on_external_change());
            } else {
                settings.connect_notify_local(Some(PREFER_DARK), |_, _| on_external_change());
            }
            settings.connect_notify_local(Some(THEME_NAME), |_, _| on_theme_name_changed());
        }
        start_portal();
    }

    fn on_external_change() {
        if !APPLYING.get() {
            refresh();
        }
    }

    /// The desktop may re-apply its theme name while the built-in theme is
    /// on (e.g. an XSettings broadcast); keep the pin.
    fn on_theme_name_changed() {
        if APPLYING.get() {
            return;
        }
        if STATE.with(|s| s.borrow().theme == BuiltinTheme::Caerus) {
            refresh();
        }
    }

    fn unwrap_variant(mut value: glib::Variant) -> glib::Variant {
        while value.type_() == glib::VariantTy::VARIANT {
            let Some(inner) = value.as_variant() else {
                break;
            };
            value = inner;
        }
        value
    }

    /// Read the portal's color scheme and watch it for changes. A missing
    /// portal is not an error: the later sources in `resolve::system` answer.
    fn start_portal() {
        let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        else {
            return;
        };
        let initial = connection
            .call_sync(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "Read",
                Some(&("org.freedesktop.appearance", "color-scheme").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
            )
            .ok()
            .and_then(|reply| unwrap_variant(reply.child_value(0)).get::<u32>());
        connection.signal_subscribe(
            Some("org.freedesktop.portal.Desktop"),
            Some("org.freedesktop.portal.Settings"),
            Some("SettingChanged"),
            Some("/org/freedesktop/portal/desktop"),
            None,
            gio::DBusSignalFlags::NONE,
            |_conn, _sender, _path, _iface, _signal, params| {
                if params.n_children() == 3
                    && params.child_value(0).str() == Some("org.freedesktop.appearance")
                    && params.child_value(1).str() == Some("color-scheme")
                {
                    if let Some(value) = unwrap_variant(params.child_value(2)).get::<u32>() {
                        STATE.with(|s| s.borrow_mut().portal = Some(value));
                        on_external_change();
                    }
                }
            },
        );
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.portal = initial;
            st.portal_connection = Some(connection);
        });
    }

    // ---- built-in theme --------------------------------------------------

    fn resource_exists(path: &str) -> bool {
        gio::resources_get_info(path, gio::ResourceLookupFlags::NONE).is_ok()
    }

    /// Probe which base stylesheet this libgtk embeds. GTK's resource set is
    /// fixed for the session, so this runs once.
    fn resolve_base(st: &mut State) {
        if st.base_resolved {
            return;
        }
        st.base_resolved = true;
        let pin_for = |fallback: &'static str| {
            if resource_exists(EMPTY_THEME) {
                "Empty"
            } else {
                fallback
            }
        };
        st.base = if resource_exists(DEFAULT_BASE) {
            Some(Base {
                resource: DEFAULT_BASE,
                dark_resource: None,
                media_driven: true,
                pin: pin_for("Default"),
            })
        } else if resource_exists(ADWAITA_BASE) {
            let media_driven = gtk::major_version() > 4
                || (gtk::major_version() == 4 && gtk::minor_version() >= 16);
            Some(Base {
                resource: ADWAITA_BASE,
                dark_resource: Some(ADWAITA_BASE_DARK),
                media_driven,
                pin: pin_for("Adwaita"),
            })
        } else {
            None
        };
        if st.base.is_none() {
            eprintln!(
                "caerus: no embedded base theme found in this GTK; the Caerus theme is \
                 layered over the desktop theme instead of replacing it"
            );
        }
    }

    fn theme_sheet(dark: bool) -> String {
        let palette = if dark { DARK_PALETTE } else { LIGHT_PALETTE };
        format!("{palette}\n{COMPONENTS}")
    }

    fn apply_theme(st: &mut State, display: &gtk::gdk::Display) {
        let Some(settings) = gtk::Settings::default() else {
            return;
        };
        if st.theme == BuiltinTheme::Caerus {
            resolve_base(st);
            let dark = st.dark;
            if let Some(base) = st.base {
                if settings.gtk_theme_name().as_deref() != Some(base.pin) {
                    settings.set_gtk_theme_name(Some(base.pin));
                }
                st.pinned = true;
                let (provider, fresh) = match st.base_provider.clone() {
                    Some(p) => (p, false),
                    None => (gtk::CssProvider::new(), true),
                };
                let wanted_dark = !base.media_driven && dark;
                if st.base_loaded_dark != Some(wanted_dark) {
                    match (wanted_dark, base.dark_resource) {
                        (true, Some(path)) => provider.load_from_resource(path),
                        _ => provider.load_from_resource(base.resource),
                    }
                    st.base_loaded_dark = Some(wanted_dark);
                }
                if fresh {
                    gtk::style_context_add_provider_for_display(
                        display,
                        &provider,
                        gtk::STYLE_PROVIDER_PRIORITY_THEME,
                    );
                    st.base_provider = Some(provider);
                }
            }
            let (provider, fresh) = match st.theme_provider.clone() {
                Some(p) => (p, false),
                None => (gtk::CssProvider::new(), true),
            };
            if st.theme_loaded_dark != Some(dark) {
                provider.load_from_string(&theme_sheet(dark));
                st.theme_loaded_dark = Some(dark);
            }
            if fresh {
                // Above USER so ~/.config/gtk-4.0/gtk.css cannot repaint it.
                gtk::style_context_add_provider_for_display(
                    display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
                );
                st.theme_provider = Some(provider);
            }
            set_root_class(true);
        } else {
            if let Some(provider) = st.theme_provider.take() {
                gtk::style_context_remove_provider_for_display(display, &provider);
            }
            if let Some(provider) = st.base_provider.take() {
                gtk::style_context_remove_provider_for_display(display, &provider);
            }
            st.theme_loaded_dark = None;
            st.base_loaded_dark = None;
            if st.pinned {
                st.pinned = false;
                // Reset, do not write the old name back, so the desktop's
                // theme (and later changes to it) apply again.
                settings.reset_property(THEME_NAME);
            }
            set_root_class(false);
        }
    }
}

#[cfg(feature = "adwaita")]
mod imp {
    use super::{BuiltinTheme, ColorScheme};
    use gtk::prelude::*;

    /// The libadwaita build only maps the color scheme onto `AdwStyleManager`;
    /// the built-in theme is not offered here.
    pub fn apply(scheme: ColorScheme, _theme: BuiltinTheme) {
        adw::StyleManager::default().set_color_scheme(match scheme {
            // `Default` behaves like the `PreferLight` set at startup.
            ColorScheme::System => adw::ColorScheme::Default,
            ColorScheme::Light => adw::ColorScheme::ForceLight,
            ColorScheme::Dark => adw::ColorScheme::ForceDark,
        });
    }

    pub fn style_toplevel(_window: &impl IsA<gtk::Window>) {}

    /// The scheme maps onto the style manager; the theme choice is ignored.
    #[cfg(test)]
    pub fn smoke_test_theme_switching() {
        let manager = adw::StyleManager::default();
        apply(ColorScheme::Dark, BuiltinTheme::Caerus);
        assert!(manager.is_dark());
        apply(ColorScheme::Light, BuiltinTheme::Caerus);
        assert!(!manager.is_dark());
        apply(ColorScheme::System, BuiltinTheme::System);
        assert_eq!(manager.color_scheme(), adw::ColorScheme::Default);
    }
}

#[cfg(test)]
mod tests {
    use super::resolve::*;
    use super::*;

    #[test]
    fn scheme_and_theme_settings_parse_and_round_trip() {
        for scheme in ColorScheme::ALL {
            assert_eq!(ColorScheme::parse(scheme.key()), Some(scheme));
            assert_eq!(ColorScheme::from_index(scheme.index()), scheme);
        }
        for theme in BuiltinTheme::ALL {
            assert_eq!(BuiltinTheme::parse(theme.key()), Some(theme));
            assert_eq!(BuiltinTheme::from_index(theme.index()), theme);
        }
        assert_eq!(ColorScheme::parse(" dark "), Some(ColorScheme::Dark));
        assert_eq!(ColorScheme::parse("Dark"), None);
        assert_eq!(ColorScheme::parse("1"), None);
        assert_eq!(BuiltinTheme::parse("shelly"), None);
        assert_eq!(ColorScheme::from_index(99), ColorScheme::System);
        assert_eq!(BuiltinTheme::from_index(99), BuiltinTheme::System);
    }

    #[test]
    fn source_values_map_to_dark_light_or_no_opinion() {
        assert_eq!(gtk_interface_scheme(0), None);
        assert_eq!(gtk_interface_scheme(1), None);
        assert_eq!(gtk_interface_scheme(2), Some(true));
        assert_eq!(gtk_interface_scheme(3), Some(false));
        assert_eq!(portal(0), None);
        assert_eq!(portal(1), Some(true));
        assert_eq!(portal(2), Some(false));
        assert_eq!(portal(7), None);
        assert_eq!(gnome_settings("prefer-dark"), Some(true));
        assert_eq!(gnome_settings("prefer-light"), Some(false));
        assert_eq!(gnome_settings("default"), None);
    }

    #[test]
    fn first_source_with_an_opinion_wins() {
        let all = Reports {
            gtk_interface_scheme: Some(false),
            portal: Some(true),
            gnome_settings: Some(true),
            prefer_dark_setting: true,
        };
        assert_eq!(system(all), (false, Source::GtkInterfaceScheme));
        let no_gtk = Reports {
            gtk_interface_scheme: None,
            ..all
        };
        assert_eq!(system(no_gtk), (true, Source::Portal));
        let portal_light = Reports {
            portal: Some(false),
            ..no_gtk
        };
        assert_eq!(system(portal_light), (false, Source::Portal));
        let no_portal = Reports {
            portal: None,
            gnome_settings: Some(false),
            ..no_gtk
        };
        assert_eq!(system(no_portal), (false, Source::GnomeSettings));
    }

    #[test]
    fn legacy_prefer_dark_is_the_last_resort() {
        let dark = Reports {
            prefer_dark_setting: true,
            ..Reports::default()
        };
        assert_eq!(system(dark), (true, Source::PreferDarkSetting));
        assert_eq!(
            system(Reports::default()),
            (false, Source::PreferDarkSetting)
        );
    }

    #[test]
    fn explicit_scheme_overrides_the_system_preference() {
        for system_dark in [false, true] {
            assert!(!effective_dark(ColorScheme::Light, system_dark));
            assert!(effective_dark(ColorScheme::Dark, system_dark));
            assert_eq!(
                effective_dark(ColorScheme::System, system_dark),
                system_dark
            );
        }
    }
}
