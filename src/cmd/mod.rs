mod initialize;
mod launch_profile;
mod create_profile;
mod delete_profile;
mod update_profile;
mod update_options;
mod close_manager;

use crate::state::AppState;
use crate::native_req::NativeMessage;
use crate::profiles::ProfilesIniState;
use crate::native_resp::NativeResponse;
use crate::cmd::initialize::process_cmd_initialize;
use crate::cmd::launch_profile::process_cmd_launch_profile;
use crate::cmd::create_profile::process_cmd_create_profile;
use crate::cmd::delete_profile::process_cmd_delete_profile;
use crate::cmd::update_profile::process_cmd_update_profile;
use crate::cmd::update_options::process_cmd_update_options;
use crate::cmd::close_manager::process_cmd_close_manager;

// === COMMANDS ===

pub fn execute_cmd_for_message(app_state: &mut AppState,
                               profiles: &mut ProfilesIniState,
                               msg: NativeMessage) -> NativeResponse {
    match msg {
        NativeMessage::Initialize(msg) => process_cmd_initialize(app_state, profiles, msg),
        NativeMessage::LaunchProfile(msg) => process_cmd_launch_profile(app_state, profiles, msg),
        NativeMessage::CreateProfile(msg) => process_cmd_create_profile(app_state, profiles, msg),
        NativeMessage::DeleteProfile(msg) => process_cmd_delete_profile(app_state, profiles, msg),
        NativeMessage::UpdateProfile(msg) => process_cmd_update_profile(app_state, profiles, msg),
        NativeMessage::UpdateOptions(msg) => process_cmd_update_options(app_state, profiles, msg),
        NativeMessage::CloseManager => process_cmd_close_manager(app_state, &profiles)
    }
}