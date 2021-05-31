// === NATIVE RESPONSE ===

use std::fmt::Debug;
use std::collections::HashMap;
use serde_json::Value;
use crate::profiles::ProfileEntry;
use std::io;
use byteorder::{NativeEndian, WriteBytesExt};
use std::io::Write;
use serde::{Serialize};

#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum NativeResponse {
    Error {
        success: bool,
        error: String,
        debug_msg: Option<String>
    },
    Success {
        success: bool,
        #[serde(flatten)]
        data: NativeResponseData
    },
    Event(NativeResponseEvent)
}

pub const NATIVE_RESP_ID_EVENT: i64 = -1;

#[derive(Serialize)]
pub struct NativeResponseWrapper {
    pub id: i64,
    pub resp: NativeResponse
}

impl NativeResponse {
    pub fn error(msg: &str) -> NativeResponse {
        NativeResponse::Error {
            success: false,
            error: String::from(msg),
            debug_msg: None
        }
    }
    pub fn error_with_dbg_msg(msg: &str, err: impl Debug) -> NativeResponse {
        NativeResponse::Error {
            success: false,
            error: String::from(msg),
            debug_msg: Some(format!("{:?}", err))
        }
    }
    pub fn error_with_dbg_str(msg: &str, err: String) -> NativeResponse {
        NativeResponse::Error {
            success: false,
            error: String::from(msg),
            debug_msg: Some(err)
        }
    }
    pub fn success(data: NativeResponseData) -> NativeResponse {
        NativeResponse::Success {
            success: true,
            data
        }
    }
    pub fn event(event: NativeResponseEvent) -> NativeResponse {
        NativeResponse::Event(event)
    }
}

#[derive(Serialize, Debug)]
pub struct NativeResponseProfileListProfileEntry {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub avatar: Option<String>,
    pub options: HashMap<String, Value>
}

impl NativeResponseProfileListProfileEntry {
    pub fn from_profile_entry(entry: &ProfileEntry) -> NativeResponseProfileListProfileEntry {
        NativeResponseProfileListProfileEntry {
            id: entry.id.clone(),
            name: entry.name.clone(),
            default: entry.default,
            avatar: entry.avatar.clone(),
            options: entry.options.clone()
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum NativeResponseData {
    Initialized {
        cached: bool
    },
    ProfileLaunched,
    ProfileCreated {
        profile: NativeResponseProfileListProfileEntry
    },
    ProfileUpdated {
        profile: NativeResponseProfileListProfileEntry
    },
    ProfileDeleted,
    OptionsUpdated {
        options: HashMap<String, Value>
    },
    ManagerClosed
}

#[derive(Serialize, Debug)]
#[serde(tag = "event")]
pub enum NativeResponseEvent {
    ProfileList { current_profile_id: String, profiles: Vec<NativeResponseProfileListProfileEntry> },
    FocusWindow { url: Option<String> },
    CloseManager,
    ConnectorInformation { version: String },
    OptionsUpdated { options: HashMap<String, Value> }
}

pub fn write_native_response(resp: NativeResponseWrapper) {
    let serialized = serde_json::to_vec(&resp).unwrap();
    // TODO Handle error
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_u32::<NativeEndian>(serialized.len() as u32);
    handle.write_all(&serialized);
    handle.flush();
}

