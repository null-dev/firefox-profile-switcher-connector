use std::fs;
use std::str::FromStr;
use ulid::{Ulid};
use crate::{AppContext, NativeResponse};
use crate::avatars::encode_avatar_to_string;
use crate::native_req::NativeMessageGetAvatar;
use crate::native_resp::NativeResponseData;

pub fn process_get_avatar(context: &AppContext, msg: NativeMessageGetAvatar) -> NativeResponse {
    let ulid = match Ulid::from_str(&msg.avatar) {
        Ok(u) => u,
        Err(e) => return NativeResponse::error_with_dbg_msg("Failed to parse avatar ID.", e),
    };
    let avatar_path = {
        let avatars_read_lock = context.avatars.read().unwrap();
        match avatars_read_lock.get(&ulid) {
            Some(p) => p.clone(),
            None => return NativeResponse::error("Avatar not found!")
        }
    };
    let avatar_data = match fs::read(&avatar_path) {
        Ok(d) => d,
        Err(e) => return NativeResponse::error_with_dbg_msg("Failed to load avatar.", e)
    };
    let mime = match avatar_path
        .extension()
        .and_then(|x| x.to_str()) {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        _ => "application/octet-stream"
    };
    let encoded_avatar_data = encode_avatar_to_string(avatar_data);
    return NativeResponse::success(NativeResponseData::GetAvatarResult {
        data: encoded_avatar_data,
        mime: mime.to_owned(),
    })
}
