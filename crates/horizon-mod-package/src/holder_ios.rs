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

/// The holder's wire marker. Nothing scans for it on iOS (there is no second process to
/// recognise), but `horizon_game::prepare_main_stack` keeps it in the image on every platform
/// and its bytes must stay contiguous, so the constant is the desktop one.
#[used]
pub static CAPABILITY: [u8; 26] = *b"horizon-private-holder-v2;";

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