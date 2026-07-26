//! Command-line entry point for dotstrap.
//!
//! The application follows a three-stage flow:
//! 1. parse a TOML manifest and command-line selection;
//! 2. validate the request and resolve a dependency-ordered tool plan;
//! 3. execute installation or configuration actions for that plan.

#![warn(missing_docs)]

mod cli;
mod dotfile;
mod manager;
mod shell;

use clap::Parser;
use cli::DotManager;

/// Parses the command line, validates it, and executes the resulting plan.
fn main() {
    let dotman = DotManager::parse();

    let manager = match dotman.validate() {
        Ok(manager) => manager,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    };

    match manager {
        None => println!("Dotfiles validated"),
        Some(manager) => {
            if let Err(error) = manager.execute() {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        }
    }
}
