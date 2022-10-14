use crate::ipc::notify_update_avatars;
use crate::native_req::NativeMessageDeleteAvatar;
use crate::native_resp::NativeResponseData;
use crate::profiles::ProfilesIniState;
use crate::{AppContext, NativeResponse};
use std::fs;
use std::str::FromStr;
use ulid::Ulid;

pub fn process_delete_avatar(
    context: &AppContext,
    msg: NativeMessageDeleteAvatar,
    profiles: &ProfilesIniState,
) -> NativeResponse {
    let ulid = match Ulid::from_str(&msg.avatar) {
        Ok(u) => u,
        Err(e) => return NativeResponse::error_with_dbg_msg("Failed to parse avatar ID.", e),
    };

    let avatar_path = {
        let avatars_read_lock = context.avatars.read().unwrap();
        match avatars_read_lock.get(&ulid) {
            Some(p) => p.clone(),
            None => return NativeResponse::error("Avatar not found!"),
        }
    };

    if let Err(e) = fs::remove_file(avatar_path) {
        return NativeResponse::error_with_dbg_msg("Failed to delete avatar file.", e);
    }

    notify_update_avatars(context, profiles);

    NativeResponse::success(NativeResponseData::AvatarDeleted)
}
