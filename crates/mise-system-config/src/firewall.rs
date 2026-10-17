//! `[bootstrap.linux.firewall]`. Only Linux interprets it, so other platforms keep the section
//! as raw values: one config can be shared across hosts.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(target_os = "linux"))]
mod other;
#[cfg(not(target_os = "linux"))]
pub use other::*;
