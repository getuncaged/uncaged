//! Shared orchestration controls reused by the `RunAgentsCardView`
//! confirmation card editor and the plan-card
//! `OrchestrationConfigBlockView`.
//!
//! The generic parameter `A` is the parent view's typed action — both
//! consumers impl [`OrchestrationControlAction`] to provide the mapping
//! from field-change events to their own action enum.

use std::collections::HashMap;

use ai::agent::orchestration_config::OrchestrationConfig;
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::{vec2f, Vector2F};
use uncaged_cli::agent::Harness;
use uncaged_core::ui::theme::Fill;
use uncaged_ui::elements::{
    ChildView, Container, CornerRadius, CrossAxisAlignment, Empty, Flex, MainAxisSize,
    ParentElement, Point, Radius, Text,
};
use uncaged_ui::event::DispatchedEvent;
use uncaged_ui::ui_components::button::ButtonVariant;
use uncaged_ui::ui_components::components::{Coords, UiComponentStyles};
use uncaged_ui::{
    AfterLayoutContext, AppContext, Element, EventContext, LayoutContext, PaintContext,
    SingletonEntity, SizeConstraint, View, ViewContext, ViewHandle,
};

use crate::ai::execution_profiles::model_menu_items::available_model_menu_items;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::ai::harness_display;
use crate::ai::llms::LLMInfo;
use crate::ai::local_harness_setup::{
    local_harness_is_product_enabled, local_harness_product_disabled_message,
    local_harness_setup_state, LocalHarnessSetupState,
};
use crate::appearance::Appearance;
use crate::menu::{MenuItem, MenuItemFields};
use crate::ui_components::blended_colors;
use crate::view_components::dropdown::{
    Dropdown, DropdownAction, DropdownItemAction, DropdownStyle,
};
use crate::view_components::FilterableDropdown;
use crate::LLMPreferences;

// ── Shared constants ────────────────────────────────────────────────

pub const ORCHESTRATION_PICKER_HEIGHT: f32 = 36.;
pub const ORCHESTRATION_PICKER_BORDER_WIDTH: f32 = 1.;
pub const ORCHESTRATION_PICKER_FONT_SIZE: f32 = 14.;
pub const ORCHESTRATION_PICKER_RADIUS: f32 = 4.;
pub const ORCHESTRATION_PICKER_MAX_WIDTH: f32 = 205.;

const DEFAULT_MODEL_LABEL: &str = "Default model";

// ── Action trait ────────────────────────────────────────────────────

/// Trait that both `RunAgentsCardViewAction` and
/// `OrchestrationConfigBlockAction` implement so the shared picker
/// creation and render helpers can produce the correct action variant.
pub trait OrchestrationControlAction: DropdownItemAction + Clone {
    fn model_changed(model_id: String) -> Self;
    fn harness_changed(harness_type: String) -> Self;
}

// ── Shared edit state ───────────────────────────────────────────────

/// Run-wide configuration fields shared between the confirmation card
/// editor and the plan-card config block. Card-specific fields
/// (agent_run_configs, base_prompt, summary) remain on the per-view state
/// structs. Child agents always launch locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationEditState {
    pub model_id: String,
    pub harness_type: String,
}

