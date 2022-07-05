// === CONFIG ===

use std::path::{PathBuf};
use serde::{Deserialize, Serialize};
use cfg_if::cfg_if;
use std::fs::OpenOptions;
use once_cell::sync::Lazy;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    browser_profile_dir: Option<PathBuf>,
    browser_binary: Option<PathBuf>
}

impl Config {
    pub fn browser_profile_dir(&self) -> PathBuf {
        self.browser_profile_dir.clone()
            .unwrap_or_else(|| get_default_browser_profile_folder().clone())
    }
    pub fn browser_binary(&self) -> Option<&PathBuf> {
        self.browser_binary.as_ref()
    }

    pub fn profiles_ini_path(&self) -> PathBuf {
        let mut profiles_ini = self.browser_profile_dir();
        profiles_ini.push("profiles.ini");
        return profiles_ini;
    }
    pub fn installs_ini_path(&self) -> PathBuf {
        let mut installs_ini = self.browser_profile_dir();
        installs_ini.push("installs.ini");
        return installs_ini;
    }
}

// Detect if Firefox is installed from Microsoft Store
#[cfg(target_os = "windows")]
static MSIX_PACKAGE: Lazy<Result<String, String>> = Lazy::new(|| {
    get_parent_proc_path()
        .map_err(|e| format!("get_parent_proc_path failed: {:?}", e))
        .and_then(|p| {
            // Windows path looks like this:
            // [Prefix(PrefixComponent { raw: "C:", parsed: Disk(67) }), RootDir, Normal("Program Files"), Normal("WindowsApps"), Normal("Mozilla.Firefox_97.0.1.0_x64__n80bbvh6b1yt2"), Normal("VFS"), Normal("ProgramFiles"), Normal("Firefox Package Root"), Normal("firefox.exe")]
            let components: Vec<Component> = p.components()
                // Skip beginning of path until we get to the root dir (e.g. the C: prefix)
                .skip_while(|c| !matches!(c, Component::RootDir))
                .skip(1) // Now skip the root dir
                .take(3) // Take the "Program Files", "WindowsApps" and package entries
                .collect();

            if let [
                Component::Normal(p1),
                Component::Normal(p2),
                Component::Normal(package)
            ] = components[..] {
                if p1 == "Program Files" && p2 == "WindowsApps" {
                    if let Some(package) = package.to_str() {
                        if let [Some(pname_sep), Some(pid_sep)] = [package.find("_"), package.rfind("_")] {
                            return Ok(format!("{}_{}", &package[..pname_sep], &package[pid_sep + 1..]))
                        }
                    }
                }
            }

            Err(format!("Browser path is not in MSIX format, components: {:?}!", components))
        })
});
#[cfg(target_os = "windows")]
pub fn get_msix_package() -> Result<&'static String, &'static String> {
    MSIX_PACKAGE.as_ref()
}

static DEFAULT_BROWSER_PROFILE_FOLDER: Lazy<PathBuf> = Lazy::new(|| {
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
            match MSIX_PACKAGE.as_ref() {
                Ok(msix_package) => {
                    log::trace!("Detected MSIX package: {}", msix_package);

                    result.push("AppData");
                    result.push("Local");
                    result.push("Packages");
                    result.push(msix_package);
                    result.push("LocalCache");
                }
                Err(e) => {
                    log::trace!("Did not detect MSIX package: {}", e);

                    result.push("AppData");
                }
            }
            result.push("Roaming");
            result.push("Mozilla");
            result.push("Firefox");
        } else {
            compile_error!("Unknown OS!");
        }
    }
    log::trace!("Found default browser profile dir: {:?}", result);
    return result;
});
fn get_default_browser_profile_folder() -> &'static PathBuf {
    &DEFAULT_BROWSER_PROFILE_FOLDER
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

