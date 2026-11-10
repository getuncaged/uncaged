use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use ai::api_keys::{ApiKeyManager, ApiKeyManagerEvent, CustomEndpoint, CustomEndpointModel};
pub use ai::LLMId;
use serde::{de, Deserialize, Serialize};
use uncaged_agent_api as api;
use uncaged_core::features::FeatureFlag;
use uncaged_core::ui::icons::Icon;
use uncaged_core::user_preferences::GetUserPreferences;
use uncaged_ui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity};

use super::custom_model_routers::{self, CustomModelRouter, ModelConfigError};
use super::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::user_config::{WarpConfig, WarpConfigUpdateEvent};

/// Key for cached LLM metadata in user preferences.
///
/// Note: this key used to store a single [`AvailableLLMs`]
/// but was migrated to store a full [`ModelsByFeature`].
pub const MODELS_BY_FEATURE_CACHE_KEY: &str = "AvailableLLMs";
const CUSTOM_ENDPOINT_USAGE_FALLBACK_LABEL: &str = "Custom endpoint";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LLMUsageMetadata {
    pub request_multiplier: usize,
    pub credit_multiplier: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisableReason {
    AdminDisabled,
    OutOfRequests,
    ProviderOutage,
    RequiresUpgrade,
    Unavailable,
}

impl DisableReason {
    /// Returns a user-facing tooltip explaining why the model is disabled.
    pub fn tooltip_text(&self) -> &'static str {
        match self {
            DisableReason::AdminDisabled => "This model has been disabled by your team admin.",
            DisableReason::OutOfRequests => "Please upgrade your plan to make more requests.",
            DisableReason::ProviderOutage => {
                "This model is temporarily unavailable due to a provider outage."
            }
            DisableReason::RequiresUpgrade => "Please upgrade your plan to access this model.",
            DisableReason::Unavailable => "This model is unavailable.",
        }
    }

    /// Returns `true` when this disable reason means the user cannot use the model
    /// and we should clear their stored preference.
    ///
    /// `OutOfRequests` and `ProviderOutage` are transient and expected to
    /// resolve without user action, so we preserve the selection.
    fn should_clear_preference(&self) -> bool {
        match self {
            DisableReason::AdminDisabled
            | DisableReason::Unavailable
            | DisableReason::RequiresUpgrade => true,
            DisableReason::OutOfRequests | DisableReason::ProviderOutage => false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LLMSpec {
    pub cost: f32,
    pub quality: f32,
    pub speed: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LLMProvider {
    OpenAI,
    Anthropic,
    Google,
    Xai,
    Unknown,
}

impl LLMProvider {
    /// Maps an LLMProvider to its corresponding icon.
    pub fn icon(&self) -> Option<Icon> {
        match self {
            LLMProvider::OpenAI => Some(Icon::OpenAILogo),
            LLMProvider::Anthropic => Some(Icon::ClaudeLogo),
            LLMProvider::Google => Some(Icon::GeminiLogo),
            LLMProvider::Xai => None,
            LLMProvider::Unknown => None,
        }
    }

    /// Human-readable provider name for user-facing copy.
    pub fn display_name(&self) -> &'static str {
        match self {
            LLMProvider::OpenAI => "OpenAI",
            LLMProvider::Anthropic => "Anthropic",
            LLMProvider::Google => "Google",
            LLMProvider::Xai => "xAI",
            LLMProvider::Unknown => "this provider",
        }
    }
}

/// The host where an LLM can be routed to.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LLMModelHost {
    DirectApi,
    AwsBedrock,
    CustomEndpoint,
    GeminiEnterprise,
    #[serde(other)]
    Unknown,
}

/// Configuration for routing an LLM to a specific host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutingHostConfig {
    pub enabled: bool,
    pub model_routing_host: LLMModelHost,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LLMContextWindow {
    #[serde(default)]
    pub is_configurable: bool,
    #[serde(default)]
    pub min: u32,
    #[serde(default)]
    pub max: u32,
    #[serde(default)]
    pub default_max: u32,
}

/// Metadata about an LLM.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LLMInfo {
    pub display_name: String,
    pub base_model_name: String,
    pub id: LLMId,
    pub reasoning_level: Option<String>,
    pub usage_metadata: LLMUsageMetadata,
    pub description: Option<String>,
    pub disable_reason: Option<DisableReason>,
    pub vision_supported: bool,
    pub spec: Option<LLMSpec>,
    pub provider: LLMProvider,
    pub host_configs: HashMap<LLMModelHost, RoutingHostConfig>,
    pub discount_percentage: Option<f32>,
    pub context_window: LLMContextWindow,
}

impl<'de> Deserialize<'de> for LLMInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        /// Helper type that can deserialize host_configs from either:
        /// - A Vec (wire format from server)
        /// - A HashMap (cached format after commit a8a82421c3)
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum HostConfigsWire {
            Vec(Vec<RoutingHostConfig>),
            Map(HashMap<LLMModelHost, RoutingHostConfig>),
        }

        impl Default for HostConfigsWire {
            fn default() -> Self {
                HostConfigsWire::Vec(Vec::new())
            }
        }

        #[derive(Deserialize)]
        struct WireLLMInfo {
            display_name: String,
            #[serde(default)]
            base_model_name: Option<String>,
            id: LLMId,
            #[serde(default)]
            reasoning_level: Option<String>,
            usage_metadata: LLMUsageMetadata,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            disable_reason: Option<DisableReason>,
            #[serde(default)]
            vision_supported: bool,
            #[serde(default)]
            spec: Option<LLMSpec>,
            provider: LLMProvider,
            #[serde(default)]
            host_configs: HostConfigsWire,
            #[serde(default)]
            discount_percentage: Option<f32>,
            #[serde(default)]
            context_window: LLMContextWindow,
        }

        let wire = WireLLMInfo::deserialize(deserializer)?;
        let host_configs = match wire.host_configs {
            HostConfigsWire::Map(map) => map,
            HostConfigsWire::Vec(vec) => {
                let mut map = HashMap::new();
                for config in vec {
                    let host = config.model_routing_host.clone();
                    if map.insert(host.clone(), config).is_some() {
                        log::warn!(
                            "Duplicate LLMModelHost entry for {:?}, using latest value",
                            host
                        );
                    }
                }
                map
            }
        };
        Ok(Self {
            base_model_name: wire
                .base_model_name
                .unwrap_or_else(|| wire.display_name.clone()),
            vision_supported: wire.vision_supported,
            provider: wire.provider,
            display_name: wire.display_name,
            id: wire.id,
            reasoning_level: wire.reasoning_level,
            usage_metadata: wire.usage_metadata,
            description: wire.description,
            disable_reason: wire.disable_reason,
            spec: wire.spec,
            host_configs,
            discount_percentage: wire.discount_percentage,
            context_window: wire.context_window,
        })
    }
}

