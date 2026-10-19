//! Optional startup diagnostics; never refreshes repositories or changes packages.
use std::sync::OnceLock;
use std::time::Instant;

pub fn trace(message: impl std::fmt::Display) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static START: OnceLock<Instant> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("CAERUS_TRACE_STARTUP").is_some()) {
        let elapsed = START.get_or_init(Instant::now).elapsed();
        eprintln!(
            "caerus startup {:>8.1} ms: {message}",
            elapsed.as_secs_f64() * 1000.0
        );
    }
}
