use std::fmt::Display;

use regex::Regex;
use serde::{Deserialize, Serialize};
use settings::macros::{define_settings_group, maybe_define_setting, register_settings_events};
use settings::{Setting, SupportedPlatforms};
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

pub trait RegexDisplayInfo {
    fn pattern(&self) -> &str;
    fn name(&self) -> Option<&str>;
}

pub const CRASH_REPORTING_ENABLED_DEFAULTS_KEY: &str = "CrashReportingEnabled";
pub const CLOUD_CONVERSATION_STORAGE_ENABLED_DEFAULTS_KEY: &str = "CloudConversationStorageEnabled";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(description = "A custom regex pattern for detecting and redacting secrets.")]
pub struct CustomSecretRegex {
    #[serde(with = "serde_regex")]
    #[schemars(with = "String", description = "The regex pattern to match secrets.")]
    pub pattern: Regex,
    #[serde(default)]
    #[schemars(description = "Optional display name for this secret pattern.")]
    pub name: Option<String>,
}

impl CustomSecretRegex {
    pub fn pattern(&self) -> &Regex {
        &self.pattern
    }
}

impl RegexDisplayInfo for CustomSecretRegex {
    fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl Display for CustomSecretRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.pattern.as_str())
    }
}

impl PartialEq for CustomSecretRegex {
    /// We do not factor in the name to equality checks --
    /// if the regex is the same, then the regex is the same.
    /// This allows us to avoid adding duplicate regexes.
    fn eq(&self, other: &Self) -> bool {
        self.pattern.as_str() == other.pattern.as_str()
    }
}

impl settings_value::SettingsValue for CustomSecretRegex {}

define_settings_group!(WarpDrivePrivacySettings, settings: [
    is_crash_reporting_enabled: IsCrashReportingEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        private: false,
        storage_key: "CrashReportingEnabled",
        toml_path: "privacy.crash_reporting_enabled",
        description: "Whether crash reports are sent.",
    },
    is_cloud_conversation_storage_enabled: IsCloudConversationStorageEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        private: false,
        storage_key: "CloudConversationStorageEnabled",
        toml_path: "agents.cloud_conversation_storage_enabled",
        description: "Whether conversations are stored in the cloud.",
    },
]);

maybe_define_setting!(CustomSecretRegexList, group: PrivacySettings, {
    type: Vec<CustomSecretRegex>,
    default: Vec::new(),
    supported_platforms: SupportedPlatforms::ALL,
    private: false,
    toml_path: "privacy.custom_secret_regex_list",
    description: "Custom regex patterns for detecting and redacting secrets.",
});

maybe_define_setting!(HasInitializedDefaultSecretRegexes, group: PrivacySettings, {
    type: bool,
    default: false,
    supported_platforms: SupportedPlatforms::ALL,
    private: true,
});

/// Singleton model for managing the user's privacy settings (whether the user has enabled crash
/// reporting and cloud conversation storage).
pub struct PrivacySettings {
    pub is_crash_reporting_enabled: bool,
    pub is_cloud_conversation_storage_enabled: bool,
    pub has_initialized_default_secret_regexes: HasInitializedDefaultSecretRegexes,
    /// List of user defined secret regexes.
    /// It's a [Vec<CustomSecretRegex>], but also a user setting.
    pub user_secret_regex_list: CustomSecretRegexList,
}

impl PrivacySettings {
    /// Registers a singleton PrivacySettings model on `app`.
    ///
    /// We expose this function publicly (while keeping the constructor private) to prevent
    /// instantiation another PrivacySettings struct, in the case where a developer might be
    /// unaware that it is registered as a singleton model.
    pub fn register_singleton(ctx: &mut AppContext) {
        let handle = ctx.add_singleton_model(PrivacySettings::new);

        register_settings_events!(
            PrivacySettings,
            user_secret_regex_list,
            CustomSecretRegexList,
            handle,
            ctx
        );
    }

    /// Returns a new PrivacySettings object initialized from locally cached values.
    fn new(ctx: &mut ModelContext<Self>) -> Self {
        // Initialize from `WarpDrivePrivacySettings`, which is the source of truth for these
        // booleans.
        let warp_drive_privacy = WarpDrivePrivacySettings::as_ref(ctx);
        let is_crash_reporting_enabled = *warp_drive_privacy.is_crash_reporting_enabled.value();
        let is_cloud_conversation_storage_enabled = *warp_drive_privacy
            .is_cloud_conversation_storage_enabled
            .value();

        // Listen for changes to the cloud model and update ourselves when they happen.
        ctx.subscribe_to_model(
            &WarpDrivePrivacySettings::handle(ctx),
            |me, _, event, ctx| {
                let privacy_settings = WarpDrivePrivacySettings::as_ref(ctx);
                match event {
                    WarpDrivePrivacySettingsChangedEvent::IsCrashReportingEnabled { .. } => {
                        me.set_is_crash_reporting_enabled(
                            *privacy_settings.is_crash_reporting_enabled.value(),
                            ctx,
                        );
                    }
                    WarpDrivePrivacySettingsChangedEvent::IsCloudConversationStorageEnabled {
                        ..
                    } => {
                        me.set_is_cloud_conversation_storage_enabled(
                            *privacy_settings
                                .is_cloud_conversation_storage_enabled
                                .value(),
                            ctx,
                        );
                    }
                }
            },
        );

        let user_secret_regex_list: CustomSecretRegexList =
            CustomSecretRegexList::new_from_storage(ctx);
        let has_initialized_default_secret_regexes: HasInitializedDefaultSecretRegexes =
            HasInitializedDefaultSecretRegexes::new_from_storage(ctx);

        Self {
            is_crash_reporting_enabled,
            is_cloud_conversation_storage_enabled,
            user_secret_regex_list,
            has_initialized_default_secret_regexes,
        }
    }

