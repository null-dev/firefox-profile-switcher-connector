use std::path::PathBuf;
use std::collections::HashMap;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io;

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
    WriteFileError(serde_json::Error)
}

//// Read global options to the specified file
pub fn write_global_options(path: &PathBuf, new_options: &HashMap<String, Value>) -> Result<(), WriteGlobalOptionsError> {
    let options_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(WriteGlobalOptionsError::OpenFileError)?;

    serde_json::to_writer(options_file, &new_options)
        .map_err(WriteGlobalOptionsError::WriteFileError)
}
