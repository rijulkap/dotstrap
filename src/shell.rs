//! Cross-platform process, executable lookup, and symlink helpers.
//!
//! Install commands for one tool run in a shared shell process so variables and
//! directory changes can carry from one manifest command to the next. Different
//! tools receive different shell processes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
static REFRESHED_PATH: OnceLock<Mutex<Option<OsString>>> = OnceLock::new();

/// Returns whether an executable file can be found directly or on the search path.
///
/// Unlike invoking the executable as a probe, this check has no side effects.
/// Cargo's conventional binary directory is searched in addition to `PATH`.
pub fn executable_exists(executable: &str) -> bool {
    let executable = Path::new(executable);
    if executable.components().count() > 1 {
        return executable.is_file();
    }

    executable_search_paths().iter().any(|directory| {
        executable_names(executable.as_os_str())
            .iter()
            .any(|name| directory.join(name).is_file())
    })
}

/// Creates a platform-appropriate symlink after expanding a leading home marker.
///
/// Missing parent directories for the target are created automatically.
pub fn create_symlink(source: &str, target: &str) -> Result<(), String> {
    let target =
        expand_home(target).ok_or_else(|| format!("cannot expand home in target `{target}`"))?;
    let source = Path::new(source);

    let source_metadata = std::fs::metadata(source)
        .map_err(|error| format!("cannot inspect source `{}`: {error}", source.display()))?;

    match std::fs::symlink_metadata(&target) {
        Ok(metadata) => {
            let kind = if metadata.file_type().is_symlink() {
                "symbolic link"
            } else if metadata.is_dir() {
                "directory"
            } else {
                "file"
            };
            return Err(format!(
                "target `{}` already exists as a {kind}; move or remove it before configuring",
                target.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect target `{}`: {error}",
                target.display()
            ));
        }
    }

    if let Some(parent) = target.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return Err(format!(
            "cannot create target directory `{}`: {error}",
            parent.display()
        ));
    }

    let result = if source_metadata.is_dir() {
        symlink::symlink_dir(source, &target)
    } else {
        symlink::symlink_file(source, &target)
    };

    result.map_err(|error| {
        format!(
            "cannot link `{}` to `{}`: {error}",
            source.display(),
            target.display()
        )
    })
}

/// Removes a symlink target without deleting ordinary files or directories.
///
/// Returns `Ok(true)` when a link was removed and `Ok(false)` when the target
/// did not exist. A non-symlink target is reported as an error.
pub fn remove_symlink(target: &str) -> Result<bool, String> {
    let target =
        expand_home(target).ok_or_else(|| format!("cannot expand home in target `{target}`"))?;
    let metadata = match std::fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!("cannot inspect `{}`: {error}", target.display()));
        }
    };

    if !metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing to remove `{}` because it is not a symbolic link",
            target.display()
        ));
    }

    remove_symlink_path(&target, &metadata)
        .map_err(|error| format!("cannot remove `{}`: {error}", target.display()))?;
    Ok(true)
}

#[cfg(not(windows))]
/// Removes a Unix symlink, including links which point to directories.
fn remove_symlink_path(target: &Path, _metadata: &std::fs::Metadata) -> Result<(), std::io::Error> {
    std::fs::remove_file(target)
}

#[cfg(windows)]
/// Removes a Windows file or directory symlink using the matching API.
fn remove_symlink_path(target: &Path, metadata: &std::fs::Metadata) -> Result<(), std::io::Error> {
    use std::os::windows::fs::FileTypeExt;

    if metadata.file_type().is_symlink_dir() {
        std::fs::remove_dir(target)
    } else {
        std::fs::remove_file(target)
    }
}

