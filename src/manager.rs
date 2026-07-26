use crate::{
    cli::Command,
    dotfile::{Manifest, Tool},
    shell::{check_executable, create_symlink},
};

pub struct Manager<'a> {
    pub os: &'a str,
    pub manifest: &'a Manifest,
    pub tool_chain: Vec<&'a str>,
    pub selected_command: &'a Command,
}

impl<'a> Manager<'a> {
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

                Command::Validate => unreachable!(),
            }
        }

        Ok(())
    }

    fn install_tool(&self, tool_name: &str, tool: &Tool) -> Result<(), String> {
        let Some(install_commands) = tool.install.as_ref() else {
            return Ok(());
        };

        let Some(commands) = install_commands.get(self.os) else {
            return Ok(());
        };

        for command in commands {
            if !check_executable(command) {
                return Err(format!(
                    "failed to execute install command `{command}` for tool `{tool_name}`"
                ));
            }
        }

        Ok(())
    }

    fn configure_tool(&self, tool_name: &str, tool: &Tool) -> Result<(), String> {
        let Some(config) = tool.configs.as_ref() else {
            return Ok(());
        };

        let Some(target) = config.targets.get(self.os) else {
            return Ok(());
        };

        println!("Linking `{}` -> `{target}`", config.source);

        if !create_symlink(&config.source, target) {
            return Err(format!(
                "failed to create symlink for tool `{tool_name}`: `{}` -> `{target}`",
                config.source
            ));
        }

        Ok(())
    }
}
