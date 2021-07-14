// === CONFIG ===

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use cfg_if::cfg_if;
use std::fs::OpenOptions;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    browser_profile_dir: Option<PathBuf>,
    browser_binary: Option<PathBuf>
}

impl Config {
    pub fn browser_profile_dir(&self) -> PathBuf {
        self.browser_profile_dir.clone().unwrap_or_else(get_default_browser_profile_folder)
    }
    pub fn browser_binary(&self) -> Option<&PathBuf> {
        self.browser_binary.as_ref()
    }

    pub fn profiles_ini_path(&self) -> PathBuf {
        let mut profiles_ini = self.browser_profile_dir().clone();
        profiles_ini.push("profiles.ini");
        return profiles_ini;
    }
    pub fn installs_ini_path(&self) -> PathBuf {
        let mut installs_ini = self.browser_profile_dir().clone();
        installs_ini.push("installs.ini");
        return installs_ini;
    }
}

fn get_default_browser_profile_folder() -> PathBuf {
    let user_dirs = directories::UserDirs::new()
        .expect("Unable to determine user folder!");

    let mut result = user_dirs.home_dir().to_path_buf();
    cfg_if! {
        if #[cfg(target_os = "linux")] {
            result.push(".mozilla");
            result.push("firefox");
        } else if #[cfg(target_os = "macos")] {
            result.push("Library");
            result.push("Application Support");
            result.push("Firefox");
        } else if #[cfg(target_os = "windows")] {
            result.push("AppData");
            result.push("Roaming");
            result.push("Mozilla");
            result.push("Firefox");
        } else {
            compile_error!("Unknown OS!");
        }
    }
    log::trace!("Found default browser profile dir: {:?}", result);
    return result;
}

impl Default for Config {
    fn default() -> Self {
        Config {
            browser_profile_dir: None,
            browser_binary: None
        }
    }
}

pub fn read_configuration(path: &PathBuf) -> Config {
    if let Ok(file) = OpenOptions::new().read(true).open(path) {
        if let Ok(config) = serde_json::from_reader(file) {
            return config;
        }
    }

    // Config doesn't exist or is invalid, load default config
    Config::default()
}

