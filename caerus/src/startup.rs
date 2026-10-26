//! Optional startup diagnostics; never refreshes repositories or changes packages.
use std::io::Write;
use std::sync::OnceLock;
use std::time::Instant;

/// Whether `CAERUS_TRACE_STARTUP` is set; check it before computing costly
/// trace arguments.
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CAERUS_TRACE_STARTUP").is_some())
}

pub fn trace(message: impl std::fmt::Display) {
    static START: OnceLock<Instant> = OnceLock::new();
    if enabled() {
        let elapsed = START.get_or_init(Instant::now).elapsed();
        // Unlike eprintln!, a closed stderr (e.g. `| head`) must not panic a
        // reload on the worker or main thread.
        let _ = writeln!(
            std::io::stderr(),
            "caerus startup {:>8.1} ms: {message}",
            elapsed.as_secs_f64() * 1000.0
        );
    }
}
