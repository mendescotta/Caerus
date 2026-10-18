//! Keep GTK on its low-memory Cairo renderer unless the user selects another backend.

pub fn configure_at_startup() {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: called at the start of main, before GTK initialization or worker threads.
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }
}
