//! Command-line argument model for dotstrap.
//!
//! This module intentionally contains only the user-facing Clap types.
//! Request validation lives in [`validation`], while tool selection and
//! dependency ordering live in [`planner`].

mod planner;
mod validation;

use clap::{Args, Parser, Subcommand};

use crate::dotfile::{Manifest, load_manifest};

#[derive(Debug, Parser)]
#[command(
    name = "dotman",
    version,
    about = "Install and configure dotfiles and development tools"
)]
/// Complete command-line request accepted by dotstrap.
pub struct DotManager {
    /// Parsed manifest loaded from the path supplied on the command line.
    #[arg(
        short,
        long,
        value_name = "FILE",
        value_parser = load_manifest
    )]
    pub manifest: Manifest,

    /// Manifest platform key used to select install commands and config targets.
    #[arg(short, long, value_name = "OS")]
    pub os: String,

    /// Prints additional diagnostic information.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Forces directly selected tools, but not their dependencies.
    #[arg(short, long, global = true, conflicts_with = "force_all")]
    pub force: bool,

    /// Forces directly selected tools and their complete dependency chains.
    #[arg(long, global = true, conflicts_with = "force")]
    pub force_all: bool,

    /// Operation to perform after validation.
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
/// Operations supported by the command-line interface.
pub enum Command {
    /// Installs tools.
    Install(SelectionArgs),

    /// Applies configuration files.
    Configure(SelectionArgs),

    /// Installs tools and then applies their configurations.
    InstallAndConfigure(SelectionArgs),

    /// Removes configuration symlinks without deleting regular files.
    RemoveSymlinks(SelectionArgs),

    /// Validates the manifest without making changes.
    Validate,
}

#[derive(Debug, Args)]
/// Optional tool and tag filters shared by actionable commands.
pub struct SelectionArgs {
    /// Installs or configures specific tools.
    #[arg(short, long, value_name = "TOOL", value_delimiter = ',')]
    pub tools: Vec<String>,

    /// Selects tools with any of these tags.
    #[arg(short = 'g', long, value_name = "TAG", value_delimiter = ',')]
    pub tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::*;

    #[test]
    fn parses_install_command() {
        let cli = DotManager::try_parse_from([
            "dotman",
            "--manifest",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("schema.toml")
                .to_str()
                .unwrap(),
            "--os",
            "windows_x64",
            "install",
            "--tools",
            "git,neovim",
        ])
        .unwrap();

        match cli.command {
            Command::Install(args) => {
                assert_eq!(args.tools, vec!["git", "neovim"]);
            }
            _ => panic!("expected install command"),
        }
    }

    #[test]
    fn parses_remove_symlinks_command() {
        let cli = DotManager::try_parse_from([
            "dotman",
            "--manifest",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("schema.toml")
                .to_str()
                .unwrap(),
            "--os",
            "linux_x64",
            "remove-symlinks",
            "--tools",
            "git",
        ])
        .unwrap();

        match cli.command {
            Command::RemoveSymlinks(args) => assert_eq!(args.tools, vec!["git"]),
            _ => panic!("expected remove-symlinks command"),
        }
    }

    #[test]
    fn parses_global_force_flag() {
        let cli = DotManager::try_parse_from([
            "dotman",
            "--manifest",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("schema.toml")
                .to_str()
                .unwrap(),
            "--os",
            "linux_x64",
            "install",
            "--force",
            "--tools",
            "git",
        ])
        .unwrap();

        assert!(cli.force);
        assert!(!cli.force_all);
    }

    #[test]
    fn parses_force_all_and_rejects_both_force_modes() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema.toml");
        let base = [
            "dotman",
            "--manifest",
            manifest.to_str().unwrap(),
            "--os",
            "linux_x64",
            "install",
        ];

        let cli =
            DotManager::try_parse_from(base.into_iter().chain(["--force-all", "--tools", "git"]))
                .unwrap();
        assert!(cli.force_all);
        assert!(!cli.force);

        assert!(
            DotManager::try_parse_from(base.into_iter().chain(["--force", "--force-all"])).is_err()
        );
    }
}
