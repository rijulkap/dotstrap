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
