mod options;
mod config;
mod storage;
mod profiles;
mod native_req;
mod native_resp;
mod ipc;
mod cmd;
mod process;

extern crate ini;
extern crate serde;
extern crate serde_json;
extern crate directories;
extern crate fs2;
extern crate cfg_if;
extern crate ring;
extern crate data_encoding;
extern crate ulid;
extern crate fern;
extern crate log;
extern crate url;
extern crate chrono;
extern crate rand;
extern crate serde_cbor;

cfg_if! {
    if #[cfg(target_family = "unix")] {
        extern crate nix;
        extern crate libc;
    } else if #[cfg(target_family = "windows")] {
        extern crate windows;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    }
}

use std::{io, env};
use std::fs;
use cfg_if::cfg_if;
use directories::{ProjectDirs, UserDirs};
use rand::Rng;
use crate::config::{read_configuration};
use crate::profiles::read_profiles;
use crate::state::AppState;
use crate::native_resp::{NativeResponseEvent, write_native_response, NativeResponseWrapper, NativeResponse, write_native_event};
use crate::cmd::execute_cmd_for_message;
use crate::native_req::{read_incoming_message, NativeMessage};

const APP_VERSION: &'static str = env!("CARGO_PKG_VERSION");

// This is the application state, it will be immutable through the life of the application
mod state {
    use crate::config::Config;
    use std::path::PathBuf;

    #[derive(Clone, Debug)]
    pub struct AppState {
        pub config: Config,
        pub first_run: bool,
        pub cur_profile_id: Option<String>,
        pub extension_id: Option<String>,
        pub internal_extension_id: Option<String>,
        pub config_dir: PathBuf,
        pub data_dir: PathBuf
    }
}

// === MAIN ===

fn main() {
    // Notify extension of our version
    write_native_event(NativeResponseEvent::ConnectorInformation {
        version: APP_VERSION.to_string()
    });

    // Calculate storage dirs
    let project_dirs = ProjectDirs::from("ax.nd",
                                        "nulldev",
                                        "FirefoxProfileSwitcher")
        .expect("Could not initialize configuration (failed to find storage dir)!");
    let pref_dir = project_dirs.preference_dir();
    let data_dir = project_dirs.data_local_dir();

    let first_run = !data_dir.exists();

    let user_dirs = UserDirs::new()
        .expect("Unable to find user dirs!");
    let desktop = user_dirs
        .desktop_dir()
        .expect("Unable to find desktop dir!");

    // mkdirs
    fs::create_dir_all(pref_dir);
    fs::create_dir_all(data_dir);

    // Enable full logging when debugging is enabled
    let log_level = log::LevelFilter::Trace;

    // Use to keep track of instances through a log session
    let instance_key: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    // Setup logging
    fern::Dispatch::new()
        .level(log_level)
        .chain(fern::log_file(desktop.join("firefox-profile-switcher-log.txt"))
            .expect("Unable to open logfile!"))
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{}]{}[{}][{}] {}",
                instance_key,
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .apply()
        .expect("Failed to setup logging!");

    log::trace!("Finished setup logging (app version: {}).", APP_VERSION);

    // Initialize Windows COM library
    cfg_if! {
        if #[cfg(target_family = "windows")] {
            if let Err(e) = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED) } {
                log::trace!("Windows COM library initialization failure, continuing anyway: {:?}", e);
            } else {
                log::trace!("Windows COM library initialized.");
            }
        }
    }

    // Find extension ID
    let args: Vec<String> = env::args().collect();
    let extension_id = args.get(2);
    if extension_id.is_none() {
        log::warn!("Could not determine extension ID!");
    }

    log::trace!("Extension id: {}", extension_id.unwrap());

    // Read configuration
    let config_path = pref_dir.join("config.json");
    let config = read_configuration(&config_path);

    log::trace!("Configuration loaded: {:?}", &config);

    let mut app_state = AppState {
        config,
        first_run,
        cur_profile_id: None,
        extension_id: extension_id.cloned(),
        internal_extension_id: None,
        config_dir: pref_dir.to_path_buf(),
        data_dir: data_dir.to_path_buf()
    };

    log::trace!("Entering main loop, initial application state: {:?}", &app_state);

    loop {
        let message = read_incoming_message(&mut io::stdin());

        log::trace!("Received message, processing: {:?}", &message);

        /*
        // TODO Lock SI when updating profile list over IPC
        // SI lock
        let lock_path = data_dir.join("si.lock");
        // Create/open SI lockfile
        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)
            .expect("Failed to open single-instance lock!");
        // Lock the lockfile
        lock_file.lock_exclusive()
            .expect("Failed to grab single-instance lock!");
         */

        let response = process_message(&mut app_state, message.msg);

        log::trace!("Message {} processed, response is: {:?}", &message.id, &response);

        write_native_response(NativeResponseWrapper {
            id: message.id,
            resp: response
        });

        log::trace!("Response written for message {}, waiting for next message.", &message.id);
    }
}

fn process_message(app_state: &mut AppState, msg: NativeMessage) -> NativeResponse {
    let mut profiles = match read_profiles(&app_state.config, &app_state.config_dir) {
        Ok(p) => p,
        Err(e) => {
            return NativeResponse::error_with_dbg_msg("Failed to load profile list.", e);
        }
    };

    log::trace!("Profile list processed!");

    execute_cmd_for_message(app_state, &mut profiles, msg)
}
