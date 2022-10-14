use crate::native_resp::{write_native_event, NativeResponseEvent};
use crate::state::AppState;
use crate::storage::global_options_data_path;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;

// === GLOBAL OPTIONS ===

//// Read global options from the specified file
pub fn read_global_options(path: &PathBuf) -> HashMap<String, Value> {
    if let Ok(file) = OpenOptions::new().read(true).open(path) {
        if let Ok(options) = serde_json::from_reader(file) {
            return options;
        }
    }

    // Global options don't exist or is invalid, load empty options
    HashMap::new()
}

#[derive(Debug)]
pub enum WriteGlobalOptionsError {
    OpenFileError(io::Error),
    WriteFileError(serde_json::Error),
}

//// Read global options to the specified file
pub fn write_global_options(
    path: &PathBuf,
    new_options: &HashMap<String, Value>,
) -> Result<(), WriteGlobalOptionsError> {
    let options_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(WriteGlobalOptionsError::OpenFileError)?;

    serde_json::to_writer(options_file, &new_options)
        .map_err(WriteGlobalOptionsError::WriteFileError)
}

pub fn native_notify_updated_options(app_state: &AppState) {
    let new_options = read_global_options(&global_options_data_path(&app_state.config_dir));

    write_native_event(NativeResponseEvent::OptionsUpdated {
        options: new_options,
    });
}
