pub use cloud_object_models::{
    AIExecutionProfile, ActionPermission, AskUserQuestionPermission, CloudAIExecutionProfile,
    CloudAIExecutionProfileModel, ComputerUsePermission, RunAgentsPermission, WriteToPtyPermission,
    PROFILE_NAME_MAX_LENGTH,
};
use uncaged_ui::{AppContext, SingletonEntity};

use super::llms::{LLMContextWindow, LLMInfo, LLMPreferences, LLMProvider};
use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{
    GenericStringObjectFormat, GenericStringObjectUniqueKey, JsonObjectType, UniquePer,
};
#[cfg(not(feature = "agent_mode_evals"))]
use crate::settings::AISettings;

pub mod editor;
pub mod model_menu_items;
pub mod profiles;

fn effective_base_model<'a>(profile: &AIExecutionProfile, app: &'a AppContext) -> &'a LLMInfo {
    let prefs = LLMPreferences::as_ref(app);
    profile
        .base_model
        .as_ref()
        .and_then(|id| prefs.get_llm_info(id))
        .unwrap_or_else(|| prefs.get_default_base_model())
}

#[cfg(not(feature = "agent_mode_evals"))]
pub fn create_default_from_legacy_settings(app: &AppContext) -> AIExecutionProfile {
    // Note that the legacy "Autonomy" and "Code Access" settings are not imported here.
    // The "Code Access" setting defaulted to "Always Ask", which is the most restrictive, so
    // it's impossible for us to infer some hesitancy about autonomy from the setting and we should
    // ignore it. The same applies to "Autonomy".
    let ai_settings = AISettings::as_ref(app);
    AIExecutionProfile {
        name: "Default".to_string(),
        is_default_profile: true,
        command_denylist: ai_settings.agent_mode_command_execution_denylist.clone(),
        // We initialize the command allowlist to be anything the user added, excluding all
        // the pre-populated defaults.
        command_allowlist: ai_settings
            .agent_mode_command_execution_allowlist
            .iter()
            .filter(|cmd| !crate::settings::DEFAULT_COMMAND_EXECUTION_ALLOWLIST.contains(cmd))
            .cloned()
            .collect(),
        directory_allowlist: ai_settings.agent_mode_coding_file_read_allowlist.clone(),
        ..Default::default()
    }
}

pub trait AIExecutionProfileAppExt {
    fn configurable_context_window(&self, app: &AppContext) -> Option<LLMContextWindow>;

    fn context_window_display_value(&self, app: &AppContext) -> Option<u32>;
    fn context_window_limit_for_request(&self, app: &AppContext) -> Option<u32>;
}

impl AIExecutionProfileAppExt for AIExecutionProfile {
    fn configurable_context_window(&self, app: &AppContext) -> Option<LLMContextWindow> {
        let llm = effective_base_model(self, app);
        if has_configurable_context_window(llm) {
            Some(llm.context_window.clone())
        } else {
            None
        }
    }

    fn context_window_display_value(&self, app: &AppContext) -> Option<u32> {
        let cw = self.configurable_context_window(app)?;
        Some(self.context_window_limit.unwrap_or(cw.default_max))
    }
    fn context_window_limit_for_request(&self, app: &AppContext) -> Option<u32> {
        let llm = effective_base_model(self, app);
        if !has_configurable_context_window(llm) {
            return None;
        }

        self.context_window_limit
            .map(|limit| limit.clamp(llm.context_window.min, llm.context_window.max))
    }
}

/// OpenAI models keep their fixed context window; their expanded (long-context
/// priced) windows are not offered.
pub(crate) fn has_configurable_context_window(llm: &LLMInfo) -> bool {
    llm.context_window.is_configurable
        && llm.context_window.max > 0
        && llm.provider != LLMProvider::OpenAI
}

impl StringModel for AIExecutionProfile {
    type CloudObjectType = CloudAIExecutionProfile;

    fn model_type_name(&self) -> &'static str {
        "AIExecutionProfile"
    }

    fn should_enforce_revisions() -> bool {
        true
    }

    fn model_format() -> GenericStringObjectFormat {
        GenericStringObjectFormat::Json(JsonObjectType::AIExecutionProfile)
    }

    fn should_show_activity_toasts() -> bool {
        false
    }

    fn display_name(&self) -> String {
        // Handles case where default profile was previously created and named "Untitled"
        if self.is_default_profile {
            "Default".to_string()
        } else if self.name.trim().is_empty() {
            "Untitled".to_string()
        } else {
            self.name.clone()
        }
    }

    fn uniqueness_key(&self) -> Option<GenericStringObjectUniqueKey> {
        // We want to prevent the creation of several default profiles per user. If it's not the default
        // profile, then there can be many.
        self.is_default_profile
            .then_some(GenericStringObjectUniqueKey {
                key: "default".to_string(),
                unique_per: UniquePer::User,
            })
    }

    fn renders_in_warp_drive(&self) -> bool {
        false
    }
}

impl JsonModel for AIExecutionProfile {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::AIExecutionProfile
    }
}