impl OrchestrationEditState {
    pub(crate) fn sanitize_for_local_execution(&mut self) {
        let Some(harness) = Harness::parse_local_child_harness(&self.harness_type) else {
            return;
        };
        if local_harness_product_disabled_message(harness).is_some() {
            self.harness_type = "oz".to_string();
            self.model_id.clear();
        }
    }
    pub fn from_run_agents_fields(model_id: &str, harness_type: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            harness_type: harness_type.to_string(),
        }
    }

    pub fn from_orchestration_config(config: &OrchestrationConfig) -> Self {
        let mut state = Self {
            model_id: config.model_id.clone(),
            harness_type: config.harness_type.clone(),
        };
        state.sanitize_for_local_execution();
        state
    }

    /// Returns `Some(reason)` if Accept / Apply must be disabled because the
    /// local harness is product-disabled.
    pub fn accept_disabled_reason(&self) -> Option<&'static str> {
        Harness::parse_local_child_harness(&self.harness_type)
            .and_then(local_harness_product_disabled_message)
    }

    /// Fills in empty fields from the approved orchestration config.
    /// When the LLM omits harness/model to inherit from the active config,
    /// the raw request arrives with defaults (empty harness, empty model).
    /// This resolves those to the config values so the UI shows the
    /// intended settings.
    pub fn resolve_from_config(&mut self, config: &OrchestrationConfig) {
        if self.harness_type.is_empty() && !config.harness_type.is_empty() {
            self.harness_type = config.harness_type.clone();
        }
        if self.model_id.is_empty() && !config.model_id.is_empty() {
            self.model_id = config.model_id.clone();
        }
        self.sanitize_for_local_execution();
    }

    /// Unconditionally overrides model and harness from the approved
    /// orchestration config. The plan config is the user-approved source of
    /// truth — the LLM's run_agents call may omit or set these differently,
    /// but the config always wins.
    pub fn override_from_approved_config(&mut self, config: &OrchestrationConfig) {
        self.model_id = config.model_id.clone();
        self.harness_type = config.harness_type.clone();
    }

    /// Converts to a native `OrchestrationConfig` for storage / match.
    pub fn to_orchestration_config(&self) -> OrchestrationConfig {
        OrchestrationConfig {
            model_id: self.model_id.clone(),
            harness_type: self.harness_type.clone(),
        }
    }
}

// ── Picker handles ──────────────────────────────────────────────────

/// Picker view handles shared between card editor and plan-card config
/// block. Generic over the action type `A`.
#[derive(Clone)]
pub struct OrchestrationPickerHandles<A: OrchestrationControlAction> {
    pub model_picker: Option<ViewHandle<FilterableDropdown<A>>>,
    pub harness_picker: Option<ViewHandle<Dropdown<A>>>,
}

impl<A: OrchestrationControlAction> Default for OrchestrationPickerHandles<A> {
    fn default() -> Self {
        Self {
            model_picker: None,
            harness_picker: None,
        }
    }
}

// ── Picker styling ──────────────────────────────────────────────────

/// Constructs the shared `UiComponentStyles` for orchestration pickers.
pub fn picker_styles(appearance: &Appearance) -> (UiComponentStyles, PickerColors) {
    let theme = appearance.theme();
    let padding = Coords {
        top: 8.,
        bottom: 8.,
        left: 12.,
        right: 12.,
    };
    let corner_radius = CornerRadius::with_all(Radius::Pixels(ORCHESTRATION_PICKER_RADIUS));
    // The picker bg is a translucent overlay (surface_overlay_1 =
    // fg at 5%). It must stay translucent so that the accent-tinted
    // card background in the config block shows through, and so that
    // gradient-background themes render correctly.
    let background_fill: Fill = theme.surface_overlay_1();
    let background: uncaged_ui::elements::Fill = background_fill.into();
    // Border and font colors are intentionally left to the dropdown's
    // default ButtonVariant::Secondary styling, which uses
    // theme.outline() and theme.main_text_color() — both are
    // contrast-aware and adapt correctly to all themes.

    let styles = UiComponentStyles {
        height: Some(ORCHESTRATION_PICKER_HEIGHT),
        background: Some(background),
        border_width: Some(ORCHESTRATION_PICKER_BORDER_WIDTH),
        border_radius: Some(corner_radius),
        font_size: Some(ORCHESTRATION_PICKER_FONT_SIZE),
        padding: Some(padding),
        ..Default::default()
    };
    let colors = PickerColors {
        padding,
        corner_radius,
        background,
    };
    (styles, colors)
}

#[derive(Clone)]
pub struct PickerColors {
    pub padding: Coords,
    pub corner_radius: CornerRadius,
    pub background: uncaged_ui::elements::Fill,
}

// ── Picker creation (generic over action type) ──────────────────────