impl LLMInfo {
    /// Returns the display name for the LLM, to be used in the LLM selector menu.
    pub fn menu_display_name(&self) -> String {
        // Custom model routers carry a routing/source description that belongs in
        // the sidecar detail panel, not inline in the chip label. Appending it
        // here would produce a redundant "(Routes by … · …)" suffix.
        if custom_model_routers::is_custom_router_id(self.id.as_str()) {
            return self.display_name.clone();
        }
        // Base label includes optional description in parentheses
        match &self.description {
            // This is a temporary implementation that won't scale well for longer
            // descriptions. We should implement a better approach for displaying
            // model descriptions, maybe through subtext.
            Some(desc) => format!("{} ({})", self.display_name, desc),
            None => self.display_name.clone(),
        }
    }

    /// Returns the given model's base name.
    /// For non-reasoning models, this is the same as the display name.
    /// E.g. gpt-5.1 (low reasoning) -> gpt-5.1
    pub fn base_model_name(&self) -> &str {
        &self.base_model_name
    }

    /// Returns true if this model has a reasoning level configured.
    pub fn has_reasoning_level(&self) -> bool {
        self.reasoning_level.is_some()
    }

    /// Returns the reasoning level label formatted for display.
    pub fn reasoning_level(&self) -> Option<String> {
        self.reasoning_level.clone()
    }
}

/// The set of LLMs available for a feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvailableLLMs {
    /// The Warp "default" LLM.
    default_id: LLMId,
    choices: Vec<LLMInfo>,

    #[serde(default)]
    preferred_codex_model_id: Option<LLMId>,
}

impl AvailableLLMs {
    /// Constructs an `AvailableLLMs` instance from the given default ID and choices.
    ///
    /// If choices is empty, returns an error.
    ///
    /// If default_id is not a valid ID present in `choices`, takes the first choice in `choices
    /// and uses it as the default.
    #[cfg(test)]
    pub fn new<T: Into<LLMInfo>>(
        mut default_id: LLMId,
        choices: impl IntoIterator<Item = T>,
        preferred_codex_model_id: Option<LLMId>,
    ) -> Result<Self, anyhow::Error> {
        let choices: Vec<LLMInfo> = choices.into_iter().map(Into::into).collect();
        if choices.is_empty() {
            return Err(anyhow::anyhow!(
                "Tried to create AvailableLLMs with empty`choices`.",
            ));
        } else if !choices.iter().any(|info| info.id == default_id) {
            let fallback_default = choices
                .first()
                .ok_or_else(|| anyhow::anyhow!("Choices should not be empty"))?;
            log::error!(
                "Default LLM ID {} not present in choices, falling back to first choice {}",
                default_id,
                fallback_default.display_name
            );
            default_id = fallback_default.id.clone();
        }

        Ok(Self {
            default_id,
            choices: choices.into_iter().collect(),
            preferred_codex_model_id,
        })
    }

    fn info_for_id(&self, id: &LLMId) -> Option<&LLMInfo> {
        self.choices.iter().find(|info| info.id == *id)
    }

    /// Returns the info for the given id only if the model is usable (present
    /// and not effectively disabled for the current user).
    fn usable_info_for_id(&self, id: &LLMId) -> Option<&LLMInfo> {
        self.info_for_id(id).filter(|info| {
            info.disable_reason
                .as_ref()
                .is_none_or(|reason| !reason.should_clear_preference())
        })
    }

    fn default_llm_info(&self) -> &LLMInfo {
        self.info_for_id(&self.default_id)
            .expect("Default LLM ID must be present in choices")
    }
}