/// Expands `~`, `~/`, or `~\` using the platform's home-directory variable.
fn expand_home(path: &str) -> Option<PathBuf> {
    if path == "~" || path.starts_with("~/") || path.starts_with(r"~\") {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        let suffix = path
            .strip_prefix("~/")
            .or_else(|| path.strip_prefix(r"~\"))
            .unwrap_or("");
        return Some(Path::new(&home).join(suffix));
    }

    Some(PathBuf::from(path))
}

/// Runs an ordered set of commands in one platform-native shell process.
///
/// Unix commands use Bash with pipeline failure detection. PowerShell commands
/// reset and inspect `$LASTEXITCODE` individually. When `show_output` is false,
/// both output streams are discarded.
pub fn run_commands(commands: &[String], show_output: bool) -> Result<(), String> {
    if commands.is_empty() {
        return Ok(());
    }

    let script = if cfg!(windows) {
        build_powershell_script(commands)
    } else {
        build_shell_script(commands)
    };

    let mut process = if cfg!(windows) {
        let mut process = Command::new("powershell");
        process.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        process
    } else {
        let mut process = Command::new("bash");
        process.args(["-c", &script]);
        process
    };

    if let Some(path) = command_path() {
        process.env("PATH", path);
    }

    if !show_output {
        process.stdout(Stdio::null()).stderr(Stdio::null());
    }

    let status = process
        .status()
        .map_err(|error| format!("failed to start shell: {error}"))?;

    if !status.success() {
        return Err(format!("install script failed with status {status}"));
    }

    Ok(())
}

/// Rejects Windows command sets which require an elevated process.
///
/// Chocolatey modifies machine-level state and is therefore treated as
/// requiring administrator privileges. Other platforms need no preflight.
pub fn ensure_command_privileges(commands: &[String]) -> Result<(), String> {
    if !cfg!(windows) || !commands.iter().any(|command| requires_elevation(command)) {
        return Ok(());
    }

    if windows_process_is_elevated()? {
        Ok(())
    } else {
        Err(
            "this install uses Chocolatey and requires an Administrator PowerShell or terminal"
                .to_owned(),
        )
    }
}

/// Requires an elevated administrator process on Windows.
///
/// Other platforms pass this preflight without doing any work.
pub fn ensure_windows_administrator() -> Result<(), String> {
    if windows_process_is_elevated()? {
        Ok(())
    } else {
        Err(
            "dotstrap must be run from an Administrator PowerShell or terminal on Windows"
                .to_owned(),
        )
    }
}

/// Refreshes the effective Windows PATH from machine and user environment data.
///
/// The refreshed value is retained inside the process helpers rather than
/// mutating Rust's global environment. Non-Windows platforms are a no-op.
pub fn refresh_environment_path() -> Result<(), String> {
    refresh_windows_path()
}

/// Returns whether a manifest command is known to require elevation.
fn requires_elevation(command: &str) -> bool {
    let command = command.trim_start().to_ascii_lowercase();
    command == "choco"
        || command.starts_with("choco ")
        || command == "choco.exe"
        || command.starts_with("choco.exe ")
}

#[cfg(windows)]
/// Determines whether the current Windows process belongs to an administrator.
fn windows_process_is_elevated() -> Result<bool, String> {
    let script = concat!(
        "$identity = [Security.Principal.WindowsIdentity]::GetCurrent(); ",
        "$principal = [Security.Principal.WindowsPrincipal]::new($identity); ",
        "$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|error| format!("failed to check Windows administrator privileges: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "failed to check Windows administrator privileges: PowerShell exited with {}",
            output.status
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .eq_ignore_ascii_case("true"))
}

#[cfg(not(windows))]
/// Reports elevation on non-Windows builds, where this preflight is unused.
fn windows_process_is_elevated() -> Result<bool, String> {
    Ok(true)
}

#[cfg(windows)]
/// Reloads persistent machine and user PATH values through PowerShell.
fn refresh_windows_path() -> Result<(), String> {
    let script = concat!(
        "$machine = [Environment]::GetEnvironmentVariable('Path', 'Machine'); ",
        "$user = [Environment]::GetEnvironmentVariable('Path', 'User'); ",
        "$combined = (($machine, $user) | Where-Object { $_ }) -join ';'; ",
        "[Environment]::ExpandEnvironmentVariables($combined)"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|error| format!("failed to refresh Windows PATH: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "failed to refresh Windows PATH: PowerShell exited with {}",
            output.status
        ));
    }

    let persistent_path = String::from_utf8_lossy(&output.stdout);
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(OsStr::new(persistent_path.trim())).collect();

    for path in executable_search_paths() {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    let joined = std::env::join_paths(paths)
        .map_err(|error| format!("failed to assemble refreshed Windows PATH: {error}"))?;
    let cache = REFRESHED_PATH.get_or_init(|| Mutex::new(None));
    *cache
        .lock()
        .map_err(|_| "failed to store refreshed Windows PATH".to_owned())? = Some(joined);
    Ok(())
}

#[cfg(not(windows))]
/// Leaves PATH unchanged on non-Windows builds.
fn refresh_windows_path() -> Result<(), String> {
    Ok(())
}

/// Builds a Bash script which identifies and checks every manifest command.
fn build_shell_script(commands: &[String]) -> String {
    let mut script = String::from("set -o pipefail\n");

    for (index, command) in commands.iter().enumerate() {
        script.push_str(&format!(
            "printf '%s\\n' {} >&2\n",
            shell_quote(&format!(
                "----> command {}/{}: {command}",
                index + 1,
                commands.len()
            )),
        ));
        script.push_str(command);
        script.push('\n');
        script.push_str("status=$?\n");
        script.push_str("if [ \"$status\" -ne 0 ]; then\n");
        script.push_str(&format!(
            "  printf '%s\\n' 'error: command {}/{} failed' >&2\n",
            index + 1,
            commands.len()
        ));
        script.push_str("  exit \"$status\"\n");
        script.push_str("fi\n");
    }

    script
}

/// Builds a PowerShell script which identifies and checks every command.
fn build_powershell_script(commands: &[String]) -> String {
    let mut script = String::from("$ErrorActionPreference = 'Stop'\n");

    for (index, command) in commands.iter().enumerate() {
        script.push_str(&format!(
            "Write-Host '{}'\n",
            format!("----> command {}/{}: {command}", index + 1, commands.len())
                .replace('\'', "''"),
        ));
        script.push_str("$global:LASTEXITCODE = 0\n");
        script.push_str(command);
        script.push('\n');

        script.push_str("if ($LASTEXITCODE -ne 0) {\n");
        script.push_str(&format!(
            "  Write-Error (\"command {}/{} failed with exit code {{0}}\" -f $LASTEXITCODE)\n",
            index + 1,
            commands.len()
        ));
        script.push_str("  exit $LASTEXITCODE\n");
        script.push_str("}\n");
    }

    script
}

/// Quotes diagnostic text as one Bash single-quoted argument.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

/// Returns `PATH` entries plus Cargo's conventional binary directory.
fn executable_search_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = effective_path()
        .as_deref()
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .collect();

    if let Some(cargo_bin) = cargo_bin_dir()
        && !paths.contains(&cargo_bin)
    {
        paths.push(cargo_bin);
    }

    paths
}

/// Returns the refreshed Windows PATH when available, otherwise the process PATH.
fn effective_path() -> Option<OsString> {
    #[cfg(windows)]
    if let Some(cache) = REFRESHED_PATH.get()
        && let Ok(path) = cache.lock()
        && let Some(path) = path.as_ref()
    {
        return Some(path.clone());
    }

    std::env::var_os("PATH")
}

/// Produces platform-appropriate filename candidates for an executable.
fn executable_names(executable: &OsStr) -> Vec<OsString> {
    if !cfg!(windows) || Path::new(executable).extension().is_some() {
        return vec![executable.to_owned()];
    }

    std::env::var_os("PATHEXT")
        .unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"))
        .to_string_lossy()
        .split(';')
        .map(|extension| {
            let mut name = executable.to_owned();
            name.push(extension);
            name
        })
        .collect()
}

/// Joins the augmented executable search path for a child process.
fn command_path() -> Option<OsString> {
    std::env::join_paths(executable_search_paths()).ok()
}

/// Returns the conventional Cargo binary directory for the current user.
fn cargo_bin_dir() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(Path::new(&home).join(".cargo").join("bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_an_executable_on_path() {
        let executable = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(executable_exists(executable));
    }

    #[test]
    fn rejects_a_missing_executable() {
        assert!(!executable_exists(
            "dotstrap-test-executable-that-does-not-exist"
        ));
    }

    #[test]
    fn reports_an_existing_symlink_target() {
        let directory = std::env::temp_dir().join(format!(
            "dotstrap-existing-target-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source");
        let target = directory.join("target");
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&target, "existing").unwrap();

        let error = create_symlink(source.to_str().unwrap(), target.to_str().unwrap()).unwrap_err();
        assert!(error.contains("already exists as a file"));
        assert!(error.contains(&target.display().to_string()));

        std::fs::remove_file(target).unwrap();
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn reports_a_failed_command() {
        let commands = vec!["true".to_owned(), "false".to_owned()];
        assert!(run_commands(&commands, false).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn detects_failure_inside_a_pipeline() {
        let commands = vec!["false | true".to_owned()];
        assert!(run_commands(&commands, false).is_err());
    }

    #[test]
    fn identifies_chocolatey_commands_as_elevated() {
        assert!(requires_elevation("choco install git -y"));
        assert!(requires_elevation("  CHOCO.EXE upgrade git"));
        assert!(!requires_elevation("cargo install tree-sitter-cli"));
    }

    #[cfg(unix)]
    #[test]
    fn removes_only_symbolic_links() {
        use std::os::unix::fs::symlink;

        let directory = std::env::temp_dir().join(format!(
            "dotstrap-remove-symlink-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source");
        let link = directory.join("link");
        std::fs::write(&source, "test").unwrap();
        symlink(&source, &link).unwrap();

        assert!(remove_symlink(link.to_str().unwrap()).unwrap());
        assert!(!link.exists());
        assert!(source.exists());
        assert!(remove_symlink(link.to_str().unwrap()).is_ok_and(|removed| !removed));
        assert!(
            remove_symlink(source.to_str().unwrap())
                .unwrap_err()
                .contains("not a symbolic link")
        );

        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
