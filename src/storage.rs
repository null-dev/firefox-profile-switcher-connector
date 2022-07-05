use std::path::{Path, PathBuf};
use crate::{AppContext};

pub fn global_options_data_path(config_dir: &Path) -> PathBuf {
    config_dir.join("global-options.json")
}

pub fn avatar_data_path(config_dir: &Path) -> PathBuf {
    config_dir.join("avatars.json")
}

pub fn options_data_path(config_dir: &Path) -> PathBuf {
    config_dir.join("profile-options.json")
}

pub fn custom_avatars_path(context: &AppContext) -> PathBuf {
    context.state.data_dir.join("avatars")
}