/// Creates a standard dropdown with the shared orchestration picker
/// chrome (border, radius, background, font).
pub fn new_standard_picker_dropdown<A: OrchestrationControlAction, V: View>(
    colors: &PickerColors,
    ctx: &mut ViewContext<V>,
) -> ViewHandle<Dropdown<A>> {
    let padding = colors.padding;
    let corner_radius = colors.corner_radius;
    let background = colors.background;
    ctx.add_typed_action_view(move |ctx_dropdown| {
        let mut dropdown = Dropdown::<A>::new(ctx_dropdown);
        dropdown.set_use_overlay_layer(false, ctx_dropdown);
        dropdown.set_match_menu_width_to_top_bar(true, ctx_dropdown);
        dropdown.set_main_axis_size(MainAxisSize::Max, ctx_dropdown);
        dropdown.set_style(DropdownStyle::ActionButtonSecondary, ctx_dropdown);
        dropdown.set_top_bar_height(ORCHESTRATION_PICKER_HEIGHT, ctx_dropdown);
        dropdown.set_top_bar_max_width(f32::INFINITY);
        dropdown.set_padding(padding, ctx_dropdown);
        dropdown.set_border_radius(corner_radius, ctx_dropdown);
        dropdown.set_background(background, ctx_dropdown);
        dropdown.set_border_width(ORCHESTRATION_PICKER_BORDER_WIDTH, ctx_dropdown);
        dropdown.set_font_size(ORCHESTRATION_PICKER_FONT_SIZE, ctx_dropdown);
        dropdown
    })
}

/// Creates a searchable dropdown with the shared orchestration picker
/// chrome (border, radius, background, font).
pub fn new_standard_filterable_picker_dropdown<A: OrchestrationControlAction, V: View>(
    styles: &UiComponentStyles,
    ctx: &mut ViewContext<V>,
) -> ViewHandle<FilterableDropdown<A>> {
    let styles = *styles;
    ctx.add_typed_action_view(move |ctx_dropdown| {
        let mut dropdown = FilterableDropdown::<A>::new(ctx_dropdown);
        dropdown.set_use_overlay_layer(false, ctx_dropdown);
        dropdown.set_match_menu_width_to_top_bar(true, ctx_dropdown);
        dropdown.set_main_axis_size(MainAxisSize::Max, ctx_dropdown);
        dropdown.set_button_variant(ButtonVariant::Secondary);
        dropdown.set_style(styles);
        dropdown.set_top_bar_height(ORCHESTRATION_PICKER_HEIGHT, ctx_dropdown);
        dropdown.set_top_bar_max_width(f32::INFINITY);
        dropdown
    })
}

