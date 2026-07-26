mod cli;
mod dotfile;
mod manager;
mod setup;
mod shell;

use clap::Parser;
use cli::DotManager;
use dotfile::Manifest;
use setup::{Mode, Setup};
use std::{fs, path::PathBuf};

fn main() {
    let dotman = DotManager::parse();

    let manager = match dotman.validate() {
        Ok(manager) => manager.unwrap(),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = manager.execute() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
