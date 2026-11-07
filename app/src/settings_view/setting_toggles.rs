//! Command-palette toggles for settings that have no settings page in Uncaged (secret
//! redaction, cloud conversation storage, SSH Warpification and Warp Drive). The pages that
//! used to host them were never reachable in Uncaged; the settings themselves stay editable in
//! the settings file and through these palette entries.

use settings::{Setting as _, ToggleableSetting as _};
use warp_core::features::FeatureFlag;
use warpui::keymap::ContextPredicate;
use warpui::{id, Action, AppContext, SingletonEntity};

use super::{
    flags, SettingActionPairContexts, SettingActionPairDescriptions, SettingsAction,
    ToggleSettingActionPair,
};
use crate::drive::settings::WarpDriveSettings;
use crate::report_if_error;
use crate::settings::PrivacySettings;
use crate::terminal::safe_mode_settings::SafeModeSettings;
use crate::terminal::warpify::settings::WarpifySettings;

#[derive(Clone, Copy, Debug)]
pub enum SettingToggleAction {
    SecretRedaction,
    CloudConversationStorage,
    SshWarpification,
    WarpDrive,
}

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    let toggle = |action| builder(SettingsAction::ToggleSetting(action));

    let mut toggle_binding_pairs = vec![
        ToggleSettingActionPair::new(
            "secret redaction",
            toggle(SettingToggleAction::SecretRedaction),
            context,
            flags::SAFE_MODE_FLAG,
        ),
        ToggleSettingActionPair::new(
            "cloud AI conversation storage",
            toggle(SettingToggleAction::CloudConversationStorage),
            &(context.clone()
                & id!(flags::IS_ANY_AI_ENABLED)
                & id!(flags::CLOUD_CONVERSATION_STORAGE_EDITABLE_FLAG)),
            flags::CLOUD_CONVERSATION_STORAGE_FLAG,
        )
        .with_enabled(|| FeatureFlag::CloudConversations.is_enabled()),
        ToggleSettingActionPair::custom(
            SettingActionPairDescriptions::new("Enable Warp Drive", "Disable Warp Drive"),
            toggle(SettingToggleAction::WarpDrive),
            SettingActionPairContexts::new(
                context.clone() & !id!(flags::ENABLE_WARP_DRIVE) & !id!("IsAnonymousUser"),
                context.clone() & id!(flags::ENABLE_WARP_DRIVE) & !id!("IsAnonymousUser"),
            ),
            None,
        )
        .with_enabled(|| FeatureFlag::OpenWarpNewSettingsModes.is_enabled()),
    ];

    if WarpifySettings::as_ref(app)
        .enable_ssh_warpification
        .is_supported_on_current_platform()
    {
        toggle_binding_pairs.push(ToggleSettingActionPair::new(
            "SSH Warpification",
            toggle(SettingToggleAction::SshWarpification),
            context,
            flags::SSH_WARPIFICATION_CONTEXT_FLAG,
        ));
    }

    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(toggle_binding_pairs, app);
}

/// Flips the setting behind `action` and saves it.
pub fn toggle_setting(action: SettingToggleAction, ctx: &mut AppContext) {
    match action {
        SettingToggleAction::SecretRedaction => {
            SafeModeSettings::handle(ctx).update(ctx, |settings, ctx| {
                report_if_error!(settings.safe_mode_enabled.toggle_and_save_value(ctx));
            });
        }
        SettingToggleAction::CloudConversationStorage => {
            PrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                let enabled = settings.is_cloud_conversation_storage_enabled;
                settings.set_is_cloud_conversation_storage_enabled(!enabled, ctx);
            });
        }
        SettingToggleAction::SshWarpification => {
            WarpifySettings::handle(ctx).update(ctx, |settings, ctx| {
                report_if_error!(settings.enable_ssh_warpification.toggle_and_save_value(ctx));
            });
        }
        SettingToggleAction::WarpDrive => {
            WarpDriveSettings::handle(ctx).update(ctx, |settings, ctx| {
                report_if_error!(settings.enable_warp_drive.toggle_and_save_value(ctx));
            });
        }
    }
}