/// Populates the model picker based on the active harness.
///
/// - **Oz / empty**: shows the Warp LLM catalog (existing behavior).
/// - **Local Codex**: shows only a "Default model" entry (no model delivery
///   possible for local Codex children).
/// - **Other non-Oz harnesses**: shows "Default model" at the top, followed
///   by the server-provided harness model catalog from
///   `HarnessAvailabilityModel::models_for()`.
pub fn populate_model_picker_for_harness<A: OrchestrationControlAction, V: View>(
    dropdown: &ViewHandle<FilterableDropdown<A>>,
    initial_model_id: &str,
    harness_type: &str,
    ctx: &mut ViewContext<V>,
) {
    let initial_model_id = initial_model_id.to_string();
    let harness_type = harness_type.to_string();
    dropdown.update(ctx, |dropdown, ctx_dropdown| {
        let harness = Harness::parse_orchestration_harness(&harness_type);
        match harness {
            Some(Harness::Oz) | None => {
                // Oz / unset: the agent-mode LLM catalog.
                // Order: auto models first, then custom models, then other models.
                let llm_prefs = LLMPreferences::as_ref(ctx_dropdown);
                let (auto_models, rest): (Vec<&LLMInfo>, Vec<_>) = llm_prefs
                    .get_base_llm_choices_for_agent_mode()
                    .partition(|llm| llm.id.as_str().starts_with("auto"));
                let (custom_models, other_models): (Vec<_>, Vec<_>) = rest
                    .into_iter()
                    .partition(|llm| llm_prefs.custom_llm_info_for_id(&llm.id).is_some());
                let ordered_choices: Vec<_> = auto_models
                    .into_iter()
                    .chain(custom_models)
                    .chain(other_models)
                    .collect();
                let selected_display_name = ordered_choices
                    .iter()
                    .find(|llm| llm.id.to_string() == initial_model_id)
                    .map(|llm| llm.menu_display_name());
                let items = available_model_menu_items(
                    ordered_choices,
                    move |llm| {
                        DropdownAction::select_action_and_close(A::model_changed(
                            llm.id.to_string(),
                        ))
                    },
                    None,
                    None,
                    false,
                    false,
                    ctx_dropdown,
                );
                dropdown.set_rich_items(items, ctx_dropdown);
                if let Some(name) = &selected_display_name {
                    dropdown.set_selected_by_name(name, ctx_dropdown);
                }
            }
            Some(Harness::Codex) => {
                // Local Codex: only "Default model" entry.
                let items = vec![default_model_menu_item::<A>()];
                dropdown.set_rich_items(items, ctx_dropdown);
                dropdown.set_selected_by_name(DEFAULT_MODEL_LABEL, ctx_dropdown);
            }
            Some(harness) => {
                // Non-Oz harness: "Default model" at top, then server-provided
                // harness models.
                let mut items: Vec<MenuItem<DropdownAction>> = vec![default_model_menu_item::<A>()];
                let availability = HarnessAvailabilityModel::as_ref(ctx_dropdown);
                if let Some(models) = availability.models_for(harness) {
                    for model in models {
                        let model_id = model.id.clone();
                        let fields = MenuItemFields::new(&model.display_name)
                            .with_on_select_action(DropdownAction::select_action_and_close(
                                A::model_changed(model_id),
                            ));
                        items.push(MenuItem::Item(fields));
                    }
                }
                // Find display name before set_rich_items borrows ctx_dropdown mutably.
                let selected_display_name = if initial_model_id.is_empty() {
                    Some(DEFAULT_MODEL_LABEL.to_string())
                } else {
                    availability
                        .models_for(harness)
                        .and_then(|models| {
                            models
                                .iter()
                                .find(|m| m.id == initial_model_id)
                                .map(|m| m.display_name.clone())
                        })
                        .or_else(|| Some(DEFAULT_MODEL_LABEL.to_string()))
                };
                dropdown.set_rich_items(items, ctx_dropdown);
                if let Some(name) = &selected_display_name {
                    dropdown.set_selected_by_name(name, ctx_dropdown);
                }
            }
        }
    });
}

/// Creates a "Default model" menu item that emits an empty model_id.
fn default_model_menu_item<A: OrchestrationControlAction>() -> MenuItem<DropdownAction> {
    MenuItem::Item(
        MenuItemFields::new(DEFAULT_MODEL_LABEL).with_on_select_action(
            DropdownAction::select_action_and_close(A::model_changed(String::new())),
        ),
    )
}

/// Returns whether the given model_id is present in the harness-filtered
/// model choices. Used to detect when a harness change invalidates the
/// current model selection.
pub fn is_model_in_filtered_choices<V: View>(
    model_id: &str,
    harness_type: &str,
    ctx: &mut ViewContext<V>,
) -> bool {
    let harness = Harness::parse_orchestration_harness(harness_type);
    match harness {
        Some(Harness::Oz) | None => LLMPreferences::as_ref(ctx)
            .get_base_llm_choices_for_agent_mode()
            .any(|llm| llm.id.to_string() == model_id),
        Some(Harness::Codex) => model_id.is_empty(),
        Some(harness) => {
            // Empty string is always valid (the "Default model" entry).
            if model_id.is_empty() {
                return true;
            }
            let availability = HarnessAvailabilityModel::as_ref(ctx);
            availability
                .models_for(harness)
                .is_some_and(|models| models.iter().any(|m| m.id == model_id))
        }
    }
}

/// Returns the default model_id for the given harness.
///
/// For Oz this is the first Warp LLM; for non-Oz harnesses it is an empty
/// string (the "Default model" entry).
pub fn first_filtered_model_id<V: View>(
    harness_type: &str,
    ctx: &mut ViewContext<V>,
) -> Option<String> {
    let harness = Harness::parse_orchestration_harness(harness_type);
    match harness {
        Some(Harness::Oz) | None => {
            let llm_prefs = LLMPreferences::as_ref(ctx);
            llm_prefs
                .get_base_llm_choices_for_agent_mode()
                .next()
                .map(|llm| llm.id.to_string())
        }
        Some(_) => Some(String::new()),
    }
}

