use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILENAME: &str = ".vpk_tool_config";

fn config_path() -> Result<PathBuf> {
    if let Ok(app_data) = env::var("APPDATA") {
        Ok(PathBuf::from(app_data).join(CONFIG_FILENAME))
    } else if let Ok(home) = env::var("HOME") {
        Ok(PathBuf::from(home).join(CONFIG_FILENAME))
    } else {
        Err(anyhow!("Could not determine config directory"))
    }
}

pub fn load_output_dir() -> Option<PathBuf> {
    if let Ok(config_path) = config_path() {
        if let Ok(content) = fs::read_to_string(&config_path) {
            let path = PathBuf::from(content.trim());
            if path.exists() && path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

pub fn save_output_dir(path: &Path) -> Result<()> {
    if let Ok(config_path) = config_path() {
        fs::write(&config_path, path.to_string_lossy().to_string())?;
    }
    Ok(())
}