/// The set of models available to the client, grouped by the feature they support.
/// This is fetched from the server and cached.
///
/// Currently, if a model is available for multiple features,
/// it will appear denormalized in each of the feature's
/// [`AvailableLLMs`]. While this denormalization doesn't add much value today,
/// it eventually lets us add feature-specific properties to an [`LLMInfo`].
///
/// NOTE: This used to include a `planning` field; this was removed after planning via subagent was
/// deprecated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelsByFeature {
    pub agent_mode: AvailableLLMs,
    pub coding: AvailableLLMs,
    /// The set of LLMs available for CLI agent.
    /// This field is optional during deserialization, as older clients might not have this field.
    #[serde(default)]
    pub cli_agent: Option<AvailableLLMs>,
    /// The set of LLMs available for computer use agent.
    /// This field is optional during deserialization, as older clients might not have this field.
    #[serde(default)]
    pub computer_use: Option<AvailableLLMs>,
}

impl ModelsByFeature {
    /// Returns the info about the LLM identified by `id`, if we have it.
    ///
    /// For models that are available across multiple features,
    /// any one of the metadata will be returned.
    fn info_for_id(&self, id: &LLMId) -> Option<&LLMInfo> {
        self.agent_mode.info_for_id(id)
    }
}

/// Returns the default AvailableLLMs for computer use.
/// Used both in `ModelsByFeature::default()` and as a fallback in `get_computer_use_available()`.
fn default_computer_use_llms() -> AvailableLLMs {
    AvailableLLMs {
        default_id: "computer-use-agent-auto".to_owned().into(),
        choices: vec![LLMInfo {
            display_name: "auto".to_owned(),
            base_model_name: "auto".to_owned(),
            id: "computer-use-agent-auto".to_owned().into(),
            reasoning_level: None,
            usage_metadata: LLMUsageMetadata {
                request_multiplier: 1,
                credit_multiplier: None,
            },
            description: None,
            disable_reason: None,
            vision_supported: true,
            spec: None,
            provider: LLMProvider::Unknown,
            host_configs: HashMap::new(),
            discount_percentage: None,
            context_window: LLMContextWindow::default(),
        }],
        preferred_codex_model_id: None,
    }
}

impl Default for ModelsByFeature {
    fn default() -> Self {
        Self {
            agent_mode: AvailableLLMs {
                default_id: "auto".to_owned().into(),
                choices: vec![LLMInfo {
                    display_name: "auto (cost-efficient)".to_owned(),
                    base_model_name: "auto (cost-efficient)".to_owned(),
                    id: "auto".to_owned().into(),
                    reasoning_level: None,
                    usage_metadata: LLMUsageMetadata {
                        request_multiplier: 1,
                        credit_multiplier: None,
                    },
                    description: None,
                    disable_reason: None,
                    vision_supported: true,
                    spec: None,
                    provider: LLMProvider::Unknown,
                    host_configs: HashMap::new(),
                    discount_percentage: None,
                    context_window: LLMContextWindow::default(),
                }],
                preferred_codex_model_id: None,
            },
            coding: AvailableLLMs {
                default_id: "auto".to_owned().into(),
                choices: vec![LLMInfo {
                    display_name: "auto (responsive)".to_owned(),
                    base_model_name: "auto (responsive)".to_owned(),
                    id: "auto".to_owned().into(),
                    reasoning_level: None,
                    usage_metadata: LLMUsageMetadata {
                        request_multiplier: 1,
                        credit_multiplier: None,
                    },
                    description: None,
                    disable_reason: None,
                    vision_supported: true,
                    spec: None,
                    provider: LLMProvider::Unknown,
                    host_configs: HashMap::new(),
                    discount_percentage: None,
                    context_window: LLMContextWindow::default(),
                }],
                preferred_codex_model_id: None,
            },
            cli_agent: Some(AvailableLLMs {
                default_id: "cli-agent-auto".to_owned().into(),
                choices: vec![LLMInfo {
                    display_name: "auto".to_owned(),
                    base_model_name: "auto".to_owned(),
                    id: "cli-agent-auto".to_owned().into(),
                    reasoning_level: None,
                    usage_metadata: LLMUsageMetadata {
                        request_multiplier: 1,
                        credit_multiplier: None,
                    },
                    description: None,
                    disable_reason: None,
                    vision_supported: false,
                    spec: None,
                    provider: LLMProvider::Unknown,
                    host_configs: HashMap::new(),
                    discount_percentage: None,
                    context_window: LLMContextWindow::default(),
                }],
                preferred_codex_model_id: None,
            }),
            computer_use: Some(default_computer_use_llms()),
        }
    }
}

/// Singleton model holding user/workspace LLM preferences, including the set of LLMs available for
/// use as well as the user's preferred LLM for Agent Mode.
pub struct LLMPreferences {
    models_by_feature: ModelsByFeature,
    // Stores temporary model overrides for a given terminal view.
    // NOTE: We only store an override if the model selected by the user is different
    // from the base LLM for the active profile. This means that if the user selects the
    // profile's default model and changes their profile, the model will update to that profile's default.
    base_llm_for_terminal_view: HashMap<EntityId, LLMId>,
    /// Synthetic `LLMInfo` entries built from the user's `ApiKeyManager.custom_endpoints` so
    /// custom models surface in the model picker and resolve through `info_for_id` lookups.
    /// Each entry's `id` is the model's `config_key` (UUID), which is also what flows out to
    /// `Request.Settings.custom_model_providers.providers[*].models[*].config_key`.
    ///
    /// Rebuilt from scratch on every `ApiKeyManagerEvent::KeysUpdated`, so adds, edits, and
    /// removals all immediately propagate to the picker.
    custom_llms: Vec<LLMInfo>,
    /// All custom model routers, including both local and cloud-backed.
    custom_model_routers: Vec<CustomModelRouter>,
}

