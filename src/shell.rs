use std::path::PathBuf;
use std::process::Command;

pub fn check_executable(executable: &str) -> bool {
    Command::new(executable).status().is_ok()
}

pub fn create_symlink(source: &str, target: &str) -> bool {
    symlink::symlink_auto(source, target).is_ok()
}
