//! Validation of complete CLI requests and construction of execution plans.

use std::path::Path;

use crate::manager::Manager;
use crate::shell::executable_exists;

use super::planner::ToolPlanner;
use super::{Command, DotManager, SelectionArgs};

impl DotManager {
    /// Validates the request and builds an executable manager when appropriate.
    ///
    /// Validation-only requests return `Ok(None)`. Actionable requests return a
    /// manager containing a dependency-ordered tool chain.
    pub fn validate(&self) -> Result<Option<Manager<'_>>, String> {
        self.validate_manifest_version()?;
        let planner = ToolPlanner::new(&self.manifest);

        match &self.command {
            Command::Install(args) => {
                self.validate_package_manager()?;
                self.manager_for(args, &planner).map(Some)
            }
            Command::Configure(args) => {
                let manager = self.manager_for(args, &planner)?;
                self.validate_config_sources(&manager.tool_chain)?;
                Ok(Some(manager))
            }
            Command::InstallAndConfigure(args) => {
                self.validate_package_manager()?;
                let manager = self.manager_for(args, &planner)?;
                self.validate_config_sources(&manager.tool_chain)?;
                Ok(Some(manager))
            }
            Command::RemoveSymlinks(args) => self.manager_for(args, &planner).map(Some),
            Command::Validate => {
                planner.validate_all()?;
                let all_tools: Vec<&str> = self.manifest.tools.keys().map(String::as_str).collect();
                self.validate_config_sources(&all_tools)?;
                Ok(None)
            }
        }
    }

    /// Builds a manager from a validated, dependency-ordered selection.
    fn manager_for<'a>(
        &'a self,
        args: &SelectionArgs,
        planner: &ToolPlanner<'a>,
    ) -> Result<Manager<'a>, String> {
        let plan = planner.plan(args)?;
        Ok(Manager {
            os: &self.os,
            manifest: &self.manifest,
            tool_chain: plan.tool_chain,
            selected_tools: plan.selected_tools,
            selected_command: &self.command,
            force: self.force,
            force_all: self.force_all,
        })
    }

    /// Ensures the manifest uses the schema version supported by this binary.
    fn validate_manifest_version(&self) -> Result<(), String> {
        if self.manifest.version == 1 {
            Ok(())
        } else {
            Err(format!(
                "unsupported manifest version {}; expected version 1",
                self.manifest.version
            ))
        }
    }

    /// Ensures all named tools have existing configuration sources.
    fn validate_config_sources(&self, tool_names: &[&str]) -> Result<(), String> {
        for tool_name in tool_names {
            let tool = self
                .manifest
                .tools
                .get(*tool_name)
                .ok_or_else(|| format!("tool `{tool_name}` does not exist in the manifest"))?;

            if let Some(config) = &tool.configs {
                let source = Path::new(&config.source);
                if !source.exists() {
                    return Err(format!(
                        "configuration source for tool `{tool_name}` does not exist: {}",
                        source.display()
                    ));
                }
            }
        }
        Ok(())
    }

    /// Ensures a configured package manager for the platform is available.
    fn validate_package_manager(&self) -> Result<(), String> {
        let managers: Vec<_> = self
            .manifest
            .package_managers
            .iter()
            .filter(|(_, manager)| manager.platform == self.os)
            .collect();

        if managers.is_empty() {
            return Err(format!(
                "operating system '{}' has no configured package manager",
                self.os
            ));
        }

        let mut errors = Vec::new();
        for (name, manager) in managers {
            let Some(check) = &manager.check else {
                continue;
            };
            if executable_exists(check) {
                return Ok(());
            }

            let hint = manager
                .hint
                .as_deref()
                .map(|hint| format!(" ({hint})"))
                .unwrap_or_default();
            errors.push(format!("`{name}` failed check `{check}`{hint}"));
        }

        if errors.is_empty() {
            Err(format!(
                "no package manager check is defined for '{}'",
                self.os
            ))
        } else {
            Err(format!(
                "no available package manager found for '{}': {}",
                self.os,
                errors.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::dotfile::{Config, Manifest, Tool};

    use super::*;

    fn tool_with_source(source: &str) -> Tool {
        Tool {
            description: None,
            deps: None,
            tags: None,
            check: None,
            install: None,
            configs: Some(Config {
                description: None,
                source: source.to_owned(),
                targets: HashMap::from([("linux_x64".to_owned(), "~/.config/test".to_owned())]),
            }),
        }
    }

    fn cli(version: u32, command: Command, tool: Tool) -> DotManager {
        DotManager {
            manifest: Manifest {
                version,
                package_managers: HashMap::new(),
                tools: HashMap::from([("test".to_owned(), tool)]),
            },
            os: "linux_x64".to_owned(),
            verbose: false,
            force: false,
            force_all: false,
            command,
        }
    }

    fn selection() -> SelectionArgs {
        SelectionArgs {
            tools: vec!["test".to_owned()],
            tags: Vec::new(),
        }
    }

    #[test]
    fn rejects_unsupported_manifest_version() {
        let cli = cli(2, Command::Validate, tool_with_source("."));
        assert_eq!(
            cli.validate().unwrap_err(),
            "unsupported manifest version 2; expected version 1"
        );
    }

    #[test]
    fn configure_requires_source_to_exist() {
        let cli = cli(
            1,
            Command::Configure(selection()),
            tool_with_source("/dotstrap/test/path/that/does/not/exist"),
        );
        assert!(
            cli.validate()
                .unwrap_err()
                .contains("configuration source for tool `test` does not exist")
        );
    }

    #[test]
    fn remove_symlinks_does_not_require_source_to_exist() {
        let cli = cli(
            1,
            Command::RemoveSymlinks(selection()),
            tool_with_source("/dotstrap/test/path/that/does/not/exist"),
        );
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn install_requires_a_package_manager_for_the_platform() {
        let cli = cli(1, Command::Install(selection()), tool_with_source("."));
        assert_eq!(
            cli.validate().unwrap_err(),
            "operating system 'linux_x64' has no configured package manager"
        );
    }
}