impl LLMPreferences {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let models_by_feature = get_cached_models(ctx).unwrap_or_default();

        // Re-reconcile disabled model preferences when BYOK keys change, since
        // RequiresUpgrade models may become usable or unusable.
        // Also rebuild `custom_llms` so adds/edits/removals to the user's custom endpoints
        // immediately flow through to the model picker.
        ctx.subscribe_to_model(
            &ApiKeyManager::handle(ctx),
            |me, _, _event: &ApiKeyManagerEvent, ctx| {
                me.rebuild_custom_llms(ctx);
                me.reconcile_disabled_model_preferences(ctx);
                ctx.emit(LLMPreferencesEvent::UpdatedAvailableLLMs);
            },
        );

        // Rebuild custom model routers whenever the local `model_configs/` directory
        // changes, and reconcile any now-stale local selection.
        if FeatureFlag::CustomModelRouters.is_enabled() {
            ctx.subscribe_to_model(&WarpConfig::handle(ctx), |me, _, event, ctx| {
                if matches!(event, WarpConfigUpdateEvent::ModelConfigs) {
                    me.rebuild_custom_model_routers(ctx);
                    me.reconcile_stale_custom_router_selection(ctx);
                }
            });
        }

        let base_llm_for_terminal_view = HashMap::new();
        let custom_llms = build_custom_llm_infos(ApiKeyManager::as_ref(ctx).keys());

        let mut me = Self {
            models_by_feature,
            base_llm_for_terminal_view,
            custom_llms,
            custom_model_routers: Vec::new(),
        };

        // Seed from any already-loaded local config (the async load emits
        // `ModelConfigs` shortly after startup to populate fully).
        if FeatureFlag::CustomModelRouters.is_enabled() {
            me.rebuild_custom_model_routers(ctx);
        }

