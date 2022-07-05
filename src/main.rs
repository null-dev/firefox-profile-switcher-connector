mod options;
mod config;
mod storage;
mod profiles;
mod native_req;
mod native_resp;
mod ipc;
mod cmd;
mod process;
mod windowing;
mod avatars;

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

use std::{io, env, thread};
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, RwLock};
use cfg_if::cfg_if;
use directories::ProjectDirs;
use indexmap::IndexMap;
use rand::Rng;
use crate::avatars::update_and_native_notify_avatars;
use crate::config::{read_configuration};
use crate::state::{AppContext, AppState};
use crate::native_resp::{NativeResponseEvent, write_native_response, NativeResponseWrapper, NativeResponse, write_native_event};
use crate::cmd::{execute_cmd_for_message, execute_init_cmd};
use crate::ipc::setup_ipc;
use crate::native_req::{read_incoming_message};
use crate::windowing::Windowing;

const APP_VERSION: &'static str = env!("CARGO_PKG_VERSION");

// This is the application state, it will be (mostly) immutable through the life of the application
mod state {
    use std::collections::HashMap;
    use crate::config::Config;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};
    use indexmap::IndexMap;
    use ulid::Ulid;
    use crate::windowing::WindowingHandle;

    #[derive(Clone, Debug)]
    pub struct AppState {
        pub config: Config,
        pub first_run: bool,
        pub cur_profile_id: Option<String>,
        pub extension_id: Option<String>,
        pub internal_extension_id: Option<String>,
        pub config_dir: PathBuf,
        pub data_dir: PathBuf,
    }

    #[derive(Clone, Debug)]
    pub struct AppContext {
        pub state: &'static AppState,
        pub windowing: WindowingHandle,
        pub avatars: Arc<RwLock<IndexMap<Ulid, PathBuf>>>
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

    // mkdirs
    fs::create_dir_all(pref_dir);
    fs::create_dir_all(data_dir);

    // Enable full logging when debugging is enabled
    let log_level = if data_dir.join("DEBUG").exists() {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Warn
    };

    // Use to keep track of instances through a log session
    let instance_key: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    // Setup logging
    fern::Dispatch::new()
        .level(log_level)
        .chain(fern::log_file(data_dir.join("log.txt"))
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

    let windowing = Windowing::new();

    let mut app_state = AppState {
        config,
        first_run,
        cur_profile_id: None,
        extension_id: extension_id.cloned(),
        internal_extension_id: None,
        config_dir: pref_dir.to_path_buf(),
        data_dir: data_dir.to_path_buf(),
    };

    log::trace!("Entering initialization loop, initial application state: {:?}", &app_state);

    // Init loop, at this time, we only accept init messages
    loop {
        let message = match read_incoming_message(&mut io::stdin()) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to deserialize incoming message: {:?}", e);
                // Best to restart here because maybe our IO went out of sync
                panic!("Forcing restart due to deserialization failure.");
            }
        };

        log::trace!("Received possible init message, processing: {:?}", &message);

        let response = execute_init_cmd(&mut app_state, message.msg);

        log::trace!("Message {} processed, response is: {:?}", &message.id, &response);

        let init_ok = match response {
            NativeResponse::Success { .. } => true,
            _ => false
        };

        write_native_response(NativeResponseWrapper {
            id: message.id,
            resp: response
        });

        log::trace!("Response written for message {}", &message.id);

        if init_ok {
            break;
        }
    }

    // No longer initing, we accept any type of message now (except init messages).

    log::trace!("Connector initialized, application state is now frozen, enter main loop.");

    // Leak the app state because we need to read it from multiple threads
    let app_state_leaked = Box::leak(Box::new(app_state));

    let context = AppContext {
        state: &*app_state_leaked,
        windowing: windowing.get_handle(),
        avatars: Arc::new(RwLock::new(IndexMap::new()))
    };

    update_and_native_notify_avatars(&context);

    // Begin IPC
    let context_clone = context.clone();
    thread::spawn(move || {
        if let Err(e) = setup_ipc(&context_clone) {
            log::error!("Failed to setup IPC server: {:?}", e);
        }
    });

    thread::spawn(move || {
        let pool = threadfin::builder()
            .size(1..50)
            .build();

        loop {
            let message = match read_incoming_message(&mut io::stdin()) {
                Ok(m) => m,
                Err(e) => {
                    log::error!("Failed to deserialize incoming message: {:?}", e);
                    // Best to restart here because maybe our IO went out of sync
                    panic!("Forcing restart due to deserialization failure.");
                }
            };

            log::trace!("Received message, processing: {:?}", &message);

            let context_clone = context.clone();

            pool.execute(move || {
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

                let response = execute_cmd_for_message(&context_clone, message.msg);

                log::trace!("Message {} processed, response is: {:?}", &message.id, &response);

                write_native_response(NativeResponseWrapper {
                    id: message.id,
                    resp: response
                });

                log::trace!("Response written for message {}.", &message.id);
            });
        }
    });

    windowing.run_event_loop();
}