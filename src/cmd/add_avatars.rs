use std::fs;
use ulid::Ulid;
use crate::{AppContext, NativeResponse};
use crate::avatars::build_avatar_path;
use crate::ipc::notify_update_avatars;
use crate::native_resp::NativeResponseData::AvatarsUpdated;
use crate::profiles::ProfilesIniState;
use crate::storage::{custom_avatars_path};

pub fn process_add_avatars(context: &AppContext, profiles: &ProfilesIniState) -> NativeResponse {
    // Pick avatar
    let result = match context.windowing.open_avatar_picker() {
        Some(r) => r,
        None => Vec::new()
    };

    // Load and create avatars dir
    let avatars_dir = custom_avatars_path(context);
    if let Err(e) = fs::create_dir_all(&avatars_dir) {
        return NativeResponse::error_with_dbg_msg("Could not create folder for avatars.", e);
    }

    // Verify avatars are the correct size
    for path in &result {
        let metadata = match path.metadata() {
            Ok(m) => m,
            Err(e) => return NativeResponse::error_with_dbg_msg(format!("Could not load information on avatar: {}", path.display()), e)
        };

        // 500 KB
        if metadata.len() > 500000 {
            return NativeResponse::error(format!("Avatar {} is too large, max size is 500 KB", path.display()));
        }
    }

    // Save all the avatars
    for path in result {
        let extension = match path.extension().and_then(|x| x.to_str()) {
            Some(e) => e,
            None => return NativeResponse::error(&format!("Invalid avatar: {}", path.display()))
        };
        let target_path = build_avatar_path(
            &avatars_dir,
            Ulid::new(),
            &extension.to_lowercase(),
        );
        if let Err(e) = fs::copy(&path, target_path) {
            return NativeResponse::error(&format!("Failed to save avatar: {}. Error: {:?}", path.display(), e))
        }
    }

    notify_update_avatars(context, profiles);

    NativeResponse::success(AvatarsUpdated)
}