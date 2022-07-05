mod initialize;
mod launch_profile;
mod create_profile;
mod delete_profile;
mod update_profile;
mod update_options;
mod close_manager;
mod add_avatars;
mod get_avatar;
mod delete_avatar;

use crate::state::AppState;
use crate::native_req::NativeMessage;
use crate::native_resp::NativeResponse;
use crate::cmd::initialize::process_cmd_initialize;
use crate::cmd::launch_profile::process_cmd_launch_profile;
use crate::cmd::create_profile::process_cmd_create_profile;
use crate::cmd::delete_profile::process_cmd_delete_profile;
use crate::cmd::update_profile::process_cmd_update_profile;
use crate::cmd::update_options::process_cmd_update_options;
use crate::cmd::close_manager::process_cmd_close_manager;
use crate::{AppContext};
use crate::cmd::add_avatars::process_add_avatars;
use crate::cmd::delete_avatar::process_delete_avatar;
use crate::cmd::get_avatar::process_get_avatar;
use crate::profiles::read_profiles;

// === COMMANDS ===

macro_rules! profiles {
    ($app_state:ident)=>{
        match read_profiles(&$app_state.config, &$app_state.config_dir) {
            Ok(p) => p,
            Err(e) => {
                return NativeResponse::error_with_dbg_msg("Failed to load profile list.", e);
            }
        }
    };
}

pub fn execute_init_cmd(app_state: &mut AppState,
                        msg: NativeMessage) -> NativeResponse {
    match msg {
        NativeMessage::Initialize(msg) => process_cmd_initialize(app_state, profiles!(app_state), msg),
        _ => NativeResponse::error_with_dbg_str("Connector is not ready yet!", "Connector has not been initialized.".to_owned())
    }
}

pub fn execute_cmd_for_message(context: &AppContext,
                               msg: NativeMessage) -> NativeResponse {
    let state = context.state;
    match msg {
        NativeMessage::Initialize(_) => NativeResponse::error("Connector cannot be initialized multiple times!"),
        NativeMessage::LaunchProfile(msg) => process_cmd_launch_profile(context, &profiles!(state), msg),
        NativeMessage::CreateProfile(msg) => process_cmd_create_profile(context, profiles!(state), msg),
        NativeMessage::DeleteProfile(msg) => process_cmd_delete_profile(context, profiles!(state), msg),
        NativeMessage::UpdateProfile(msg) => process_cmd_update_profile(context, profiles!(state), msg),
        NativeMessage::UpdateOptions(msg) => process_cmd_update_options(context, &profiles!(state), msg),
        NativeMessage::CloseManager => process_cmd_close_manager(context, &profiles!(state)),
        NativeMessage::AddAvatars => process_add_avatars(context, &profiles!(state)),
        NativeMessage::GetAvatar(msg) => process_get_avatar(context, msg),
        NativeMessage::DeleteAvatar(msg) => process_delete_avatar(context, msg, &profiles!(state)),
    }
}