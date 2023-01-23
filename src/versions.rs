use once_cell::sync::Lazy;
use semver::{VersionReq};
use crate::AppState;

/*
pub const MIN_VERSION_2: Lazy<VersionReq> = Lazy::new(|| VersionReq::parse(">=2.0.0").unwrap());

pub fn is_min_version_2(app_state: &AppState) -> bool {
    match &app_state.extension_version {
        None => false,
        Some(version) => MIN_VERSION_2.matches(&version)
    }
}
*/