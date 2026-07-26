//! Legacy host-platform detection helpers.
//!
//! This module is not currently wired into the command-line application; the
//! platform is supplied explicitly with `--os`.

/// Returns the legacy manifest platform name inferred at compile time.
///
/// Unsupported operating-system and architecture combinations return `None`.
pub fn get_os() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("windows")
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "x86_64") {
            Some("ubuntu_x64")
        } else if cfg!(target_arch = "aarch64") {
            Some("ubuntu_aarch64")
        } else {
            None
        }
    } else if cfg!(target_os = "macos") {
        Some("macos")
    } else {
        None
    }
}
