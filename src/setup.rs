//! Legacy setup model retained for reference.
//!
//! The active application represents these choices with `cli::Command` and
//! `manager::Manager`; this module is not currently compiled.

use std::path::PathBuf;

/// Legacy setup operation and optional tool selection.
pub enum Mode {
    /// Install the selected tools.
    Install(Option<Vec<String>>),

    /// Configure the selected tools.
    Conifgure(Option<Vec<String>>),

    /// Install and configure the selected tools.
    InstallAndConfigure(Option<Vec<String>>),
}

/// Legacy owned setup request.
pub struct Setup {
    /// Directory containing dotfile sources.
    pub dotfiles_path: PathBuf,

    /// Selected platform name.
    pub current_os: String,

    /// Operation requested by the user.
    pub selected_mode: Mode,
}