    /// Constructor for tests only.
    #[cfg(test)]
    pub fn mock(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            is_crash_reporting_enabled: true,
            is_cloud_conversation_storage_enabled: true,
            user_secret_regex_list: CustomSecretRegexList::new(None),
            has_initialized_default_secret_regexes: HasInitializedDefaultSecretRegexes::new(None),
        }
    }

    /// Sets `is_crash_reporting_enabled` to the given value.
    ///
    /// Additionally, this writes the given value to the user's local defaults, and additionally
    /// sends a request to update the user's `is_crash_reporting_enabled` value stored server-side.
    /// Finally, emits a `PrivacySettingsEvent::UpdateIsCrashReportingEnabled` event.
    pub fn set_is_crash_reporting_enabled(
        &mut self,
        new_value: bool,
        ctx: &mut ModelContext<PrivacySettings>,
    ) {
        let old_value = self.is_crash_reporting_enabled;
        if new_value != old_value {
            self.is_crash_reporting_enabled = new_value;

            WarpDrivePrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                log::info!("Setting is_crash_reporting_enabled to {new_value}");
                let _ = settings
                    .is_crash_reporting_enabled
                    .set_value(new_value, ctx);
            });

            ctx.emit(PrivacySettingsChangedEvent::UpdateIsCrashReportingEnabled {
                old_value,
                new_value,
            });
            ctx.notify();
        }
    }

    pub fn set_is_cloud_conversation_storage_enabled(
        &mut self,
        new_value: bool,
        ctx: &mut ModelContext<PrivacySettings>,
    ) {
        let old_value = self.is_cloud_conversation_storage_enabled;
        if new_value == old_value {
            return;
        }

        self.is_cloud_conversation_storage_enabled = new_value;

        WarpDrivePrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
            log::info!("Setting is_cloud_conversation_storage_enabled to {new_value}");
            let _ = settings
                .is_cloud_conversation_storage_enabled
                .set_value(new_value, ctx);
        });

        ctx.emit(
            PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled {
                old_value,
                new_value,
            },
        );
        ctx.notify();
    }

    pub fn remove_user_secret_regex(&mut self, idx: &usize, ctx: &mut ModelContext<Self>) {
        let mut new_user_secret_regex_list = self.user_secret_regex_list.to_vec();
        new_user_secret_regex_list.remove(*idx);
        if self
            .user_secret_regex_list
            .set_value(new_user_secret_regex_list, ctx)
            .is_err()
        {
            log::error!("Custom Secret Regex List failed to serialize")
        }
    }

    /// Initializes the custom secret regex list with the default regexes, provided
    /// non matches can be found.
    /// This can be called when a user first enables secret redaction.
    pub fn add_all_recommended_regex(&mut self, ctx: &mut ModelContext<Self>) {
        let mut new_user_secret_regex_list = self.user_secret_regex_list.to_vec();
        let num_existing_regexes = new_user_secret_regex_list.len();

        // Add all the default regexes if they don't already exist
        for default_regex in crate::terminal::model::secrets::regexes::DEFAULT_REGEXES_WITH_NAMES {
            if let Ok(regex) = Regex::new(default_regex.pattern) {
                let custom_regex = CustomSecretRegex {
                    pattern: regex,
                    name: Some(default_regex.name.to_string()),
                };
                if !new_user_secret_regex_list.contains(&custom_regex) {
                    new_user_secret_regex_list.push(custom_regex);
                }
            } else {
                log::error!("Failed to compile default regex: {}", default_regex.pattern);
            }
        }

        if num_existing_regexes == new_user_secret_regex_list.len() {
            return;
        }

        if self
            .user_secret_regex_list
            .set_value(new_user_secret_regex_list, ctx)
            .is_err()
        {
            log::error!("Failed to serialize default regexes to custom secret regex list")
        }

        ctx.notify();
    }

    /// Initializes the custom secret regex list with the default regexes.
    /// This will only be executed once per user, and only if they haven't already initialized.
    pub fn initialize_default_regexes_once(&mut self, ctx: &mut ModelContext<Self>) {
        // Only initialize if we haven't done so before
        if !*self.has_initialized_default_secret_regexes.value() {
            self.add_all_recommended_regex(ctx);

            // Mark as initialized
            if self
                .has_initialized_default_secret_regexes
                .set_value(true, ctx)
                .is_err()
            {
                log::error!("Failed to set has_initialized_default_secret_regexes flag");
            }
        }
    }
}

/// Events emitted when PrivacySettings is updated.
#[derive(Clone, Copy)]
pub enum PrivacySettingsChangedEvent {
    UpdateIsCrashReportingEnabled {
        old_value: bool,
        new_value: bool,
    },
    UpdateIsCloudConversationStorageEnabled {
        old_value: bool,
        new_value: bool,
    },
    CustomSecretRegexList {
        change_event_reason: ChangeEventReason,
    },
    HasInitializedDefaultSecretRegexes {
        change_event_reason: ChangeEventReason,
    },
}

impl Entity for PrivacySettings {
    type Event = PrivacySettingsChangedEvent;
}

impl SingletonEntity for PrivacySettings {}
