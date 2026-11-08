//! Win32 adapters. All unsafe Win32 code lives here and in render-d2d. (ADR-0001 §7)
//!
//! Spikes S0–S10 live in `examples/`.

#[cfg(windows)]
pub mod desktop;
#[cfg(windows)]
pub mod dock;
#[cfg(windows)]
pub mod pulse;
#[cfg(windows)]
pub mod session;
#[cfg(windows)]
pub mod winevent;

#[cfg(windows)]
pub use desktop::{emergency_detach, Win32DesktopLayer};
#[cfg(windows)]
pub use dock::{emergency_undock, Win32ShellDock};
#[cfg(windows)]
pub use pulse::Win32Pulses;
#[cfg(windows)]
pub use winevent::Win32WindowTracker;
