#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod config;
mod gui;
mod packer;
mod vpk;

#[cfg(test)]
mod tests;

fn main() -> anyhow::Result<()> {
    if std::env::args_os().len() > 1 {
        cli::run()
    } else {
        gui::run()
    }
}