        me
    }

    /// Returns the `LLMInfo` for the base LLM to be used for an Agent Mode request.
    pub fn get_active_base_model<'a>(
        &'a self,
        app: &'a AppContext,
        terminal_view_id: Option<EntityId>,
    ) -> &'a LLMInfo {
        self.get_preferred_base_model(app, terminal_view_id)
    }

    /// Returns `LLMInfo` for the currently selected LLM to be used for Agent Mode.
    fn get_preferred_base_model(
        &self,
        app: &AppContext,
        terminal_view_id: Option<EntityId>,
    ) -> &LLMInfo {
        if let Some(terminal_view_id) = terminal_view_id {
            let raw_override = self.base_llm_for_terminal_view.get(&terminal_view_id);
            if let Some(llm_id) = raw_override {
                if let Some(llm_info) = Self::server_info_for_id_router_gated(
                    &self.models_by_feature.agent_mode,
                    llm_id,
                )
                .or_else(|| self.custom_router_llm_info_for_id_if_enabled(llm_id))
                {
                    return llm_info;
                }
            }
        }

        let profile = AIExecutionProfilesModel::as_ref(app).active_profile(terminal_view_id, app);

        profile
            .data()
            .base_model
            .clone()
            .and_then(|id| {
                Self::server_info_for_id_router_gated(&self.models_by_feature.agent_mode, &id)
                    .or_else(|| self.custom_router_llm_info_for_id_if_enabled(&id))
            })
            .unwrap_or_else(|| self.models_by_feature.agent_mode.default_llm_info())
    }

    pub fn get_active_coding_model<'a>(
        &'a self,
        app: &'a AppContext,
        terminal_view_id: Option<EntityId>,
    ) -> &'a LLMInfo {
        self.get_preferred_coding_model(app, terminal_view_id)
    }

    /// Returns `LLMInfo` for user's preferred coding model.
    fn get_preferred_coding_model(
        &self,
        app: &AppContext,
        terminal_view_id: Option<EntityId>,
    ) -> &LLMInfo {
        let profile = AIExecutionProfilesModel::as_ref(app).active_profile(terminal_view_id, app);

        profile
            .data()
            .coding_model
            .clone()
            .and_then(|id| {
                Self::server_info_for_id_router_gated(&self.models_by_feature.coding, &id)
                    .or_else(|| self.custom_router_llm_info_for_id_if_enabled(&id))
            })
            .unwrap_or_else(|| self.models_by_feature.coding.default_llm_info())
    }

    /// Resolves `id` against a server-provided model list, but hides cloud/team
    /// custom routers when the custom-router feature flag is off. Mirrors the
    /// gating applied to local routers (see
    /// [`Self::custom_router_llm_info_for_id_if_enabled`]) so the whole
    /// custom-router feature is controlled by a single client flag.
    fn server_info_for_id_router_gated<'a>(
        available: &'a AvailableLLMs,
        id: &LLMId,
    ) -> Option<&'a LLMInfo> {
        let info = available.info_for_id(id)?;
        if !FeatureFlag::CustomModelRouters.is_enabled()
            && custom_model_routers::is_cloud_custom_router_id(info.id.as_str())
        {
            return None;
        }
        Some(info)
    }

    /// Returns the set of LLMs available for Agent Mode use.
    pub fn get_base_llm_choices_for_agent_mode(&self) -> impl Iterator<Item = &LLMInfo> {
        // Don't show admin-disabled models in the dropdown
        let routers_enabled = FeatureFlag::CustomModelRouters.is_enabled();
        self.models_by_feature
            .agent_mode
            .choices
            .iter()
            .filter(|llm| !matches!(llm.disable_reason, Some(DisableReason::AdminDisabled)))
            // Gate cloud/team routers behind the same flag as local routers so
            // the entire custom-router feature is controlled by one flag.
            .filter(move |llm| {
                routers_enabled || !custom_model_routers::is_cloud_custom_router_id(llm.id.as_str())
            })
            .chain(self.custom_router_choices())
    }

    /// Returns the set of LLMs available for coding.
    pub fn get_coding_llm_choices(&self) -> impl Iterator<Item = &LLMInfo> {
        // Don't show admin-disabled models in the dropdown
        let routers_enabled = FeatureFlag::CustomModelRouters.is_enabled();
        self.models_by_feature
            .coding
            .choices
            .iter()
            .filter(|llm| !matches!(llm.disable_reason, Some(DisableReason::AdminDisabled)))
            // Gate cloud/team routers behind the same flag as local routers.
            .filter(move |llm| {
                routers_enabled || !custom_model_routers::is_cloud_custom_router_id(llm.id.as_str())
            })
            .chain(self.custom_router_choices())
    }

    /// Returns the set of LLMs available for CLI agent.
    pub fn get_cli_agent_llm_choices(&self) -> impl Iterator<Item = &LLMInfo> {
        self.get_cli_agent_available().choices.iter()
    }

    /// Returns the `LLMInfo` for the CLI agent model.
    pub fn get_active_cli_agent_model<'a>(
        &'a self,
        app: &'a AppContext,
        terminal_view_id: Option<EntityId>,
    ) -> &'a LLMInfo {
        let profile = AIExecutionProfilesModel::as_ref(app).active_profile(terminal_view_id, app);

        let available = self.get_cli_agent_available();
        profile
            .data()
            .cli_agent_model
            .clone()
            .and_then(|id| available.info_for_id(&id))
            .unwrap_or_else(|| available.default_llm_info())
    }

    /// Returns the default CLI agent model as a fallback.
    pub fn get_default_cli_agent_model(&self) -> &LLMInfo {
        self.get_cli_agent_available().default_llm_info()
    }

    /// Helper to get the AvailableLLMs for cli_agent, falling back to agent_mode.
    fn get_cli_agent_available(&self) -> &AvailableLLMs {
        self.models_by_feature
            .cli_agent
            .as_ref()
            .unwrap_or(&self.models_by_feature.agent_mode)
    }

    /// Returns the `LLMInfo` for the computer use agent model.
    pub fn get_active_computer_use_model<'a>(
        &'a self,
        app: &'a AppContext,
        terminal_view_id: Option<EntityId>,
    ) -> &'a LLMInfo {
        let profile = AIExecutionProfilesModel::as_ref(app).active_profile(terminal_view_id, app);

        let available = self.get_computer_use_available();
        profile
            .data()
            .computer_use_model
            .clone()
            .and_then(|id| available.info_for_id(&id))
            .unwrap_or_else(|| available.default_llm_info())
    }

    /// Helper to get the AvailableLLMs for computer_use.
    /// Falls back to a computer-use-specific default if None.
    fn get_computer_use_available(&self) -> &AvailableLLMs {
        static DEFAULT: OnceLock<AvailableLLMs> = OnceLock::new();
        self.models_by_feature
            .computer_use
            .as_ref()
            .unwrap_or_else(|| DEFAULT.get_or_init(default_computer_use_llms))
    }

    /// Returns metadata about an LLM, if the client knows about it.
    /// Falls back to the user's custom-endpoint LLMs when the id isn't a server-known model
    /// id (e.g. when it's a `config_key` UUID).
    pub fn get_llm_info(&self, id: &LLMId) -> Option<&LLMInfo> {
        self.models_by_feature
            .info_for_id(id)
            .or_else(|| self.custom_llm_info_for_id(id))
            .or_else(|| self.custom_router_llm_info_for_id(id))
    }

    /// Resolves an `LLMId` against the user's custom-endpoint LLMs.
    /// Returns `None` if the id isn't a known custom model `config_key`.
    pub fn custom_llm_info_for_id(&self, id: &LLMId) -> Option<&LLMInfo> {
        self.custom_llms.iter().find(|info| info.id == *id)
    }

    /// Footer label for custom endpoint usage keyed by the request config_key.
    /// The synthetic custom LLMInfo already owns alias-or-name display semantics.
    pub fn custom_endpoint_usage_display_label(&self, config_key: &str) -> String {
        let config_key = LLMId::from(config_key);
        self.custom_llm_info_for_id(&config_key)
            .map(|info| info.display_name.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| CUSTOM_ENDPOINT_USAGE_FALLBACK_LABEL.to_string())
    }

    /// Resolves a custom model router by its `config_key`/`LLMId`.
    pub fn custom_model_router_for_id(&self, id: &LLMId) -> Option<&CustomModelRouter> {
        self.custom_model_routers.iter().find(|m| m.llm_id() == *id)
    }

    fn custom_router_llm_info_for_id(&self, id: &LLMId) -> Option<&LLMInfo> {
        self.custom_model_routers
            .iter()
            .find(|m| m.info.id == *id)
            .map(|m| &m.info)
    }

    fn custom_router_llm_info_for_id_if_enabled(&self, id: &LLMId) -> Option<&LLMInfo> {
        FeatureFlag::CustomModelRouters
            .is_enabled()
            .then(|| self.custom_router_llm_info_for_id(id))
            .flatten()
    }

    /// Iterator over the custom router picker entries, gated on the feature flag.
    pub fn custom_router_choices(&self) -> impl Iterator<Item = &LLMInfo> {
        let enabled = FeatureFlag::CustomModelRouters.is_enabled();
        self.custom_model_routers
            .iter()
            .filter(move |_| enabled)
            .map(|m| &m.info)
    }

    /// Builds the custom_model_routers registry for an outbound request.
    pub fn custom_model_routers_for_request(
        &self,
        base_id: &LLMId,
        coding_id: &LLMId,
    ) -> api::request::settings::CustomModelRouters {
        let mut models = Vec::new();
        let mut seen = HashSet::new();
        for id in [base_id, coding_id] {
            if let Some(entry) = self.custom_router_proto_entry(id) {
                if seen.insert(entry.config_key.clone()) {
                    models.push(entry);
                }
            }
        }
        api::request::settings::CustomModelRouters { routers: models }
    }

    /// Returns the proto registry entry for a local custom-router id, or `None`
    /// if `id` is not a known local router.
    fn custom_router_proto_entry(
        &self,
        id: &LLMId,
    ) -> Option<api::request::settings::custom_model_routers::CustomModelRouter> {
        self.custom_model_router_for_id(id).map(|m| m.to_proto())
    }

    /// Rebuilds `custom_model_routers` from the `model_configs/` directory,
    /// then notifies subscribers.
    ///
    /// Routers whose targets include an unknown model are excluded and a
    /// warning is logged. The check uses the currently loaded model list
    /// (server-fetched + cached), so it is best-effort at startup before
    /// the server responds.
    fn rebuild_custom_model_routers(&mut self, ctx: &mut ModelContext<Self>) {
        let local = WarpConfig::as_ref(ctx).custom_model_routers().clone();

        let mut deduped = Vec::with_capacity(local.len());
        let mut seen = HashSet::new();
        for model in local {
            if seen.insert(model.config_key()) {
                deduped.push(model);
            }
        }
        let mut validation_errors: Vec<ModelConfigError> = Vec::new();
        deduped.retain(|router| {
            let unknown: Vec<&str> = router
                .all_targets()
                .into_iter()
                .filter(|id| self.get_llm_info(&LLMId::from(*id)).is_none())
                .collect();
            if unknown.is_empty() {
                return true;
            }
            let error_message = format!("unknown target model(s): {}", unknown.join(", "));
            log::warn!(
                "Custom model router '{}': {} — excluding from picker",
                router.info.display_name,
                error_message,
            );
            validation_errors.push(ModelConfigError {
                file_name: router
                    .source_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or(router.info.display_name.as_str())
                    .to_owned(),
                file_path: router.source_path.clone().unwrap_or_default(),
                error_message,
            });
            false
        });
        if !validation_errors.is_empty() {
            WarpConfig::handle(ctx).update(ctx, |_, ctx| {
                ctx.emit(WarpConfigUpdateEvent::ModelConfigErrors(validation_errors));
            });
        }

        // vision is supported only when every concrete target model supports it.
        for router in &mut deduped {
            router.info.vision_supported = router.all_targets().iter().all(|id| {
                self.get_llm_info(&LLMId::from(*id))
                    .is_some_and(|info| info.vision_supported)
            });
        }

        self.custom_model_routers = deduped;
        ctx.emit(LLMPreferencesEvent::UpdatedAvailableLLMs);
    }

    /// Resets any persisted *local* custom-router selection that no longer resolves
    /// to a loaded definition, so a deleted/invalid local config falls back to the
    /// default model and the visible selection updates. Scoped to local
    /// ids so a cloud selection isn't reset by a local reload.
    fn reconcile_stale_custom_router_selection(&mut self, ctx: &mut ModelContext<Self>) {
        let valid_local: HashSet<LLMId> = self
            .custom_model_routers
            .iter()
            .map(|m| m.llm_id())
            .collect();

        let mut updated_agent_mode = false;
        let mut updated_coding = false;

        self.base_llm_for_terminal_view.retain(|_, id| {
            let stale = custom_model_routers::is_local_custom_router_id(id.as_str())
                && !valid_local.contains(&*id);
            updated_agent_mode |= stale;
            !stale
        });

        AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles, ctx| {
            for profile_id in profiles.get_all_profile_ids() {
                let Some(profile) = profiles.get_profile_by_id(profile_id, ctx) else {
                    continue;
                };
                let profile_data = profile.data();
                let base_stale = profile_data.base_model.as_ref().is_some_and(|id| {
                    custom_model_routers::is_local_custom_router_id(id.as_str())
                        && !valid_local.contains(id)
                });
                if base_stale {
                    profiles.set_base_model(profile_id, None, ctx);
                    profiles.set_context_window_limit(profile_id, None, ctx);
                    updated_agent_mode = true;
                }
                let coding_stale = profile_data.coding_model.as_ref().is_some_and(|id| {
                    custom_model_routers::is_local_custom_router_id(id.as_str())
                        && !valid_local.contains(id)
                });
                if coding_stale {
                    profiles.set_coding_model(profile_id, None, ctx);
                    updated_coding = true;
                }
            }
        });

        if updated_agent_mode {
            self.trigger_snapshot_save(ctx);
            ctx.emit(LLMPreferencesEvent::UpdatedActiveAgentModeLLM);
        }
        if updated_coding {
            ctx.emit(LLMPreferencesEvent::UpdatedActiveCodingLLM);
        }
    }

    /// Reads the user's current `ApiKeyManager.custom_endpoints` and replaces `custom_llms`
    /// with synthetic `LLMInfo`s. Called on every `ApiKeyManagerEvent::KeysUpdated`, so adds,
    /// edits, and removals all propagate immediately.
    fn rebuild_custom_llms(&mut self, app: &AppContext) {
        self.custom_llms = build_custom_llm_infos(ApiKeyManager::as_ref(app).keys());
    }

    /// Returns the default base model as a fallback.
    pub fn get_default_base_model(&self) -> &LLMInfo {
        self.models_by_feature.agent_mode.default_llm_info()
    }

    /// Returns the default coding model as a fallback.
    pub fn get_default_coding_model(&self) -> &LLMInfo {
        self.models_by_feature.coding.default_llm_info()
    }

    /// Returns the preferred Codex model, if set by the server.
    pub fn get_preferred_codex_model(&self) -> Option<&LLMInfo> {
        self.models_by_feature
            .agent_mode
            .preferred_codex_model_id
            .as_ref()
            .and_then(|id| self.models_by_feature.agent_mode.info_for_id(id))
    }

    #[cfg(feature = "integration_tests")]
    pub fn is_available_agent_mode_llm(&self, id: &LLMId) -> bool {
        self.models_by_feature.agent_mode.info_for_id(id).is_some()
    }

    /// Creates a pane-level override for the Agent Mode LLM.
    pub fn update_preferred_agent_mode_llm(
        &mut self,
        preferred_llm_id: &LLMId,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        let profile =
            AIExecutionProfilesModel::as_ref(ctx).active_profile(Some(terminal_view_id), ctx);

        let profile_default_model_id = profile
            .data()
            .base_model
            .as_ref()
            .and_then(|id| self.models_by_feature.agent_mode.info_for_id(id))
            .unwrap_or_else(|| self.models_by_feature.agent_mode.default_llm_info())
            .id
            .clone();

        // Only remove override if we're setting to the profile's default.
        // Otherwise, always set the override explicitly.
        let changed = if preferred_llm_id == &profile_default_model_id {
            self.base_llm_for_terminal_view
                .remove(&terminal_view_id)
                .is_some()
        } else {
            self.base_llm_for_terminal_view
                .insert(terminal_view_id, preferred_llm_id.clone());
            true
        };

        if changed {
            self.trigger_snapshot_save(ctx);
            ctx.emit(LLMPreferencesEvent::UpdatedActiveAgentModeLLM);
        }
    }

    /// Triggers a snapshot save to persist LLM override changes.
    fn trigger_snapshot_save(&self, ctx: &mut ModelContext<Self>) {
        ctx.dispatch_global_action("workspace:save_app", ());
    }

    pub fn update_preferred_coding_llm(
        &self,
        preferred_llm_id: &LLMId,
        terminal_view_id: Option<EntityId>,
        ctx: &mut ModelContext<Self>,
    ) {
        let new_value = if preferred_llm_id == &self.models_by_feature.coding.default_id {
            None
        } else {
            Some(preferred_llm_id.clone())
        };

        let mut changed = false;
        AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles, ctx| {
            let profile = profiles.active_profile(terminal_view_id, ctx);

            if profile.data().coding_model != new_value {
                profiles.set_coding_model(*profile.id(), new_value, ctx);
                changed = true;
            }
        });

        if changed {
            ctx.emit(LLMPreferencesEvent::UpdatedActiveCodingLLM);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_models_by_feature_for_test(&mut self, models_by_feature: ModelsByFeature) {
        self.models_by_feature = models_by_feature;
    }

    /// Clear any model selections where the model is no longer supported
    /// or effectively disabled, and clear orphaned context window limits
    /// for non-configurable or unusable models.
    ///
    /// Called both when the model list is refreshed from the server and when
    /// BYOK API keys change (since `RequiresUpgrade` usability is BYOK-aware).
    fn reconcile_disabled_model_preferences(&self, ctx: &mut ModelContext<Self>) {
        let profiles_model = AIExecutionProfilesModel::handle(ctx);
        profiles_model.update(ctx, |profiles, ctx| {
            for profile_id in profiles.get_all_profile_ids() {
                if let Some(profile) = profiles.get_profile_by_id(profile_id, ctx) {
                    let profile_data = profile.data();
                    let preferred_base_model = profile_data.base_model.clone();
                    let effective_base_model_id = preferred_base_model
                        .as_ref()
                        .unwrap_or(&self.models_by_feature.agent_mode.default_id);
                    let effective_base_model_usable = self
                        .models_by_feature
                        .agent_mode
                        .usable_info_for_id(effective_base_model_id);
                    let effective_base_model_unusable = effective_base_model_usable.is_none();
                    let effective_base_model_is_configurable = effective_base_model_usable
                        .is_some_and(|info| info.context_window.is_configurable);
                    let has_context_window_limit = profile_data.context_window_limit.is_some();

                    if preferred_base_model.is_some() && effective_base_model_unusable {
                        profiles.set_base_model(profile_id, None, ctx);
                    }
                    if has_context_window_limit
                        && (effective_base_model_unusable || !effective_base_model_is_configurable)
                    {
                        profiles.set_context_window_limit(profile_id, None, ctx);
                    }
                    if let Some(preferred_llm_id) = &profile.data().coding_model {
                        if self
                            .models_by_feature
                            .coding
                            .usable_info_for_id(preferred_llm_id)
                            .is_none()
                        {
                            profiles.set_coding_model(profile_id, None, ctx);
                        }
                    }
                    if let Some(preferred_llm_id) = &profile.data().cli_agent_model {
                        if self
                            .get_cli_agent_available()
                            .usable_info_for_id(preferred_llm_id)
                            .is_none()
                        {
                            profiles.set_cli_agent_model(profile_id, None, ctx);
                        }
                    }
                    if let Some(preferred_llm_id) = &profile.data().computer_use_model {
                        if self
                            .get_computer_use_available()
                            .usable_info_for_id(preferred_llm_id)
                            .is_none()
                        {
                            profiles.set_computer_use_model(profile_id, None, ctx);
                        }
                    }
                }
            }
        });
    }

    pub fn vision_supported(&self, app: &AppContext, terminal_view_id: Option<EntityId>) -> bool {
        self.get_active_base_model(app, terminal_view_id)
            .vision_supported
    }

    pub fn get_base_llm_override(&self, terminal_view_id: EntityId) -> Option<String> {
        if let Some(override_str) = self
            .base_llm_for_terminal_view
            .get(&terminal_view_id)
            .and_then(|llm_id| serde_json::to_string(llm_id).ok())
        {
            return Some(override_str);
        }

        log::debug!("LLM override not found in memory for terminal view: {terminal_view_id:?}");
        None
    }

    /// Removes the LLM override for a terminal view.
    /// This ensures that the new profile's default model is used.
    pub fn remove_llm_override(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        let old = self.base_llm_for_terminal_view.remove(&terminal_view_id);
        if old.is_some() {
            self.trigger_snapshot_save(ctx);
            ctx.emit(LLMPreferencesEvent::UpdatedActiveAgentModeLLM);
        }
    }
}

