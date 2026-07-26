//! Tool selection and dependency-ordered execution planning.

use std::collections::HashSet;

use crate::dotfile::Manifest;

use super::SelectionArgs;

/// Resolves a manifest selection into a validated, topologically ordered plan.
pub(super) struct ToolPlanner<'a> {
    manifest: &'a Manifest,
}

impl<'a> ToolPlanner<'a> {
    /// Creates a planner over a borrowed manifest.
    pub(super) fn new(manifest: &'a Manifest) -> Self {
        Self { manifest }
    }

    /// Validates and resolves a selection into dependency-first order.
    pub(super) fn plan(&self, args: &SelectionArgs) -> Result<Vec<&'a str>, String> {
        self.validate_selected_tools(args)?;
        self.validate_selected_tags(args)?;

        let selected = self.selected_tools(args);
        self.validate_dependencies_exist(&selected)?;
        self.validate_no_dependency_cycles(&selected)?;
        self.resolve_tools(selected)
    }

    /// Validates the entire manifest dependency graph.
    pub(super) fn validate_all(&self) -> Result<(), String> {
        let all_tools: HashSet<&str> = self.manifest.tools.keys().map(String::as_str).collect();
        self.validate_dependencies_exist(&all_tools)?;
        self.validate_no_dependency_cycles(&all_tools)
    }

    /// Expands explicit names and tags; an empty selection means every tool.
    fn selected_tools<'b>(&'b self, args: &'b SelectionArgs) -> HashSet<&'b str> {
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
                selected.insert(tool_name);
            }
        }

        selected
    }

    /// Resolves selected tools and transitive dependencies deterministically.
    fn resolve_tools(&self, selected: HashSet<&str>) -> Result<Vec<&'a str>, String> {
        let mut selected: Vec<&str> = selected.into_iter().collect();
        selected.sort_unstable();

        let mut resolved = Vec::new();
        let mut visited = HashSet::new();
        let mut visiting = HashSet::new();

        for tool_name in selected {
            self.resolve_tool(tool_name, &mut visited, &mut visiting, &mut resolved)?;
        }

        Ok(resolved)
    }

    /// Performs a depth-first topological traversal for one tool.
    fn resolve_tool(
        &self,
        tool_name: &str,
        visited: &mut HashSet<&'a str>,
        visiting: &mut HashSet<&'a str>,
        resolved: &mut Vec<&'a str>,
    ) -> Result<(), String> {
        let (canonical_name, tool) = self
            .manifest
            .tools
            .get_key_value(tool_name)
            .ok_or_else(|| format!("tool `{tool_name}` does not exist in the manifest"))?;
        let canonical_name = canonical_name.as_str();

        if visited.contains(canonical_name) {
            return Ok(());
        }
        if !visiting.insert(canonical_name) {
            return Err(format!(
                "circular dependency detected while resolving `{canonical_name}`"
            ));
        }

        let mut dependencies: Vec<&str> = tool
            .deps
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .collect();
        dependencies.sort_unstable();

        for dependency in dependencies {
            self.resolve_tool(dependency, visited, visiting, resolved)?;
        }

        visiting.remove(canonical_name);
        visited.insert(canonical_name);
        resolved.push(canonical_name);
        Ok(())
    }

    /// Ensures all dependencies reachable from a selected set are defined.
    fn validate_dependencies_exist(&self, selected: &HashSet<&str>) -> Result<(), String> {
        let mut visited = HashSet::new();
        for tool_name in selected {
            self.validate_tool_dependencies_exist(tool_name, &mut visited)?;
        }
        Ok(())
    }

    /// Recursively validates dependency names while avoiding repeated work.
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

    /// Ensures the selected dependency subgraph contains no cycles.
    fn validate_no_dependency_cycles(&self, selected: &HashSet<&str>) -> Result<(), String> {
        let mut fully_visited = HashSet::new();
        let mut current_path = Vec::new();
        for tool_name in selected {
            self.visit_dependency(tool_name, &mut fully_visited, &mut current_path)?;
        }
        Ok(())
    }

    /// Traverses dependencies while retaining the current path for cycle errors.
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

    /// Rejects explicit tool names absent from the manifest.
    fn validate_selected_tools(&self, args: &SelectionArgs) -> Result<(), String> {
        for tool_name in &args.tools {
            if !self.manifest.tools.contains_key(tool_name) {
                return Err(format!("tool `{tool_name}` does not exist in the manifest"));
            }
        }
        Ok(())
    }

    /// Rejects requested tags which do not occur on any tool.
    fn validate_selected_tags(&self, args: &SelectionArgs) -> Result<(), String> {
        for tag_name in &args.tags {
            let found = self.manifest.tools.values().any(|tool| {
                tool.tags
                    .as_ref()
                    .is_some_and(|tags| tags.contains(tag_name))
            });
            if !found {
                return Err(format!("tag `{tag_name}` does not exist in the manifest"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dotfile::Manifest;

    fn manifest(tools_toml: &str) -> Manifest {
        toml::from_str(&format!("version = 1\n[package_managers]\n{tools_toml}")).unwrap()
    }

    fn selection(tools: &[&str], tags: &[&str]) -> SelectionArgs {
        SelectionArgs {
            tools: tools.iter().map(ToString::to_string).collect(),
            tags: tags.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn empty_selection_selects_all_tools() {
        let manifest = manifest("[tools.git]\n[tools.rust]\n[tools.neovim]");
        let plan = ToolPlanner::new(&manifest)
            .plan(&selection(&[], &[]))
            .unwrap();
        assert_eq!(plan, vec!["git", "neovim", "rust"]);
    }

    #[test]
    fn combines_explicit_tools_and_tags() {
        let manifest =
            manifest("[tools.git]\ntags=['core']\n[tools.rust]\ntags=['dev']\n[tools.nvim]");
        let plan = ToolPlanner::new(&manifest)
            .plan(&selection(&["rust"], &["core"]))
            .unwrap();
        assert_eq!(plan, vec!["git", "rust"]);
    }

    #[test]
    fn resolves_dependencies_once_and_before_dependants() {
        let manifest = manifest(
            "[tools.git]\ndeps=['runtime']\n\
             [tools.neovim]\ndeps=['runtime']\n\
             [tools.runtime]",
        );
        let plan = ToolPlanner::new(&manifest)
            .plan(&selection(&["git", "neovim"], &[]))
            .unwrap();
        assert_eq!(plan, vec!["runtime", "git", "neovim"]);
    }

    #[test]
    fn rejects_unknown_tool_tag_and_dependency() {
        let manifest = manifest("[tools.git]\ndeps=['missing']");
        let planner = ToolPlanner::new(&manifest);

        assert_eq!(
            planner.plan(&selection(&["unknown"], &[])).unwrap_err(),
            "tool `unknown` does not exist in the manifest"
        );
        assert_eq!(
            planner.plan(&selection(&[], &["unknown"])).unwrap_err(),
            "tag `unknown` does not exist in the manifest"
        );
        assert_eq!(
            planner.plan(&selection(&["git"], &[])).unwrap_err(),
            "tool `git` depends on unknown tool `missing`"
        );
    }

    #[test]
    fn rejects_dependency_cycle_with_path() {
        let manifest = manifest(
            "[tools.git]\ndeps=['rust']\n\
             [tools.rust]\ndeps=['cargo']\n\
             [tools.cargo]\ndeps=['git']",
        );
        let error = ToolPlanner::new(&manifest)
            .plan(&selection(&["git"], &[]))
            .unwrap_err();
        assert_eq!(
            error,
            "circular dependency detected: git -> rust -> cargo -> git"
        );
    }

    #[test]
    fn rejects_direct_self_dependency() {
        let manifest = manifest("[tools.git]\ndeps=['git']");
        let error = ToolPlanner::new(&manifest)
            .plan(&selection(&["git"], &[]))
            .unwrap_err();
        assert_eq!(error, "circular dependency detected: git -> git");
    }

    #[test]
    fn validate_all_checks_unselected_tools() {
        let manifest = manifest("[tools.git]\n[tools.unselected]\ndeps=['missing']");
        let error = ToolPlanner::new(&manifest).validate_all().unwrap_err();
        assert_eq!(error, "tool `unselected` depends on unknown tool `missing`");
    }
}
