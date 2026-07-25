use std::process::Command;

pub fn check_executable(executable: &str) -> bool {
    Command::new(executable)
        .status()
        .is_ok()
}