#[derive(Clone, Debug)]
pub enum LLMPreferencesEvent {
    UpdatedAvailableLLMs,
    UpdatedActiveAgentModeLLM,
    UpdatedActiveCodingLLM,
}

impl Entity for LLMPreferences {
    type Event = LLMPreferencesEvent;
}

impl SingletonEntity for LLMPreferences {}

/// Builds synthetic [`LLMInfo`]s from the user's persisted custom endpoints.
///
/// One entry per `CustomEndpointModel`. The display label is the **alias** when present,
/// falling back to the raw model name. The `id` is the model's `config_key`, which is
/// also what flows out to `Request.Settings.custom_model_providers` so the server can map
/// a `ModelConfig.{base,coding,cli_agent,computer_use_agent}` selection back to the
/// user-provided endpoint.
///
/// Endpoints with empty URL or API key, and models with empty name or config_key, are
/// skipped — they shouldn't surface in the picker until the user finishes configuring them.
fn build_custom_llm_infos(keys: &ai::api_keys::ApiKeys) -> Vec<LLMInfo> {
    keys.custom_endpoints
        .iter()
        .filter(|ep| !ep.url.trim().is_empty() && !ep.api_key.is_empty())
        .flat_map(|endpoint| {
            endpoint
                .models
                .iter()
                .filter(|m| !m.name.trim().is_empty() && !m.config_key.is_empty())
                .map(move |model| custom_llm_info_from(endpoint, model))
        })
        .collect()
}

