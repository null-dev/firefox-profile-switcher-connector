use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use ulid::{Ulid};
use crate::{AppContext, NativeResponse};
use crate::avatars::encode_avatar_to_string;
use crate::ipc::notify_update_avatars;
use crate::native_req::{NativeMessageDeleteAvatar, NativeMessageGetAvatar};
use crate::native_resp::NativeResponseData;
use crate::profiles::ProfilesIniState;

pub fn process_cmd_delete_avatar(context: &AppContext, profiles: ProfilesIniState, msg: NativeMessageDeleteAvatar) -> NativeResponse {
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
    if let Err(e) = fs::remove_file(avatar_path) {
        return NativeResponse::error_with_dbg_msg("Failed to delete avatar file.", e)
    }

    notify_update_avatars(context, &profiles);

    return NativeResponse::success(NativeResponseData::AvatarDeleted)
}
