use std::path::{Path, PathBuf};

pub enum Mode {
    Install(Option<Vec<String>>),
    Conifgure(Option<Vec<String>>),
    InstallAndConfigure(Option<Vec<String>>),
}

pub struct Setup {
    pub dotfiles_path: PathBuf,
    pub current_os: String,
    pub selected_mode: Mode,
}
