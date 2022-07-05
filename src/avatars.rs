use std::{cmp, fs};
use std::fs::{DirEntry};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;
use base64::STANDARD_NO_PAD;
use indexmap::IndexMap;
use ulid::Ulid;
use crate::{AppContext, NativeResponseEvent, write_native_event};
use crate::storage::custom_avatars_path;

pub fn build_avatar_path(custom_avatars_path: &Path, ulid: Ulid, ext: &str) -> PathBuf {
    let filename = ulid.to_string() + "." + ext;
    custom_avatars_path.join(filename)
}

pub fn list_avatars(custom_avatars_path: &Path) -> IndexMap<Ulid, PathBuf> {
    if let Ok(r) = fs::read_dir(custom_avatars_path) {
        // Sort by creation time descending
        let mut dir_entries: Vec<(DirEntry, SystemTime)> = r.filter_map(|f| f.ok())
            .map(|e| {
                let t = e.metadata()
                    .and_then(|m| m.created())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (e, t)
            })
            .collect();
        dir_entries.sort_by_key(|p| cmp::Reverse(p.1));
        return dir_entries
            .iter()
            .filter_map(|f| {
                let path = f.0.path();
                path.file_stem()
                    .and_then(|f| f.to_str())
                    .and_then(|f| Ulid::from_str(f).ok())
                    .map(|ulid| (ulid, path))
            })
            .collect()
    }

    return IndexMap::new()
}

pub fn update_and_native_notify_avatars(context: &AppContext) {
    let avatars_path = custom_avatars_path(context);
    let avatars = list_avatars(&avatars_path);

    let avatars_as_ulids = avatars.keys().map(|u| u.to_string()).collect();

    *context.avatars.write().unwrap() = avatars;

    write_native_event(NativeResponseEvent::AvatarsUpdated {
        avatars: avatars_as_ulids
    });
}

const AVATAR_ENCODING: base64::Config = STANDARD_NO_PAD;
pub fn encode_avatar_to_string(avatar: Vec<u8>) -> String {
    base64::encode_config(avatar, AVATAR_ENCODING)
}