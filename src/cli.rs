use clap::{Args, Parser, Subcommand};
use std::collections::HashSet;
use std::path::Path;
use std::{fs, path::PathBuf};

use crate::dotfile::Manifest;
use crate::shell::check_executable;

#[derive(Debug, Parser)]
#[command(
    name = "dotman",
    version,
    about = "Install and configure dotfiles and development tools"
)]
pub struct DotManager {
    #[arg(
        short,
        long,
        value_name = "FILE",
        value_parser = parse_manifest
    )]
    pub manifest: Manifest,

    #[arg(short, long, value_name = "OS")]
    pub os: String,

    /// Print additional diagnostic information
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install tools
    Install(SelectionArgs),

    /// Apply configuration files
    Configure(SelectionArgs),

    /// Install tools and apply their configurations
    InstallAndConfigure(SelectionArgs),

    /// Validate the manifest without making changes
    Validate,
}

#[derive(Debug, Args)]
pub struct SelectionArgs {
    /// Install or configure specific tools
    #[arg(short, long, value_name = "TOOL", value_delimiter = ',')]
    pub tools: Vec<String>,

    /// Select tools with any of these tags
    #[arg(short = 'g', long, value_name = "TAG", value_delimiter = ',')]
    pub tags: Vec<String>,
}

impl DotManager {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_os_has_pkgmgr()?;

        match &self.command {
            Command::Install(args) => {
                self.validate_os_has_pkgmgr()?;
                self.validate_pkgmgr_is_exec()?;
                self.validate_selection(args)?;
            }

            Command::Configure(args) => {
                self.validate_selection(args)?;
                self.validate_selected_config_sources(args)?;
            }

            Command::InstallAndConfigure(args) => {
                self.validate_os_has_pkgmgr()?;
                self.validate_pkgmgr_is_exec()?;
                self.validate_selection(args)?;
                self.validate_selected_config_sources(args)?;
            }

            Command::Validate => {
                self.validate_all_manifest_dependencies()?;
                self.validate_all_config_sources()?;
            }
        }

