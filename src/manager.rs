//! Execution of validated installation and configuration plans.

use crate::{
    cli::Command,
    dotfile::{Manifest, Tool},
    shell::{
        create_symlink, ensure_command_privileges, executable_exists, refresh_environment_path,
        remove_symlink, run_commands,
    },
};

/// Borrowed, validated execution plan for a single CLI request.
#[derive(Debug)]
pub struct Manager<'a> {
    /// Platform key used to select commands and configuration targets.
    pub os: &'a str,

    /// Manifest that owns all referenced tool definitions.
    pub manifest: &'a Manifest,

    /// Dependency-ordered tool names to process.
    pub tool_chain: Vec<&'a str>,

    /// Requested operation to apply to every tool in the chain.
    pub selected_command: &'a Command,

    /// Whether normal installed-tool and existing-link checks are bypassed.
    pub force: bool,
}

impl<'a> Manager<'a> {
    /// Executes the selected operation for each tool in dependency order.
    ///
    /// Processing stops at the first error so later tools never run after a
    /// failed prerequisite.
    pub fn execute(&self) -> Result<(), String> {
        if matches!(self.selected_command, Command::Validate) {
            return Ok(());
        }

        for &tool_name in &self.tool_chain {
            let tool = self
                .manifest
                .tools
                .get(tool_name)
                .expect("tool chain should contain only validated tools");

            println!("----> Processing `{tool_name}`");

            match self.selected_command {
                Command::Install(_) => {
                    self.install_tool(tool_name, tool)?;
                }

                Command::Configure(_) => {
                    self.configure_tool(tool_name, tool)?;
                }

                Command::InstallAndConfigure(_) => {
                    self.install_tool(tool_name, tool)?;
                    self.configure_tool(tool_name, tool)?;
                }

                Command::RemoveSymlinks(_) => {
                    self.remove_tool_symlink(tool_name, tool)?;
                }

                Command::Validate => unreachable!(),
            }
        }

        Ok(())
    }

    /// Installs a tool unless its check executable is already available.
    fn install_tool(&self, tool_name: &str, tool: &Tool) -> Result<(), String> {
        if !self.force
            && let Some(check) = tool.check.as_deref()
            && executable_exists(check)
        {
            println!("----> Skipping `{tool_name}` (`{check}` is already available)");
            return Ok(());
        }

        let Some(install_commands) = tool.install.as_ref() else {
            return Ok(());
        };

        let Some(commands) = install_commands.get(self.os) else {
            return Ok(());
        };

        ensure_command_privileges(commands)
            .map_err(|error| format!("cannot install `{tool_name}`: {error}"))?;

        run_commands(commands, true)
            .map_err(|error| format!("failed installing `{tool_name}`: {error}"))?;

        refresh_environment_path()
            .map_err(|error| format!("installed `{tool_name}`, but {error}"))?;

        if !self.force
            && let Some(check) = tool.check.as_deref()
            && !executable_exists(check)
        {
            return Err(format!(
                "installed `{tool_name}`, but its check executable `{check}` was not found"
            ));
        }

        Ok(())
    }

    /// Links a tool's configuration source to the current platform target.
    fn configure_tool(&self, tool_name: &str, tool: &Tool) -> Result<(), String> {
        let Some(config) = tool.configs.as_ref() else {
            return Ok(());
        };

        let Some(target) = config.targets.get(self.os) else {
            return Ok(());
        };

        if self.force {
            remove_symlink(target).map_err(|error| {
                format!("cannot replace configuration target for `{tool_name}`: {error}")
            })?;
        }

        println!("Linking `{}` -> `{target}`", config.source);

        if !create_symlink(&config.source, target) {
            return Err(format!(
                "failed to create symlink for tool `{tool_name}`: `{}` -> `{target}`",
                config.source
            ));
        }

        Ok(())
    }

    /// Removes a tool's configuration target when it is a symbolic link.
    fn remove_tool_symlink(&self, tool_name: &str, tool: &Tool) -> Result<(), String> {
        let Some(config) = tool.configs.as_ref() else {
            return Ok(());
        };
        let Some(target) = config.targets.get(self.os) else {
            return Ok(());
        };

        match remove_symlink(target)
            .map_err(|error| format!("failed removing symlink for `{tool_name}`: {error}"))?
        {
            true => println!("Removed symlink `{target}`"),
            false => println!("----> No symlink to remove for `{tool_name}`"),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::dotfile::Config;

    use super::*;

    fn empty_manifest() -> Manifest {
        Manifest {
            version: 1,
            package_managers: HashMap::new(),
            tools: HashMap::new(),
        }
    }

    fn tool(check: Option<&str>, install: Option<Vec<String>>, config: Option<Config>) -> Tool {
        Tool {
            description: None,
            deps: None,
            tags: None,
            check: check.map(ToOwned::to_owned),
            install: install.map(|commands| HashMap::from([("linux_x64".to_owned(), commands)])),
            configs: config,
        }
    }

    #[cfg(unix)]
    #[test]
    fn force_install_bypasses_existing_executable_check() {
        let marker = std::env::temp_dir().join(format!(
            "dotstrap-force-install-test-{}",
            std::process::id()
        ));
        let command = format!("touch '{}'", marker.display());
        let tool = tool(Some("sh"), Some(vec![command]), None);
        let manifest = empty_manifest();
        let selected_command = Command::Install(crate::cli::SelectionArgs {
            tools: Vec::new(),
            tags: Vec::new(),
        });

        let manager = Manager {
            os: "linux_x64",
            manifest: &manifest,
            tool_chain: Vec::new(),
            selected_command: &selected_command,
            force: false,
        };
        manager.install_tool("test", &tool).unwrap();
        assert!(!marker.exists());

        let forced_manager = Manager {
            force: true,
            ..manager
        };
        forced_manager.install_tool("test", &tool).unwrap();
        assert!(marker.exists());

        std::fs::remove_file(marker).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn force_configure_replaces_symlink_but_normal_configure_does_not() {
        use std::os::unix::fs::symlink;

        let directory = std::env::temp_dir().join(format!(
            "dotstrap-force-configure-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let old_source = directory.join("old");
        let new_source = directory.join("new");
        let target = directory.join("target");
        std::fs::write(&old_source, "old").unwrap();
        std::fs::write(&new_source, "new").unwrap();
        symlink(&old_source, &target).unwrap();

        let tool = tool(
            None,
            None,
            Some(Config {
                description: None,
                source: new_source.to_string_lossy().into_owned(),
                targets: HashMap::from([(
                    "linux_x64".to_owned(),
                    target.to_string_lossy().into_owned(),
                )]),
            }),
        );
        let manifest = empty_manifest();
        let selected_command = Command::Configure(crate::cli::SelectionArgs {
            tools: Vec::new(),
            tags: Vec::new(),
        });
        let manager = Manager {
            os: "linux_x64",
            manifest: &manifest,
            tool_chain: Vec::new(),
            selected_command: &selected_command,
            force: false,
        };

        assert!(manager.configure_tool("test", &tool).is_err());
        assert_eq!(std::fs::read_link(&target).unwrap(), old_source);

        let forced_manager = Manager {
            force: true,
            ..manager
        };
        forced_manager.configure_tool("test", &tool).unwrap();
        assert_eq!(std::fs::read_link(&target).unwrap(), new_source);

        std::fs::remove_file(target).unwrap();
        std::fs::remove_file(old_source).unwrap();
        std::fs::remove_file(new_source).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
