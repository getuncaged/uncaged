//! Settings for cloud agent functionality.
//!
//! This module contains user-specific settings for cloud agent features,
//! such as remembering the last selected environment.

use std::collections::HashMap;

use settings::macros::define_settings_group;
use settings::SupportedPlatforms;

use crate::server::ids::SyncId;

#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(description = "Selected third-party harness model.")]
pub struct HarnessModelSelection {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_level: Option<String>,
}

define_settings_group!(CloudAgentSettings, settings: [
    last_selected_environment_id: LastSelectedEnvironmentId {
        type: Option<SyncId>,
        default: None,
        supported_platforms: SupportedPlatforms::ALL,
        private: true,
    },
    last_selected_harness: LastSelectedHarness {
        type: Option<String>,
        default: None,
        supported_platforms: SupportedPlatforms::ALL,
        private: true,
    },
    last_selected_host: LastSelectedHost {
        type: Option<String>,
        default: None,
        supported_platforms: SupportedPlatforms::ALL,
        private: true,
    },
    last_selected_harness_model: LastSelectedHarnessModel {
        type: HashMap<String, HarnessModelSelection>,
        default: HashMap::new(),
        supported_platforms: SupportedPlatforms::ALL,
        private: true,
    }
]);

impl CloudAgentSettings {}
