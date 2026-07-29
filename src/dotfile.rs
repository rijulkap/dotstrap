//! Deserializable data model for the TOML manifest.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
/// Root of a versioned dotstrap manifest.
pub struct Manifest {
    /// Manifest schema version. The current application accepts version `1`.
    pub version: u32,

    /// Package managers indexed by a user-defined name.
    pub package_managers: HashMap<String, PackageManager>,

    /// Installable and configurable tools indexed by tool name.
    pub tools: HashMap<String, Tool>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
/// Package manager availability definition for one platform.
pub struct PackageManager {
    /// Optional human-readable description.
    pub description: Option<String>,

    /// Platform key for which this package manager is applicable.
    pub platform: String,

    /// Executable whose presence indicates that the manager is available.
    pub check: Option<String>,

    /// Optional remediation shown when the availability check fails.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
/// Installation and configuration definition for one tool.
pub struct Tool {
    /// Optional human-readable description.
    pub description: Option<String>,

    /// Other tools which must be processed before this tool.
    pub deps: Option<Vec<String>>,

    /// Labels used to select groups of tools from the command line.
    pub tags: Option<Vec<String>>,

    /// Executable check shared across platforms or selected by platform.
    pub check: Option<ToolCheck>,

    /// Ordered shell commands indexed by platform key.
    pub install: Option<HashMap<String, Vec<String>>>,

    /// Optional configuration source and platform-specific link targets.
    pub configs: Option<Config>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
/// Tool availability check in shared or platform-specific form.
pub enum ToolCheck {
    /// One executable name used on every platform.
    Command(String),

    /// Executable names indexed by manifest platform key.
    ByPlatform(HashMap<String, String>),
}

impl ToolCheck {
    /// Returns the executable check applicable to a platform, if configured.
    pub fn for_platform(&self, platform: &str) -> Option<&str> {
        match self {
            Self::Command(command) => Some(command),
            Self::ByPlatform(commands) => commands.get(platform).map(String::as_str),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
/// A configuration source linked to a platform-specific target.
pub struct Config {
    /// Optional human-readable description.
    pub description: Option<String>,

    /// Source file or directory, relative to the manifest unless absolute.
    pub source: String,

    /// Symlink destinations indexed by platform key.
    pub targets: HashMap<String, String>,
}

/// Reads a TOML manifest and anchors relative config sources to its directory.
///
/// The manifest path is canonicalized first, making resolved sources independent
/// of the process working directory after loading.
pub fn load_manifest(path: &str) -> Result<Manifest, String> {
    let requested_path = PathBuf::from(path);
    let path = fs::canonicalize(&requested_path).map_err(|error| {
        format!(
            "failed to resolve manifest path {}: {error}",
            requested_path.display()
        )
    })?;
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut manifest: Manifest = toml::from_str(&text)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;

    let manifest_dir = path
        .parent()
        .expect("a canonical manifest path should always have a parent");

    for tool in manifest.tools.values_mut() {
        let Some(config) = tool.configs.as_mut() else {
            continue;
        };
        let source = Path::new(&config.source);
        if source.is_relative() {
            config.source = manifest_dir.join(source).to_string_lossy().into_owned();
        }
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn schema_fixture_covers_manifest_fields() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema.toml");
        let text = fs::read_to_string(path).unwrap();
        let manifest = toml::from_str::<Manifest>(&text).unwrap();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.package_managers.len(), 2);
        assert_eq!(manifest.tools.len(), 5);

        let apt = &manifest.package_managers["apt"];
        assert_eq!(apt.platform, "linux_x64");
        assert_eq!(apt.check.as_deref(), Some("apt-get"));
        assert!(apt.description.is_some());
        assert!(apt.hint.is_some());

        let compiler = &manifest.tools["compiler"];
        assert!(compiler.description.is_none());
        assert!(compiler.install.is_none());
        assert!(compiler.configs.is_none());

        let git = &manifest.tools["git"];
        assert_eq!(git.deps.as_deref().unwrap(), ["rust"]);
        assert!(git.tags.as_ref().unwrap().contains(&"core".to_owned()));
        assert_eq!(
            git.check
                .as_ref()
                .and_then(|check| check.for_platform("linux_x64")),
            Some("git")
        );
        assert_eq!(git.install.as_ref().unwrap()["linux_x64"].len(), 2);

        let config = git.configs.as_ref().unwrap();
        assert_eq!(config.source, "git/.gitconfig");
        assert_eq!(config.targets["windows_x64"], "~/.gitconfig");

        let neovim = &manifest.tools["neovim"];
        assert_eq!(neovim.deps.as_deref().unwrap(), ["git", "rust"]);

        let fd = &manifest.tools["fd"];
        let fd_check = fd.check.as_ref().unwrap();
        assert_eq!(fd_check.for_platform("linux_x64"), Some("fdfind"));
        assert_eq!(fd_check.for_platform("windows_x64"), Some("fd"));
        assert_eq!(fd_check.for_platform("freebsd_x64"), None);
    }

    #[test]
    fn load_manifest_anchors_relative_config_sources() {
        let directory = std::env::temp_dir().join(format!(
            "dotstrap-load-manifest-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("manifest.toml");
        fs::write(
            &path,
            r#"
                version = 1
                [package_managers]
                [tools.git]
                [tools.git.configs]
                source = "git/.gitconfig"
                [tools.git.configs.targets]
                linux_x64 = "~/.gitconfig"
            "#,
        )
        .unwrap();

        let manifest = load_manifest(path.to_str().unwrap()).unwrap();
        let canonical_directory = fs::canonicalize(&directory).unwrap();
        assert_eq!(
            PathBuf::from(&manifest.tools["git"].configs.as_ref().unwrap().source),
            canonical_directory.join("git/.gitconfig")
        );
        assert!(Path::new(&manifest.tools["git"].configs.as_ref().unwrap().source).is_absolute());

        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
