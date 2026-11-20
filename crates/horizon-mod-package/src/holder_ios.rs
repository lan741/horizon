//! The holder on iOS: there is no second process to hand the mod files' leases to.
//!
//! On desktop the game can be started by a launcher, which runs the game as a *holder* child that
//! keeps a lease on the installation's cache directories while the mod runtime runs beside it
//! (`holder.rs`, `runtime_leases.rs`). iOS cannot spawn a helper process and the app bundle is
//! read-only, so the whole lease mechanism has nothing to protect: the game owns its container
//! for the life of the process. The entry's three calls stay here so `entry.rs` reads the same on
//! every platform — and so the refusal is explicit rather than a missing symbol.
use crate::output::Capture;
use crate::selection::LaunchArguments;

/// Nothing to retain: the app's own process is the holder.
pub fn retain_capability() {}

/// Nothing to enter: there is no earliest-main handoff.
pub fn enter() {}

/// The selected game's leases. iOS never has them; mods load in-process instead
/// (wasmtime's Pulley interpreter, since the platform forbids JIT), which needs no capture.
pub fn acquire(selection: &LaunchArguments) -> Option<Capture> {
    let _ = selection;
    None
}