        Ok(())
    }

    fn selected_tools<'a>(&'a self, args: &'a SelectionArgs) -> HashSet<&'a str> {
        if args.tools.is_empty() && args.tags.is_empty() {
            return self.manifest.tools.keys().map(String::as_str).collect();
        }

        let mut selected: HashSet<&str> = args.tools.iter().map(String::as_str).collect();

        for (tool_name, tool) in &self.manifest.tools {
            let matches_tag = args
                .tags
                .iter()
                .any(|tag| tool.tags.as_ref().is_some_and(|tags| tags.contains(tag)));

            if matches_tag {
                selected.insert(tool_name.as_str());
            }
        }

        selected
    }

    fn resolve_tools<'a>(&'a self, args: &'a SelectionArgs) -> Result<HashSet<&'a str>, String> {
        let mut resolved = HashSet::new();

        for tool in self.selected_tools(args) {
            self.resolve_tool(tool, &mut resolved)?;
        }

        Ok(resolved)
    }

    fn resolve_tool<'a>(
        &'a self,
        tool_name: &'a str,
        resolved: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        if !resolved.insert(tool_name) {
            return Ok(());
        }

        let tool = self
            .manifest
            .tools
            .get(tool_name)
            .ok_or_else(|| format!("tool `{tool_name}` does not exist"))?;

        for dependency in tool.deps.as_deref().unwrap_or_default() {
            self.resolve_tool(dependency, resolved)?;
        }

        Ok(())
    }

    fn validate_selection(&self, args: &SelectionArgs) -> Result<(), String> {
        self.validate_selected_tools_in_manifest(args)?;
        self.validate_selected_tags_in_manifest(args)?;

        let selected_tools = self.selected_tools(args);

        self.validate_dependencies_exist(&selected_tools)?;
        self.validate_no_dependency_cycles(&selected_tools)?;

        Ok(())
    }

    fn validate_tool_config_source(&self, tool_name: &str) -> Result<(), String> {
        let tool = self
            .manifest
            .tools
            .get(tool_name)
            .ok_or_else(|| format!("tool `{tool_name}` does not exist in the manifest"))?;

        let Some(config) = &tool.configs else {
            return Ok(());
        };

        let source = Path::new(&config.source);

        if !source.exists() {
            return Err(format!(
                "configuration source for tool `{tool_name}` does not exist: {}",
                source.display()
            ));
        }

        Ok(())
    }

    fn validate_all_config_sources(&self) -> Result<(), String> {
        for tool_name in self.manifest.tools.keys() {
            self.validate_tool_config_source(tool_name)?;
        }

        Ok(())
    }

    fn validate_selected_config_sources(&self, args: &SelectionArgs) -> Result<(), String> {
        for tool_name in self.resolve_selected_tools(args) {
            self.validate_tool_config_source(tool_name)?;
        }

        Ok(())
    }

    fn validate_all_manifest_dependencies(&self) -> Result<(), String> {
        let all_tools: HashSet<&str> = self.manifest.tools.keys().map(String::as_str).collect();

        self.validate_dependencies_exist(&all_tools)?;
        self.validate_no_dependency_cycles(&all_tools)?;

        Ok(())
    }

    fn validate_dependencies_exist(&self, selected_tools: &HashSet<&str>) -> Result<(), String> {
        let mut visited = HashSet::new();

        for tool_name in selected_tools {
            self.validate_tool_dependencies_exist(tool_name, &mut visited)?;
        }

        Ok(())
    }

    fn validate_no_dependency_cycles(&self, selected_tools: &HashSet<&str>) -> Result<(), String> {
        let mut fully_visited = HashSet::new();
        let mut current_path = Vec::new();

        for tool_name in selected_tools {
            self.visit_dependency(tool_name, &mut fully_visited, &mut current_path)?;
        }

        Ok(())
    }

    fn visit_dependency(
        &self,
        tool_name: &str,
        fully_visited: &mut HashSet<String>,
        current_path: &mut Vec<String>,
    ) -> Result<(), String> {
        if let Some(cycle_start) = current_path.iter().position(|name| name == tool_name) {
            let mut cycle = current_path[cycle_start..].to_vec();
            cycle.push(tool_name.to_owned());

            return Err(format!(
                "circular dependency detected: {}",
                cycle.join(" -> ")
            ));
        }

        if fully_visited.contains(tool_name) {
            return Ok(());
        }

        let tool = self
            .manifest
            .tools
            .get(tool_name)
            .ok_or_else(|| format!("tool `{tool_name}` does not exist in the manifest"))?;

        current_path.push(tool_name.to_owned());

        for dependency in tool.deps.as_deref().unwrap_or_default() {
            self.visit_dependency(dependency, fully_visited, current_path)?;
        }

        current_path.pop();
        fully_visited.insert(tool_name.to_owned());

        Ok(())
    }

    fn validate_tool_dependencies_exist(
        &self,
        tool_name: &str,
        visited: &mut HashSet<String>,
    ) -> Result<(), String> {
        if !visited.insert(tool_name.to_owned()) {
            return Ok(());
        }

        let tool = self
            .manifest
            .tools
            .get(tool_name)
            .ok_or_else(|| format!("tool `{tool_name}` does not exist in the manifest"))?;

        for dependency in tool.deps.as_deref().unwrap_or_default() {
            if !self.manifest.tools.contains_key(dependency) {
                return Err(format!(
                    "tool `{tool_name}` depends on unknown tool `{dependency}`"
                ));
            }

            self.validate_tool_dependencies_exist(dependency, visited)?;
        }

        Ok(())
    }

    fn resolve_selected_tools<'a>(&'a self, args: &'a SelectionArgs) -> HashSet<&'a str> {
        if args.tools.is_empty() && args.tags.is_empty() {
            return self.manifest.tools.keys().map(String::as_str).collect();
        }

        let mut selected: HashSet<&str> = args.tools.iter().map(String::as_str).collect();

        for (tool_name, tool) in &self.manifest.tools {
            let matches_tag = args
                .tags
                .iter()
                .any(|tag| tool.tags.as_ref().is_some_and(|tags| tags.contains(tag)));

            if matches_tag {
                selected.insert(tool_name.as_str());
            }
        }

        selected
    }

    fn validate_pkgmgr_is_exec(&self) -> Result<(), String> {
        let package_managers = self
            .manifest
            .package_managers
            .iter()
            .filter(|(_, manager)| manager.platform == self.os);

        let mut errors = Vec::new();

        for (name, manager) in package_managers {
            let Some(check) = &manager.check else {
                continue;
            };

            let status = check_executable(check);

            if status {
                return Ok(());
            }

            errors.push(format!("`{name}` failed check `{check}`"));
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

    fn validate_os_has_pkgmgr(&self) -> Result<(), String> {
        if self
            .manifest
            .package_managers
            .values()
            .any(|manager| manager.platform == self.os)
        {
            Ok(())
        } else {
            Err(format!(
                "operating system '{}' has no configured package manager",
                self.os
            ))
        }
    }

    fn validate_selected_tags_in_manifest(&self, args: &SelectionArgs) -> Result<(), String> {
        for tag_name in &args.tags {
            let mut found = false;

            for tool in self.manifest.tools.values() {
                if let Some(tags) = &tool.tags {
                    if tags.contains(tag_name) {
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                return Err(format!("tag `{tag_name}` does not exist in the manifest"));
            }
        }

        Ok(())
    }

    fn validate_selected_tools_in_manifest(&self, args: &SelectionArgs) -> Result<(), String> {
        for tool_name in &args.tools {
            if !self.manifest.tools.contains_key(tool_name) {
                return Err(format!("tool `{tool_name}` does not exist in the manifest"));
            }
        }

        Ok(())
    }
}

fn parse_manifest(path: &str) -> Result<Manifest, String> {
    let path = PathBuf::from(path);

    let text = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

    toml::from_str(&text).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

fn make_cli(tools_toml: &str, command: Command) -> DotManager {
    let manifest_toml = format!(
        r#"
            version = 1

            [package_managers]

            {tools_toml}
        "#
    );

    DotManager {
        manifest: toml::from_str(&manifest_toml).unwrap(),
        os: "windows_x64".to_owned(),
        verbose: false,
        command,
    }
}

    fn selection(tools: &[&str], tags: &[&str]) -> SelectionArgs {
        SelectionArgs {
            tools: tools.iter().map(|value| value.to_string()).collect(),
            tags: tags.iter().map(|value| value.to_string()).collect(),
        }
    }

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
    fn empty_selection_selects_all_tools() {
        let cli = make_cli(
            r#"
                [tools.git]
                [tools.rust]
                [tools.neovim]
            "#,
            Command::Install(selection(&[], &[])),
        );

        let args = selection(&[], &[]);
        let selected = cli.selected_tools(&args);

        assert_eq!(selected.len(), 3);
        assert!(selected.contains("git"));
        assert!(selected.contains("rust"));
        assert!(selected.contains("neovim"));
    }

    #[test]
    fn selects_explicit_tools() {
        let cli = make_cli(
            r#"
                [tools.git]
                [tools.rust]
                [tools.neovim]
            "#,
            Command::Install(selection(&["git", "neovim"], &[])),
        );

        let args = selection(&["git", "neovim"], &[]);
        let selected = cli.selected_tools(&args);

        assert_eq!(selected.len(), 2);
        assert!(selected.contains("git"));
        assert!(selected.contains("neovim"));
        assert!(!selected.contains("rust"));
    }

    #[test]
    fn selects_tools_matching_tag() {
        let cli = make_cli(
            r#"
                [tools.git]
                tags = ["core"]

                [tools.rust]
                tags = ["development"]

                [tools.neovim]
                tags = ["core", "editor"]
            "#,
            Command::Install(selection(&[], &["core"])),
        );

        let args = selection(&[], &["core"]);
        let selected = cli.selected_tools(&args);

        assert_eq!(selected.len(), 2);
        assert!(selected.contains("git"));
        assert!(selected.contains("neovim"));
        assert!(!selected.contains("rust"));
    }

    #[test]
    fn combines_explicit_tools_and_tags() {
        let cli = make_cli(
            r#"
                [tools.git]
                tags = ["core"]

                [tools.rust]
                tags = ["development"]

                [tools.neovim]
                tags = ["editor"]
            "#,
            Command::Install(selection(&["rust"], &["core"])),
        );

        let args = selection(&["rust"], &["core"]);
        let selected = cli.selected_tools(&args);

        assert_eq!(selected.len(), 2);
        assert!(selected.contains("rust"));
        assert!(selected.contains("git"));
    }

    #[test]
    fn resolve_tools_includes_dependencies() {
        let cli = make_cli(
            r#"
                [tools.git]
                deps = ["rust"]

                [tools.rust]
                deps = ["compiler"]

                [tools.compiler]
            "#,
            Command::Install(selection(&["git"], &[])),
        );

        let args = selection(&["git"], &[]);
        let resolved = cli.resolve_tools(&args).unwrap();

        assert_eq!(resolved.len(), 3);
        assert!(resolved.contains("git"));
        assert!(resolved.contains("rust"));
        assert!(resolved.contains("compiler"));
    }

    #[test]
    fn shared_dependency_is_only_resolved_once() {
        let cli = make_cli(
            r#"
                [tools.git]
                deps = ["runtime"]

                [tools.neovim]
                deps = ["runtime"]

                [tools.runtime]
            "#,
            Command::Install(selection(&["git", "neovim"], &[])),
        );

        let args = selection(&["git", "neovim"], &[]);
        let resolved = cli.resolve_tools(&args).unwrap();

        assert_eq!(resolved.len(), 3);
        assert!(resolved.contains("git"));
        assert!(resolved.contains("neovim"));
        assert!(resolved.contains("runtime"));
    }

    #[test]
    fn rejects_unknown_dependency() {
        let cli = make_cli(
            r#"
                [tools.git]
                deps = ["missing-tool"]
            "#,
            Command::Install(selection(&["git"], &[])),
        );

        let args = selection(&["git"], &[]);
        let selected = cli.selected_tools(&args);

        let error = cli
            .validate_dependencies_exist(&selected)
            .unwrap_err();

        assert_eq!(
            error,
            "tool `git` depends on unknown tool `missing-tool`"
        );
    }

    #[test]
    fn rejects_direct_dependency_cycle() {
        let cli = make_cli(
            r#"
                [tools.git]
                deps = ["git"]
            "#,
            Command::Install(selection(&["git"], &[])),
        );

        let args = selection(&["git"], &[]);
        let selected = cli.selected_tools(&args);

        let error = cli
            .validate_no_dependency_cycles(&selected)
            .unwrap_err();

        assert_eq!(
            error,
            "circular dependency detected: git -> git"
        );
    }

    #[test]
    fn rejects_indirect_dependency_cycle() {
        let cli = make_cli(
            r#"
                [tools.git]
                deps = ["rust"]

                [tools.rust]
                deps = ["cargo"]

                [tools.cargo]
                deps = ["git"]
            "#,
            Command::Install(selection(&["git"], &[])),
        );

        let args = selection(&["git"], &[]);
        let selected = cli.selected_tools(&args);

        let error = cli
            .validate_no_dependency_cycles(&selected)
            .unwrap_err();

        assert_eq!(
            error,
            "circular dependency detected: git -> rust -> cargo -> git"
        );
    }

    #[test]
    fn accepts_valid_dependency_graph() {
        let cli = make_cli(
            r#"
                [tools.neovim]
                deps = ["git", "rust"]

                [tools.git]

                [tools.rust]
                deps = ["compiler"]

                [tools.compiler]
            "#,
            Command::Install(selection(&["neovim"], &[])),
        );

        let args = selection(&["neovim"], &[]);
        let selected = cli.selected_tools(&args);

        assert!(cli.validate_dependencies_exist(&selected).is_ok());
        assert!(cli.validate_no_dependency_cycles(&selected).is_ok());
    }
}
