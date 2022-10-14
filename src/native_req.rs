use anyhow::{Context, Result};
use byteorder::{NativeEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;

// === NATIVE REQUEST ===
#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageInitialize {
    pub extension_id: String,
    pub profile_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageLaunchProfile {
    pub profile_id: String,
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageCreateProfile {
    pub name: String,
    pub avatar: String,
    pub options: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageDeleteProfile {
    pub profile_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageUpdateProfile {
    pub profile_id: String,
    pub name: String,
    pub avatar: Option<String>,
    pub options: HashMap<String, Value>,
    pub default: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageUpdateOptions {
    pub changes: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageGetAvatar {
    pub avatar: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageDeleteAvatar {
    pub avatar: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "command")]
pub enum NativeMessage {
    Initialize(NativeMessageInitialize),
    LaunchProfile(NativeMessageLaunchProfile),
    CreateProfile(NativeMessageCreateProfile),
    DeleteProfile(NativeMessageDeleteProfile),
    UpdateProfile(NativeMessageUpdateProfile),
    UpdateOptions(NativeMessageUpdateOptions),
    CloseManager,
    AddAvatars,
    GetAvatar(NativeMessageGetAvatar),
    DeleteAvatar(NativeMessageDeleteAvatar),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NativeMessageWrapper {
    pub id: i64,
    pub msg: NativeMessage,
}

pub fn read_incoming_message(input: &mut impl Read) -> Result<NativeMessageWrapper> {
    // Read size of incoming message
    let size = input
        .read_u32::<NativeEndian>()
        .expect("Failed to read native message size!");

    // Read and deserialize
    let mut conf_buffer = vec![0u8; size as usize];

    input
        .read_exact(&mut conf_buffer)
        .expect("Failed to read native message!");
    serde_json::from_slice(&conf_buffer).context("Failed to deserialize native message!")
}