fn custom_llm_info_from(endpoint: &CustomEndpoint, model: &CustomEndpointModel) -> LLMInfo {
    let label = model.display_label().to_owned();
    LLMInfo {
        display_name: label.clone(),
        base_model_name: label,
        id: model.config_key.clone().into(),
        reasoning_level: None,
        usage_metadata: LLMUsageMetadata {
            request_multiplier: 1,
            credit_multiplier: None,
        },
        description: Some(format!("Custom · {}", endpoint.name)),
        disable_reason: None,
        vision_supported: true,
        spec: None,
        provider: LLMProvider::Unknown,
        host_configs: HashMap::new(),
        discount_percentage: None,
        context_window: LLMContextWindow::default(),
    }
}

/// Gets the last cached LLM metadata.
fn get_cached_models(app: &mut AppContext) -> Option<ModelsByFeature> {
    let value = app
        .private_user_preferences()
        .read_value(MODELS_BY_FEATURE_CACHE_KEY)
        .ok()
        .flatten()?;

    // Try to deserialize to the [`ModelsByFeature`] type.
    match serde_json::from_str::<ModelsByFeature>(value.as_str()) {
        Ok(config) => Some(config),
        Err(e1) => {
            // If that fails, try to deserialize directly to [`AvailableLLMs`].
            // Before we had model choice by feature, all available LLMs were solely
            // for Agent Mode.
            match serde_json::from_str::<AvailableLLMs>(value.as_str()) {
                Ok(config) => Some(ModelsByFeature {
                    agent_mode: config,
                    ..Default::default()
                }),
                Err(e2) => {
                    log::warn!("Failed to deserialize cached LLMs: {e1}\n{e2}");
                    None
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "llms_tests.rs"]
mod tests;