fn should_show_harness_picker(_state: &OrchestrationEditState) -> bool {
    true
}

pub fn populate_harness_picker<A: OrchestrationControlAction, V: View>(
    dropdown: &ViewHandle<Dropdown<A>>,
    initial_harness: &str,
    ctx: &mut ViewContext<V>,
) {
    let initial_harness = initial_harness.to_string();
    dropdown.update(ctx, |dropdown, ctx_dropdown| {
        let availability = HarnessAvailabilityModel::as_ref(ctx_dropdown);
        let harnesses = availability.available_harnesses();

        let resolve_entry_harness = |harness: Harness, display_name: &str| match harness {
            Harness::Unknown => [
                Harness::Oz,
                Harness::Claude,
                Harness::OpenCode,
                Harness::Gemini,
                Harness::Codex,
            ]
            .into_iter()
            .find(|candidate| harness_display::display_name(*candidate) == display_name)
            .unwrap_or(Harness::Unknown),
            harness => harness,
        };

        // Sort selectable harnesses before disabled ones, preserving
        // relative order within each group.
        // Filter out Gemini — it is not yet supported as a multi-agent
        // harness and causes an infinite "Spawning agents" hang.
        let mut sorted: Vec<_> = harnesses
            .iter()
            .filter(|entry| {
                let harness = resolve_entry_harness(entry.harness, &entry.display_name);
                harness != Harness::Gemini && local_harness_is_product_enabled(harness)
            })
            .collect();
        sorted.sort_by_key(|entry| {
            let harness = resolve_entry_harness(entry.harness, &entry.display_name);
            !(entry.enabled && local_harness_setup_state(harness).is_selectable())
        });

        // Resolve the target harness so we can match by enum variant
        // even when the `initial_harness` string is "claude" but the
        // cached entry.harness deserialized as Unknown.
        let target_harness = Harness::parse_orchestration_harness(&initial_harness);

        let mut items: Vec<MenuItem<DropdownAction>> = Vec::new();
        let mut selected_name: Option<String> = None;
        let target_display = target_harness.map(|harness| availability.display_name_for(harness));

        for entry in sorted {
            let harness = resolve_entry_harness(entry.harness, &entry.display_name);
            let local_setup_state = local_harness_setup_state(harness);
            // Use the server-provided display_name for the label so stale
            // cache entries (where harness deserializes as Unknown) still
            // show the correct name.
            let mut fields = MenuItemFields::new(&entry.display_name)
                .with_icon(harness_display::icon_for(harness));
            if let Some(color) = harness_display::brand_color(harness) {
                fields = fields.with_override_icon_color(Fill::from(color));
            }
            let harness_str = harness.to_string();
            let selectable = entry.enabled && local_setup_state.is_selectable();
            if selectable {
                fields = fields.with_on_select_action(DropdownAction::select_action_and_close(
                    A::harness_changed(harness_str.clone()),
                ));
            } else {
                fields = fields.with_disabled(true);
                let tooltip = match local_setup_state {
                    LocalHarnessSetupState::MissingHarness { tooltip } => tooltip,
                    LocalHarnessSetupState::ProductDisabled { message } => message,
                    LocalHarnessSetupState::Ready => "Disabled by your administrator",
                };
                fields = fields.with_tooltip(tooltip);
            }
            // Match by harness string first, then fall back to matching
            // the display_name against the client-side name for the target
            // harness. This handles stale cache entries where entry.harness
            // is Unknown but entry.display_name is still correct.
            if selected_name.is_none() {
                if harness_str.eq_ignore_ascii_case(&initial_harness) {
                    selected_name = Some(entry.display_name.clone());
                } else if let Some(target_display) = target_display {
                    if entry.display_name == target_display {
                        selected_name = Some(entry.display_name.clone());
                    }
                }
            }
            items.push(MenuItem::Item(fields));
        }
        dropdown.set_rich_items(items, ctx_dropdown);
        if let Some(name) = selected_name {
            dropdown.set_selected_by_name(&name, ctx_dropdown);
        }
    });
}

/// Normalizes a harness_type string for use as a HashMap key in
/// per-harness model memory. Empty string (the wire representation
/// of Oz) is mapped to "oz" so saves and lookups are consistent.
pub fn harness_save_key(harness_type: &str) -> &str {
    if harness_type.is_empty() {
        "oz"
    } else {
        harness_type
    }
}

