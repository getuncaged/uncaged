use std::sync::Arc;

use warp_core::features::FeatureFlag;
use warp_core::settings::Setting;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::auth::auth_state::AuthState;
use crate::report_if_error;
use crate::settings::input::InputBoxType;
use crate::settings::{
    AISettings, FontSettings, InputSettings, PrivacySettings, ThemeSettings, ThinkingDisplayMode,
};
use crate::terminal::session_settings::SessionSettings;
use crate::themes::theme::ThemeKind;

pub struct SettingsInitializer;

impl Default for SettingsInitializer {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsInitializer {
    pub fn new() -> Self {
        Self
    }

    /// A hook for changing settings values after a user is fetched from the server.
    ///
    /// Specifically useful for adjusting settings for first-time users when the default value of a
    /// setting as set in define_settings_group! is no longer the desired default value,
    /// but we don't want to change it for existing users (which is what would happen if we changed the
    /// default value in define_settings_group! in code).
    pub fn handle_user_fetched(&self, auth_state: Arc<AuthState>, ctx: &mut ModelContext<Self>) {
        /// We use a font-size of 16px (12pt) on Windows to more closely match the default font size of
        /// Windows terminal.
        const DEFAULT_WINDOWS_MONOSPACE_FONT_SIZE: f32 = 16.;

        if auth_state.is_onboarded() == Some(false) {
            PrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                // Previously, secret redaction had a built-in default set of regexes that users couldn't change.
                // We want to add that default list to all existing users' lists, so we don't regress their current secret redaction experience.
                // However, for new users, we don't want to add these defaults without their explicit action, so we disable adding them here.
                settings.disable_default_regex_trigger(ctx);
            });

            if FeatureFlag::DefaultAdeberryTheme.is_enabled() {
                log::debug!("Setting default theme to Adeberry for new user");
                ThemeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    if *settings.theme_kind.value() == ThemeKind::Phenomenon {
                        report_if_error!(settings.theme_kind.set_value(ThemeKind::Adeberry, ctx));
                    }
                });
            }

            if cfg!(windows) {
                log::debug!("Setting default font size to 16px (12pt) for a new Windows user");
                FontSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .monospace_font_size
                        .set_value(DEFAULT_WINDOWS_MONOSPACE_FONT_SIZE, ctx));
                })
            }

            let did_update_input_type = InputSettings::handle(ctx).update(ctx, |settings, ctx| {
                if !settings.input_box_type.is_value_explicitly_set()
                    && *settings.input_box_type.value() == InputBoxType::Classic
                {
                    log::debug!("Setting default input type to Warp prompt for new user");
                    report_if_error!(settings
                        .input_box_type
                        .set_value(InputBoxType::Universal, ctx));
                    ctx.notify();
                    return true;
                }
                false
            });
            // Keep honor_ps1 in sync: Universal input requires honor_ps1 = false.
            if did_update_input_type {
                SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                    if *settings.honor_ps1.value() {
                        report_if_error!(settings.honor_ps1.set_value(false, ctx));
                    }
                });
            }
        }

        // Migrate NLD settings when AgentView is enabled.
        //
        // Explicitly set `nld_in_terminal_enabled_internal` for all users if
        // it has not previously been set.
        //
        // For existing users, when the old, previously-global autodetection setting
        // (`ai_autodetection_enabled_internal`) true, set `nld_in_terminal_enabled_internal` to
        // true. Otherwise, explicitly set to `false`.
        //
        // Any further user modification of the setting will be via explicit update, so it'll
        // be exempt from this logic, which is effectively one-time upon first startup of a binary
        // containing this logic.
        //
        // TODO(zachbai): Remove this approximately 6 weeks from 2/5/26.
        if FeatureFlag::AgentView.is_enabled() {
            AISettings::handle(ctx).update(ctx, |ai_settings, ctx| {
                if ai_settings
                    .nld_in_terminal_enabled_internal
                    .is_value_explicitly_set()
                {
                    return;
                }

                let is_existing_user = auth_state.is_onboarded() == Some(true);
                let was_global_autodetection_enabled_for_existing_user =
                    *ai_settings.ai_autodetection_enabled_internal && is_existing_user;
                report_if_error!(ai_settings
                    .nld_in_terminal_enabled_internal
                    .set_value(was_global_autodetection_enabled_for_existing_user, ctx));
            });
        }

        // Migrate the old `KeepThinkingExpanded` bool setting to the new
        // `ThinkingDisplayMode` enum setting.
        //
        // The old setting was a boolean (default: false) that controlled whether
        // agent thinking blocks stayed expanded after streaming. It has been
        // replaced by a three-option enum: ShowAndCollapse (default),
        // AlwaysShow, and NeverShow.
        //
        // If the user explicitly set `KeepThinkingExpanded` to `true`, migrate
        // them to `ThinkingDisplayMode::AlwaysShow` so they don't lose their
        // preference when updating to the new client.
        //
        // TODO(jefflloyd): Remove this approximately 6 weeks from 3/19/26.
        {
            use warp_core::user_preferences::GetUserPreferences as _;

            AISettings::handle(ctx).update(ctx, |ai_settings, ctx| {
                // If the new setting already has a value in preferences, the
                // migration has already run (or the user set it directly).
                let new_key_exists = ctx
                    .private_user_preferences()
                    .read_value("ThinkingDisplayMode")
                    .unwrap_or_default()
                    .is_some();

                if new_key_exists {
                    return;
                }

                // Read the old boolean setting directly from preferences
                // because `KeepThinkingExpanded` has been removed from the
                // `AISettings` struct — there is no typed field left to query.
                let old_value_was_true = ctx
                    .private_user_preferences()
                    .read_value("KeepThinkingExpanded")
                    .unwrap_or_default()
                    .and_then(|v| serde_json::from_str::<bool>(&v).ok())
                    == Some(true);

                if old_value_was_true {
                    report_if_error!(ai_settings
                        .thinking_display_mode
                        .set_value(ThinkingDisplayMode::AlwaysShow, ctx));
                }

                // Clean up the old key.
                let _ = ctx
                    .private_user_preferences()
                    .remove_value("KeepThinkingExpanded");
            });
        }
    }
}

