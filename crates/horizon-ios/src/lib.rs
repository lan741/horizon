//! Horizon's iOS/iPadOS platform layer.
//!
//! This is the first stage of the mobile port: it owns everything the engine expects from a
//! desktop `main` (an event loop, a Metal surface, a frame clock, a pointer/touch device and the
//! writable directories), so the desktop entry can be replaced by [`run`] without touching the
//! game's logic. See `docs/ios-port.md` for what is wired and what is still desktop-only.
#![forbid(unsafe_code)]

pub mod app;
pub mod gpu;
pub mod paths;
pub mod touch;

/// Start the app: the body of the executable's `main`, on every platform so `cargo check` runs
/// everywhere while only iOS ships it.
pub fn run() -> ! {
    app::run()
}