/// [`OrchestrationEditState::accept_disabled_reason`] plus the local
/// harness setup checks. Card views should prefer this.
pub fn accept_disabled_reason_with_setup(state: &OrchestrationEditState) -> Option<String> {
    if let Some(reason) = state.accept_disabled_reason() {
        return Some(reason.to_string());
    }
    let harness = Harness::parse_local_child_harness(&state.harness_type)?;
    match local_harness_setup_state(harness) {
        LocalHarnessSetupState::MissingHarness { tooltip } => Some(tooltip.to_string()),
        LocalHarnessSetupState::ProductDisabled { message } => Some(message.to_string()),
        LocalHarnessSetupState::Ready => None,
    }
}

// ── Shared action helpers ───────────────────────────────────────────

/// Handles a harness change for both card views: saves/restores per-harness
/// model selection, repopulates the model picker, and re-resolves the auth
/// secret selection for the new harness.
///
/// Does NOT re-enter the harness picker that dispatched this action.
pub fn apply_harness_change<A: OrchestrationControlAction, V: View>(
    state: &mut OrchestrationEditState,
    memory: &mut HashMap<String, String>,
    handles: &OrchestrationPickerHandles<A>,
    new_harness_type: &str,
    fallback_base_model_id: impl FnOnce(&mut ViewContext<V>) -> Option<String>,
    ctx: &mut ViewContext<V>,
) {
    // Save current model for the old harness.
    let old_key = harness_save_key(&state.harness_type).to_string();
    memory.insert(old_key, state.model_id.clone());
    state.harness_type = new_harness_type.to_string();

    state.sanitize_for_local_execution();
    if state.harness_type != new_harness_type {
        if let Some(handle) = &handles.harness_picker {
            populate_harness_picker(handle, &state.harness_type, ctx);
        }
    }
    // Try to restore a previously saved model for this harness.
    let new_key = harness_save_key(&state.harness_type);
    let restored = memory
        .get(new_key)
        .filter(|id| is_model_in_filtered_choices(id, &state.harness_type, ctx))
        .cloned();
    if let Some(saved_id) = restored {
        state.model_id = saved_id;
    } else if !is_model_in_filtered_choices(&state.model_id, &state.harness_type, ctx) {
        // No saved model — fall back to conversation base model
        // for Oz, or default for non-Oz.
        let reset_id = fallback_base_model_id(ctx)
            .filter(|id| is_model_in_filtered_choices(id, &state.harness_type, ctx))
            .or_else(|| first_filtered_model_id(&state.harness_type, ctx))
            .unwrap_or_default();
        state.model_id = reset_id;
    }
    if let Some(handle) = &handles.model_picker {
        populate_model_picker_for_harness(handle, &state.model_id, &state.harness_type, ctx);
    }
}

// ── Picker repopulation + selection sync ──

/// Repopulates the harness, model, and auth-secret pickers from the
/// current server-provided data, revalidates `state.model_id` against
/// the updated catalog (resetting to default if the model disappeared),
/// then re-syncs dropdown selections.
pub fn repopulate_all_pickers<A: OrchestrationControlAction, V: View>(
    state: &mut OrchestrationEditState,
    handles: &OrchestrationPickerHandles<A>,
    ctx: &mut ViewContext<V>,
) {
    state.sanitize_for_local_execution();
    if let Some(handle) = &handles.harness_picker {
        populate_harness_picker(handle, &state.harness_type, ctx);
    }
    // Reset model if it disappeared from the harness's catalog.
    if !is_model_in_filtered_choices(&state.model_id, &state.harness_type, ctx) {
        if let Some(first_id) = first_filtered_model_id(&state.harness_type, ctx) {
            state.model_id = first_id;
        }
    }
    if let Some(handle) = &handles.model_picker {
        populate_model_picker_for_harness(handle, &state.model_id, &state.harness_type, ctx);
    }
    sync_picker_selections(state, handles, ctx);
}