impl Entity for SettingsInitializer {
    type Event = ();
}

/// Mark CloudPreferencesSyncer as global application state.
impl SingletonEntity for SettingsInitializer {}

/// Uncaged: input mode is a toggle, not a guess -- turn autodetection off once.
///
/// Upstream defaulted `ai_autodetection_enabled_internal` to true, and the first-run onboarding
/// callout offered an "Enable Natural Language Detection" checkbox that set
/// `nld_in_terminal_enabled_internal`. Both write an explicit value, and defaults only apply to
/// keys that are absent -- so changing the defaults reaches new installs only, while every install
/// that has ever run keeps classifying.
///
/// That classification is not cheap in a shipped build: `script/*/bundle` compiles in
/// `nld_classifier_v3`, so it is a BERT-tiny ONNX forward pass per keystroke, concentrated on the
/// first characters of a line (once the completer can describe the first token, the classifier
/// short-circuits). It can also move the input between Shell and AI after the user has already
/// chosen a mode.
///
/// So: clear both, exactly once, and record that we did. Anyone who wants the classifier back can
/// turn it on in Settings -> AI and it stays on -- the marker is already set, so this never runs
/// again and never overrides that choice.
///
/// Deliberately NOT in `SettingsInitializer::handle_user_fetched` alongside the upstream
/// migrations: that runs only after a successful server user fetch. Uncaged has no accounts and
/// points its server config at an unroutable sentinel, so nothing there ever executes. This is
/// called from `settings::init` instead, which runs on every launch.
pub fn apply_uncaged_autodetection_reset(ctx: &mut AppContext) {
    AISettings::handle(ctx).update(ctx, |ai_settings, ctx| {
        if *ai_settings.uncaged_autodetection_reset_applied {
            return;
        }

        if *ai_settings.ai_autodetection_enabled_internal {
            report_if_error!(ai_settings
                .ai_autodetection_enabled_internal
                .set_value(false, ctx));
        }
        if *ai_settings.nld_in_terminal_enabled_internal {
            report_if_error!(ai_settings
                .nld_in_terminal_enabled_internal
                .set_value(false, ctx));
        }

        report_if_error!(ai_settings
            .uncaged_autodetection_reset_applied
            .set_value(true, ctx));
    });
}
