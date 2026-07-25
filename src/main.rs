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
    // parse cli options
    let manager = DotManager::parse();

    if let Err(error) = manager.validate() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}