pub fn sync_picker_selections<A: OrchestrationControlAction, V: View>(
    state: &OrchestrationEditState,
    handles: &OrchestrationPickerHandles<A>,
    ctx: &mut ViewContext<V>,
) {
    if let Some(model_picker) = handles.model_picker.clone() {
        let target_model_id = state.model_id.clone();
        let harness_type = state.harness_type.clone();
        model_picker.update(ctx, |dropdown, ctx_dropdown| {
            let harness = Harness::parse_orchestration_harness(&harness_type);
            let display_name = match harness {
                Some(Harness::Oz) | None => {
                    let llm_prefs = LLMPreferences::as_ref(ctx_dropdown);
                    llm_prefs
                        .get_base_llm_choices_for_agent_mode()
                        .find(|llm| llm.id.to_string() == target_model_id)
                        .map(|llm| llm.menu_display_name())
                }
                Some(harness) => {
                    if target_model_id.is_empty() {
                        Some(DEFAULT_MODEL_LABEL.to_string())
                    } else {
                        let availability = HarnessAvailabilityModel::as_ref(ctx_dropdown);
                        availability.models_for(harness).and_then(|models| {
                            models
                                .iter()
                                .find(|m| m.id == target_model_id)
                                .map(|m| m.display_name.clone())
                        })
                    }
                }
            };
            if let Some(name) = &display_name {
                dropdown.set_selected_by_name(name, ctx_dropdown);
            }
        });
    }
    if let Some(harness_picker) = handles.harness_picker.clone() {
        let harness_type = state.harness_type.clone();
        let show_harness_picker = should_show_harness_picker(state);
        harness_picker.update(ctx, |dropdown, ctx_dropdown| {
            if show_harness_picker {
                dropdown.set_enabled(ctx_dropdown);
            } else {
                dropdown.set_disabled(ctx_dropdown);
            }
            let target = Harness::parse_orchestration_harness(&harness_type).unwrap_or(Harness::Oz);
            // Use the server-provided display_name from HarnessAvailabilityModel
            // so the selection matches the labels (which also use display_name).
            let display = HarnessAvailabilityModel::as_ref(ctx_dropdown)
                .display_name_for(target)
                .to_string();
            dropdown.set_selected_by_name(&display, ctx_dropdown);
        });
    }
}

#[cfg(test)]
#[path = "orchestration_controls_tests.rs"]
mod tests;

// ── Adaptive picker layout ──────────────────────────────────────────

/// Lays out children horizontally at a fixed width when they all fit,
/// otherwise stacks them vertically at full available width.
///
/// Switches to vertical when `n * picker_width + (n-1) * spacing` exceeds
/// the available width from the incoming size constraint.
struct AdaptivePickerRow {
    children: Vec<Box<dyn Element>>,
    picker_width: f32,
    spacing: f32,
    is_vertical: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AdaptivePickerRow {
    fn new(picker_width: f32, spacing: f32) -> Self {
        Self {
            children: Vec::new(),
            picker_width,
            spacing,
            is_vertical: false,
            size: None,
            origin: None,
        }
    }

    fn add_child(&mut self, child: Box<dyn Element>) {
        self.children.push(child);
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for AdaptivePickerRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let n = self.children.len();
        if n == 0 {
            self.size = Some(Vector2F::zero());
            return Vector2F::zero();
        }

        let total_horizontal =
            self.picker_width * n as f32 + self.spacing * n.saturating_sub(1) as f32;

        self.is_vertical = total_horizontal > constraint.max.x();

        if self.is_vertical {
            let width = constraint.max.x();
            let mut total_height = 0.0f32;
            for (i, child) in self.children.iter_mut().enumerate() {
                if i > 0 {
                    total_height += self.spacing;
                }
                let child_constraint =
                    SizeConstraint::new(vec2f(width, 0.), vec2f(width, f32::INFINITY));
                let child_size = child.layout(child_constraint, ctx, app);
                total_height += child_size.y();
            }
            let size = vec2f(width, total_height);
            self.size = Some(size);
            size
        } else {
            let mut max_height = 0.0f32;
            for child in self.children.iter_mut() {
                let child_constraint = SizeConstraint::new(
                    vec2f(self.picker_width, 0.),
                    vec2f(self.picker_width, f32::INFINITY),
                );
                let child_size = child.layout(child_constraint, ctx, app);
                max_height = max_height.max(child_size.y());
            }
            let size = vec2f(total_horizontal, max_height);
            self.size = Some(size);
            size
        }
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        for child in &mut self.children {
            child.after_layout(ctx, app);
        }
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let mut current = origin;
        if self.is_vertical {
            for (i, child) in self.children.iter_mut().enumerate() {
                if i > 0 {
                    current += vec2f(0., self.spacing);
                }
                child.paint(current, ctx, app);
                if let Some(size) = child.size() {
                    current += vec2f(0., size.y());
                }
            }
        } else {
            for (i, child) in self.children.iter_mut().enumerate() {
                if i > 0 {
                    current += vec2f(self.spacing, 0.);
                }
                child.paint(current, ctx, app);
                let advance = child.size().map_or(self.picker_width, |s| s.x());
                current += vec2f(advance, 0.);
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let mut handled = false;
        for child in &mut self.children {
            handled |= child.dispatch_event(event, ctx, app);
        }
        handled
    }
}

// ── Render helpers ──────────────────────────────────────────────────

pub fn render_picker_row<A: OrchestrationControlAction>(
    state: &OrchestrationEditState,
    handles: &OrchestrationPickerHandles<A>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    render_picker_row_with_layout(state, handles, appearance, false)
}

/// Renders pickers vertically at full width when `vertical` is true,
/// or in the original horizontal layout when false.
pub fn render_picker_row_with_layout<A: OrchestrationControlAction>(
    state: &OrchestrationEditState,
    handles: &OrchestrationPickerHandles<A>,
    appearance: &Appearance,
    vertical: bool,
) -> Box<dyn Element> {
    let show_harness_picker = should_show_harness_picker(state);

    if vertical {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.);

        let add = |col: &mut Flex, label: &str, picker: Option<Box<dyn Element>>| {
            col.add_child(render_picker_column(label, picker, appearance));
        };

        // Plan-card ordering puts the harness picker before the model picker
        // so it does not split the model picker from the "Primary model…" subtext
        // that follows the picker row.
        if show_harness_picker {
            add(
                &mut column,
                "Agent harness",
                handles
                    .harness_picker
                    .as_ref()
                    .map(|p| ChildView::new(p).finish()),
            );
        }
        add(
            &mut column,
            "Base model",
            handles
                .model_picker
                .as_ref()
                .map(|p| ChildView::new(p).finish()),
        );

        Container::new(column.finish())
            .with_margin_top(12.)
            .finish()
    } else {
        let mut row = AdaptivePickerRow::new(ORCHESTRATION_PICKER_MAX_WIDTH, 12.);

        let add_picker =
            |row: &mut AdaptivePickerRow, label: &str, picker: Option<Box<dyn Element>>| {
                let col = render_picker_column(label, picker, appearance);
                row.add_child(col);
            };

        if show_harness_picker {
            add_picker(
                &mut row,
                "Agent harness",
                handles
                    .harness_picker
                    .as_ref()
                    .map(|p| ChildView::new(p).finish()),
            );
        }
        add_picker(
            &mut row,
            "Base model",
            handles
                .model_picker
                .as_ref()
                .map(|p| ChildView::new(p).finish()),
        );

        Container::new(row.finish()).with_margin_top(12.).finish()
    }
}

pub fn render_picker_column(
    label: &str,
    picker: Option<Box<dyn Element>>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let label_el = Text::new(
        label.to_string(),
        appearance.ui_font_family(),
        appearance.monospace_font_size() - 1.,
    )
    .with_color(blended_colors::text_disabled(theme, theme.surface_1()))
    .finish();

    let body: Box<dyn Element> = picker.unwrap_or_else(|| Empty::new().finish());
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(label_el)
        .with_child(body)
        .finish()
}

pub fn render_validation_error(
    reason: impl Into<String>,
    color: ColorU,
    appearance: &Appearance,
) -> Box<dyn Element> {
    Container::new(
        Text::new(
            reason.into(),
            appearance.ui_font_family(),
            appearance.monospace_font_size() - 1.,
        )
        .with_color(color)
        .finish(),
    )
    .with_margin_bottom(8.)
    .finish()
}
