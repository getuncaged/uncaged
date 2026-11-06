use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use ::settings::{Setting, SettingSection, ToggleableSetting};
use enum_iterator::all;
use warp_core::ui::theme::color::internal_colors;
use warp_util::path::user_friendly_path;
use warpui::elements::{
    Align, Border, ChildView, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    Dismiss, Element, Empty, Flex, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Radius, Shrinkable, Text, Wrap, DEFAULT_UI_LINE_HEIGHT_RATIO,
};
use warpui::fonts::{FamilyId, FontInfo, Properties, Weight};
use warpui::keymap::{ContextPredicate, FixedBinding};
use warpui::platform::{Cursor, FilePickerConfiguration, GraphicsBackend, SystemTheme};
use warpui::rendering::ThinStrokes;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::slider::SliderStateHandle;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::units::IntoPixels;
use warpui::{
    id, Action, AppContext, Entity, ModelHandle, SingletonEntity, TypedActionView, UpdateModel,
    View, ViewContext, ViewHandle, WindowId,
};

use super::directory_color_add_picker::{DirectoryColorAddPicker, DirectoryColorAddPickerEvent};
use super::settings_page::{
    build_reset_button, render_body_item, render_body_item_label, render_dropdown_item,
    render_segmented_control, render_settings_description, render_slider_row, Category,
    LocalOnlyIconState, MatchData, PageType, Segment, SettingsPageEvent, SettingsPageMeta,
    SettingsPageViewHandle, SettingsWidget, SliderRow, ToggleState, CONTENT_FONT_SIZE,
    HEADER_PADDING, ROW_CONTROL_GAP,
};
use super::{
    flags, SettingActionPairContexts, SettingActionPairDescriptions, SettingsAction,
    SettingsSection, ToggleSettingActionPair,
};
use crate::appearance::{Appearance, AppearanceEvent};
use crate::channel::{Channel, ChannelState};
use crate::context_chips::prompt::{Prompt, PromptEvent};
use crate::context_chips::renderer::{ChipDragState, Renderer as ContextChipRenderer};
use crate::context_chips::ChipAvailability;
use crate::editor::{
    EditOrigin, EditorView, Event as EditorEvent, InteractionState, SingleLineEditorOptions,
    TextOptions,
};
use crate::features::FeatureFlag;
use crate::gpu_state::{GPUState, GPUStateEvent};
use crate::prompt::editor_modal::OpenSource as PromptEditorOpenSource;
use crate::server::telemetry::{InputUXChangeOrigin, TelemetryEvent};
use crate::settings::app_icon::AppIconSettings;
use crate::settings::{
    active_theme_kind, respect_system_theme, AIFontName, AppEditorSettings, CursorBlink,
    CursorDisplayType, EnforceMinimumContrast, FontSettings, FontSettingsChangedEvent, GPUSettings,
    InputBoxType, InputModeSettings, InputSettings, InputSettingsChangedEvent, MonospaceFontName,
    PaneSettings, ThemeSettings, DEFAULT_MONOSPACE_FONT_NAME,
};
use crate::terminal::block_list_viewport::InputMode;
use crate::terminal::blockgrid_element::BlockGridElement;
use crate::terminal::ligature_settings::LigatureSettings;
use crate::terminal::model::blockgrid::BlockGrid;
use crate::terminal::model::ObfuscateSecrets;
use crate::terminal::session_settings::SessionSettings;
use crate::terminal::settings::{AltScreenPaddingMode, SpacingMode, TerminalSettings};
use crate::terminal::{BlockListSettings, SizeInfo};
use crate::themes::theme::{self, RespectSystemTheme, SelectedSystemThemes, ThemeGroup, ThemeKind};
use crate::ui_components::color_dot::{render_color_dot, TAB_COLOR_OPTIONS};
use crate::ui_components::icons::Icon;
use crate::user_config::{WarpConfig, WarpConfigUpdateEvent};
use crate::util::bindings;
use crate::view_components::action_button::{ActionButton, ButtonSize, NakedTheme};
use crate::view_components::dropdown::{DROPDOWN_PADDING, TOP_MENU_BAR_MAX_WIDTH};
use crate::view_components::{Dropdown, DropdownItem, FilterableDropdown, FilterableDropdownEvent};
use crate::window_settings::{
    BackgroundBlurRadius, BackgroundOpacity, WindowSettings, WindowSettingsChangedEvent, ZoomLevel,
};
use crate::workspace::header_toolbar_editor::HeaderToolbarInlineEditor;
use crate::workspace::tab_settings::{
    canonical_directory_key, DirectoryTabColor, TabCloseButtonPosition, TabSettings,
    TabSettingsChangedEvent, WorkspaceDecorationVisibility,
};
use crate::workspace::WorkspaceAction;
use crate::{report_error, report_if_error, send_telemetry_from_ctx, themes};

/// One width for every dropdown on the page, so their edges line up down the column. The same
/// width dropdowns default to elsewhere in Settings.
const DROPDOWN_WIDTH: f32 = TOP_MENU_BAR_MAX_WIDTH;
/// Every numeric field on the page (font sizes, line height, window size, padding).
const NUMBER_INPUT_WIDTH: f32 = 64.;
/// The current-theme preview on the Theme card, as a fraction of a full preview card.
const CURRENT_THEME_PREVIEW_SCALE: f32 = 0.45;
const MIN_FONT_SIZE: usize = 1;
const MAX_FONT_SIZE: usize = 120;
const MIN_LINE_SPACING: f32 = 0.1;
const MAX_LINE_SPACING: f32 = 5.;

// Max and min sizes for new window creation in terms of rows and cols
const MIN_NEW_WINDOW_ROWS_OR_COLS: u16 = 5;
const MAX_NEW_WINDOW_ROWS_OR_COLS: u16 = 2000;

fn default_font_label(is_ai_font: bool) -> String {
    if is_ai_font {
        format!("{} (default)", AIFontName::default_value())
    } else {
        format!("{} (default)", MonospaceFontName::default_value())
    }
}

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    // Add all the toggle settings from the Appearance Page that you want to show up on the Command Palette here.
    let mut toggle_binding_pairs = vec![
        ToggleSettingActionPair::new(
            "compact mode",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleCompactMode,
            )),
            context,
            flags::COMPACT_MODE_CONTEXT_FLAG,
        ),
        ToggleSettingActionPair::new(
            "themes: sync with OS",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleRespectSystemTheme,
            )),
            context,
            flags::RESPECT_SYSTEM_THEME_CONTEXT_FLAG,
        ),
    ];

    toggle_binding_pairs.push(
        ToggleSettingActionPair::new(
            "cursor blink",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleCursorBlink,
            )),
            context,
            flags::CURSOR_BLINK_CONTEXT_FLAG,
        )
        .is_supported_on_current_platform(
            AppEditorSettings::as_ref(app)
                .cursor_blink
                .is_supported_on_current_platform(),
        ),
    );

    toggle_binding_pairs.push(
        ToggleSettingActionPair::new(
            "jump to bottom of block button",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleJumpToBottomOfBlockButton,
            )),
            context,
            flags::JUMP_TO_BOTTOM_OF_BLOCK_BUTTON_CONTEXT_FLAG,
        )
        .is_supported_on_current_platform(
            BlockListSettings::as_ref(app)
                .show_jump_to_bottom_of_block_button
                .is_supported_on_current_platform(),
        ),
    );

    toggle_binding_pairs.push(
        ToggleSettingActionPair::new(
            "block dividers",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleShowBlockDividers,
            )),
            context,
            flags::BLOCK_DIVIDERS_CONTEXT_FLAG,
        )
        .is_supported_on_current_platform(
            BlockListSettings::as_ref(app)
                .show_block_dividers
                .is_supported_on_current_platform(),
        ),
    );

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "launch animation",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleLaunchAnimation,
        )),
        context,
        flags::LAUNCH_ANIMATION_CONTEXT_FLAG,
    ));

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "dim inactive panes",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleDimInactivePanes,
        )),
        context,
        flags::DIM_INACTIVE_PANES_FLAG,
    ));

    app.register_fixed_bindings(vec![FixedBinding::empty(
        "Start Input at the Top".to_string(),
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::SetInputMode {
                new_mode: InputMode::Waterfall,
                from_binding: true,
            },
        )),
        context.to_owned(),
    )
    .with_group(bindings::BindingGroup::Settings.as_str())]);

    app.register_fixed_bindings(vec![FixedBinding::empty(
        "Pin Input to the Top".to_string(),
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::SetInputMode {
                new_mode: InputMode::PinnedToTop,
                from_binding: true,
            },
        )),
        context.to_owned(),
    )
    .with_group(bindings::BindingGroup::Settings.as_str())]);

    app.register_fixed_bindings(vec![FixedBinding::empty(
        "Pin Input to the Bottom".to_string(),
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::SetInputMode {
                new_mode: InputMode::PinnedToBottom,
                from_binding: true,
            },
        )),
        context.to_owned(),
    )]);

    // Add command palette entry for toggling between Warp and Classic input modes
    app.register_fixed_bindings(vec![FixedBinding::empty(
        "Toggle Input Mode (Uncaged/Classic)".to_string(),
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleInputMode,
        )),
        context.to_owned(),
    )
    .with_group(bindings::BindingGroup::Settings.as_str())]);
    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "open new windows with custom size",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleOpenWindowsAtCustomSize,
        )),
        context,
        flags::OPEN_WINDOWS_AT_CUSTOM_SIZE_FLAG,
    ));

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "window blur acrylic texture",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleBlurTexture,
        )),
        context,
        flags::WINDOW_BLUR_TEXTURE_FLAG,
    ));

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "tools panel visibility across tabs",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleLeftPanelVisibility,
        )),
        context,
        flags::LEFT_PANEL_VISIBILITY_ACROSS_TABS_FLAG,
    ));

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "agent font matching terminal font",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleMatchAIToTerminalFontFamily,
        )),
        context,
        flags::MATCH_AI_FONT_TO_TERMINAL_FONT_FLAG,
    ));

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "notebook font size matching terminal font size",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleMatchNotebookToMonospaceFontSize,
        )),
        context,
        flags::MATCH_NOTEBOOK_FONT_SIZE_TO_TERMINAL_FONT_SIZE_FLAG,
    ));

    toggle_binding_pairs.push(
        ToggleSettingActionPair::new(
            "tab indicators",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleTabIndicators,
            )),
            context,
            flags::TAB_INDICATORS_FLAG,
        )
        .is_supported_on_current_platform(
            TabSettings::as_ref(app)
                .show_indicators
                .is_supported_on_current_platform(),
        ),
    );

    if !FeatureFlag::OpenWarpNewSettingsModes.is_enabled() {
        toggle_binding_pairs.push(
            ToggleSettingActionPair::custom(
                SettingActionPairDescriptions::new(
                    "Show code review button in tab bar",
                    "Hide code review button in tab bar",
                ),
                builder(SettingsAction::AppearancePageToggle(
                    AppearancePageAction::ToggleShowCodeReviewButton,
                )),
                SettingActionPairContexts::new(
                    context.to_owned() & !id!(flags::SHOW_CODE_REVIEW_BUTTON_FLAG),
                    context.to_owned() & id!(flags::SHOW_CODE_REVIEW_BUTTON_FLAG),
                ),
                None,
            )
            .is_supported_on_current_platform(
                TabSettings::as_ref(app)
                    .show_code_review_button
                    .is_supported_on_current_platform(),
            ),
        );
    }

    toggle_binding_pairs.push(
        ToggleSettingActionPair::new(
            "focus follows mouse",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleFocusPaneOnHover,
            )),
            context,
            flags::FOCUS_PANES_ON_HOVER_CONTEXT_FLAG,
        )
        .is_supported_on_current_platform(
            PaneSettings::as_ref(app)
                .focus_panes_on_hover
                .is_supported_on_current_platform(),
        ),
    );

    if FeatureFlag::FullScreenZenMode.is_enabled() {
        // Add bindings for each visibility option.
        app.register_fixed_bindings([
            FixedBinding::empty(
                "Always show tab bar".to_string(),
                builder(SettingsAction::AppearancePageToggle(
                    AppearancePageAction::SetWorkspaceDecorationVisibility(
                        WorkspaceDecorationVisibility::AlwaysShow,
                    ),
                )),
                context.to_owned(),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
            FixedBinding::empty(
                "Hide tab bar if fullscreen".to_string(),
                builder(SettingsAction::AppearancePageToggle(
                    AppearancePageAction::SetWorkspaceDecorationVisibility(
                        WorkspaceDecorationVisibility::HideFullscreen,
                    ),
                )),
                context.to_owned(),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
            FixedBinding::empty(
                "Only show tab bar on hover".to_string(),
                builder(SettingsAction::AppearancePageToggle(
                    AppearancePageAction::SetWorkspaceDecorationVisibility(
                        WorkspaceDecorationVisibility::OnHover,
                    ),
                )),
                context.to_owned(),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
        ]);

        // Add a toggle alias for "Zen mode".
        toggle_binding_pairs.push(
            ToggleSettingActionPair::new(
                "zen mode",
                builder(SettingsAction::AppearancePageToggle(
                    AppearancePageAction::ToggleWorkspaceDecorationVisibility,
                )),
                context,
                flags::HIDE_WORKSPACE_DECORATIONS_CONTEXT_FLAG,
            )
            .is_supported_on_current_platform(
                TabSettings::as_ref(app)
                    .workspace_decoration_visibility
                    .is_supported_on_current_platform(),
            ),
        )
    }

    if FeatureFlag::VerticalTabs.is_enabled() {
        toggle_binding_pairs.push(ToggleSettingActionPair::new(
            "vertical tab layout",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleVerticalTabs,
            )),
            context,
            flags::USE_VERTICAL_TABS_FLAG,
        ));
        toggle_binding_pairs.push(ToggleSettingActionPair::new(
            "show vertical tabs panel in restored windows",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleShowVerticalTabPanelInRestoredWindows,
            )),
            context,
            flags::SHOW_VERTICAL_TAB_PANEL_IN_RESTORED_WINDOWS_FLAG,
        ));
        toggle_binding_pairs.push(ToggleSettingActionPair::new(
            "latest user prompt as conversation title in tab names",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleUseLatestUserPromptAsConversationTitleInTabNames,
            )),
            context,
            flags::USE_LATEST_USER_PROMPT_AS_CONVERSATION_TITLE_IN_TAB_NAMES_FLAG,
        ));
    }

    if FeatureFlag::Ligatures.is_enabled() {
        toggle_binding_pairs.push(ToggleSettingActionPair::new(
            "ligature rendering",
            builder(SettingsAction::AppearancePageToggle(
                AppearancePageAction::ToggleLigatureRendering,
            )),
            context,
            flags::LIGATURE_RENDERING_CONTEXT_FLAG,
        ));
    }

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "preserve active tab color for new tabs",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::TogglePreserveActiveTabColor,
        )),
        context,
        flags::PRESERVE_ACTIVE_TAB_COLOR_FLAG,
    ));

    toggle_binding_pairs.push(ToggleSettingActionPair::new(
        "custom padding in alt-screen",
        builder(SettingsAction::AppearancePageToggle(
            AppearancePageAction::ToggleAltScreenPadding,
        )),
        context,
        flags::ALT_SCREEN_PADDING_FLAG,
    ));

    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(toggle_binding_pairs, app);
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum FontType {
    Any,
    #[default]
    Monospace,
}

impl FontType {
    fn toggle(self) -> Self {
        match self {
            Self::Monospace => Self::Any,
            Self::Any => Self::Monospace,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppearancePageAction {
    LineHeightEditorResetRatio,
    SetNewWindowsCustomColumns,
    SetNewWindowsCustomRows,
    SetFontSize,
    SetFontWeight(Weight),
    SetNotebookFontSize,
    SetLineHeight,
    SetOpacity(f32),
    SetBlur(f32),
    OpacitySliderDragged(f32),
    BlurSliderDragged(f32),
    SetFontFamily(String),
    SetAIFontFamily(String),
    SetThinStrokes(ThinStrokes),
    SetInputMode {
        new_mode: InputMode,
        from_binding: bool,
    },
    SetInputType(InputBoxType),
    ToggleShowDockIcon,
    SetCursorType(CursorDisplayType),
    SetWorkspaceDecorationVisibility(WorkspaceDecorationVisibility),
    ToggleWorkspaceDecorationVisibility,
    ToggleJumpToBottomOfBlockButton,
    ToggleShowBlockDividers,
    ToggleCompactMode,
    ToggleCursorBlink,
    ToggleRespectSystemTheme,
    ToggleOpenWindowsAtCustomSize,
    ToggleLaunchAnimation,
    ToggleDimInactivePanes,
    ToggleAllAvailableFonts,
    ToggleMatchNotebookToMonospaceFontSize,
    ToggleMatchAIToTerminalFontFamily,
    ToggleTabIndicators,
    ToggleShowCodeReviewButton,
    TogglePreserveActiveTabColor,
    ToggleVerticalTabs,
    ToggleShowVerticalTabPanelInRestoredWindows,
    ToggleHideTitleBarSearchBarInVerticalTabs,
    ToggleUseLatestUserPromptAsConversationTitleInTabNames,
    ToggleLigatureRendering,
    ToggleBlurTexture,
    ToggleLeftPanelVisibility,
    SetEnforceMinimumContrast(EnforceMinimumContrast),
    /// The theme used while the system is in `appearance`, when "Sync with OS" is on.
    SetSystemTheme {
        appearance: SystemTheme,
        theme: ThemeKind,
    },
    ToggleFocusPaneOnHover,
    ToggleInputMode,
    ToggleAltScreenPadding,
    UpdateAltScreenPaddingMode(AltScreenPaddingMode),
    SetTabCloseButtonPosition(TabCloseButtonPosition),
    SetZoomLevel(u16),
    ResetZoomLevel,
    SetDefaultDirectoryTabColor {
        path: PathBuf,
        color: DirectoryTabColor,
    },
    RemoveDefaultDirectoryTabColor {
        path: PathBuf,
    },
}

pub struct AppearanceSettingsPageView {
    page: PageType<Self>,
    window_id: WindowId,
    font_size_editor: ViewHandle<EditorView>,
    line_height_editor: ViewHandle<EditorView>,
    notebook_font_size_editor: ViewHandle<EditorView>,
    ai_font_family_dropdown: ViewHandle<FilterableDropdown<AppearancePageAction>>,
    new_window_columns_editor: ViewHandle<EditorView>,
    valid_new_window_columns: bool,
    new_window_rows_editor: ViewHandle<EditorView>,
    valid_new_window_rows: bool,
    /// The window opacity and blur sliders' states. Owned here rather than by their rows so a
    /// change made elsewhere (the theme creator, the settings file) can reset them.
    opacity_state: SliderStateHandle,
    blur_state: SliderStateHandle,
    font_family_dropdown: ViewHandle<FilterableDropdown<AppearancePageAction>>,
    font_weight_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    thin_strokes_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    enforce_min_contrast_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    input_mode_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    /// The themes for light and dark system appearance, shown while "Sync with OS" is on.
    light_theme_dropdown: ViewHandle<FilterableDropdown<AppearancePageAction>>,
    dark_theme_dropdown: ViewHandle<FilterableDropdown<AppearancePageAction>>,
    workspace_decorations_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    tab_close_button_position_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    zoom_level_dropdown: ViewHandle<Dropdown<AppearancePageAction>>,
    zoom_reset_button_mouse_state: MouseStateHandle,
    available_families: HashMap<String, (Option<FamilyId>, FontType)>,
    /// Uncaged: whether system-font enumeration has been kicked off (on first dropdown open).
    system_fonts_requested: bool,
    view_font_type: FontType,
    alt_screen_padding_editor: ViewHandle<EditorView>,
    color_picker_dot_states: Vec<Vec<MouseStateHandle>>,
    directory_tab_color_delete_buttons: Vec<ViewHandle<ActionButton>>,
    header_toolbar_inline_editor: ViewHandle<HeaderToolbarInlineEditor>,

    /// The context chip renderers based on the most recently
    /// selected Warp prompt configuration.
    context_chips: Vec<ContextChipRenderer>,

    /// The information we need to render the PS1 as a grid when we're
    /// honoring the user's PS1.
    ps1_grid_info: Option<(BlockGrid, SizeInfo)>,
}

impl Entity for AppearanceSettingsPageView {
    type Event = SettingsPageEvent;
}

impl TypedActionView for AppearanceSettingsPageView {
    type Action = AppearancePageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        use AppearancePageAction::*;

        match action {
            LineHeightEditorResetRatio => self.reset_line_height_ratio(ctx),
            SetNewWindowsCustomColumns => self.update_new_windows_num_columns(true, ctx),
            SetNewWindowsCustomRows => self.update_new_windows_num_rows(true, ctx),
            SetFontSize => self.set_font_size(ctx),
            SetFontWeight(value) => self.set_font_weight(*value, ctx),
            ToggleMatchNotebookToMonospaceFontSize => {
                FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
                    report_if_error!(font_settings
                        .match_notebook_to_monospace_font_size
                        .toggle_and_save_value(ctx));
                });
            }
            ToggleMatchAIToTerminalFontFamily => self.toggle_match_ai_font_to_terminal_font(ctx),
            SetNotebookFontSize => self.set_notebook_font_size(ctx),
            SetLineHeight => self.set_line_height_ratio(ctx),
            SetOpacity(value) => self.set_opacity(*value, true, ctx),
            SetBlur(value) => self.set_blur(*value, true, ctx),
            SetFontFamily(name) => self.set_font_family(name, ctx),
            SetAIFontFamily(name) => {
                self.set_ai_font_family(name, ctx);
                FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
                    report_if_error!(font_settings
                        .match_ai_font_to_terminal_font
                        .set_value(false, ctx));
                });
            }
            SetThinStrokes(value) => self.set_thin_strokes(value, ctx),
            SetEnforceMinimumContrast(value) => {
                FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
                    report_if_error!(font_settings
                        .enforce_minimum_contrast
                        .set_value(*value, ctx,));
                });
            }
            SetWorkspaceDecorationVisibility(value) => {
                self.set_workspace_decoration_visibility(*value, ctx)
            }
            ToggleWorkspaceDecorationVisibility => self.toggle_workspace_decoration_visiblity(ctx),
            ToggleJumpToBottomOfBlockButton => self.toggle_jump_to_bottom_of_block_button(ctx),
            ToggleShowBlockDividers => self.toggle_show_block_dividers(ctx),
            ToggleCompactMode => self.toggle_compact_mode(ctx),
            ToggleCursorBlink => self.toggle_cursor_blink(ctx),
            ToggleOpenWindowsAtCustomSize => self.toggle_open_windows_at_custom_size(ctx),
            ToggleLaunchAnimation => self.toggle_launch_animation(ctx),
            ToggleRespectSystemTheme => self.toggle_respect_system_theme(ctx),
            ToggleAllAvailableFonts => self.toggle_all_available_fonts(ctx),
            ToggleDimInactivePanes => self.toggle_dim_inactive_panes(ctx),
            ToggleBlurTexture => self.toggle_blur_texture(ctx),
            ToggleLeftPanelVisibility => self.toggle_left_panel_visibility(ctx),
            SetInputMode {
                new_mode,
                from_binding,
            } => self.set_input_mode(*new_mode, *from_binding, ctx),
            SetInputType(input_type) => self.set_input_type(*input_type, ctx),
            ToggleShowDockIcon => self.toggle_show_dock_icon(ctx),
            SetCursorType(cursor_display_type) => self.set_cursor_type(*cursor_display_type, ctx),
            OpacitySliderDragged(val) => self.set_opacity(*val, false, ctx),
            BlurSliderDragged(val) => self.set_blur(*val, false, ctx),
            SetSystemTheme { appearance, theme } => {
                self.set_system_theme(*appearance, theme.clone(), ctx)
            }
            ToggleTabIndicators => self.toggle_tab_indicators(ctx),
            ToggleShowCodeReviewButton => self.toggle_show_code_review_button(ctx),
            TogglePreserveActiveTabColor => self.toggle_preserve_active_tab_color(ctx),
            ToggleVerticalTabs => self.toggle_vertical_tabs(ctx),
            ToggleShowVerticalTabPanelInRestoredWindows => {
                self.toggle_show_vertical_tab_panel_in_restored_windows(ctx)
            }
            ToggleHideTitleBarSearchBarInVerticalTabs => {
                self.toggle_hide_title_bar_search_bar_in_vertical_tabs(ctx)
            }
            ToggleUseLatestUserPromptAsConversationTitleInTabNames => {
                self.toggle_use_latest_user_prompt_as_conversation_title_in_tab_names(ctx)
            }
            ToggleLigatureRendering => self.toggle_ligature_rendering(ctx),
            ToggleFocusPaneOnHover => {
                PaneSettings::handle(ctx).update(ctx, |pane_settings, ctx| {
                    match pane_settings
                        .focus_panes_on_hover
                        .toggle_and_save_value(ctx)
                    {
                        Ok(new_val) => {
                            send_telemetry_from_ctx!(
                                TelemetryEvent::ToggleFocusPaneOnHover { enabled: new_val },
                                ctx
                            );
                        }
                        Err(e) => {
                            report_error!(e);
                        }
                    }
                });
                ctx.notify();
            }
            ToggleInputMode => {
                self.toggle_input_mode(ctx);
            }
            ToggleAltScreenPadding => {
                let new_mode = TerminalSettings::as_ref(ctx).alt_screen_padding.toggled();
                TerminalSettings::handle(ctx).update(ctx, |terminal_settings, ctx| {
                    report_if_error!(terminal_settings
                        .alt_screen_padding
                        .set_value(new_mode, ctx));
                });
                self.set_alt_screen_padding_editor_text(ctx);
                send_telemetry_from_ctx!(
                    TelemetryEvent::UpdateAltScreenPaddingMode { new_mode },
                    ctx
                );
            }
            UpdateAltScreenPaddingMode(new_mode) => {
                TerminalSettings::handle(ctx).update(ctx, |terminal_settings, ctx| {
                    report_if_error!(terminal_settings
                        .alt_screen_padding
                        .set_value(*new_mode, ctx));
                });
                self.set_alt_screen_padding_editor_text(ctx);
                send_telemetry_from_ctx!(
                    TelemetryEvent::UpdateAltScreenPaddingMode {
                        new_mode: *new_mode,
                    },
                    ctx
                );
            }
            SetTabCloseButtonPosition(position) => {
                self.update_tab_close_button_position(*position, ctx);
            }
            SetZoomLevel(zoom_level) => {
                WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
                    report_if_error!(window_settings.zoom_level.set_value(*zoom_level, ctx));
                });
                ctx.notify();
            }
            ResetZoomLevel => {
                WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
                    report_if_error!(window_settings.zoom_level.clear_value(ctx));
                });
                ctx.notify();
            }
            SetDefaultDirectoryTabColor { path, color } => {
                let path = path.clone();
                TabSettings::handle(ctx).update(ctx, |settings, ctx| {
                    let new_value = settings
                        .directory_tab_colors
                        .value()
                        .with_color(&path, *color);
                    let _ = settings.directory_tab_colors.set_value(new_value, ctx);
                });
                ctx.notify();
            }
            RemoveDefaultDirectoryTabColor { path } => {
                let path = path.clone();
                TabSettings::handle(ctx).update(ctx, |settings, ctx| {
                    let new_value = settings
                        .directory_tab_colors
                        .value()
                        .with_color(&path, DirectoryTabColor::Suppressed);
                    let _ = settings.directory_tab_colors.set_value(new_value, ctx);
                });
                ctx.notify();
            }
        }
    }
}

impl View for AppearanceSettingsPageView {
    fn ui_name() -> &'static str {
        "AppearanceSettingsPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl AppearanceSettingsPageView {
    fn editor<F>(
        mut event_handler: F,
        buffer_text: &str,
        ui_font_size: f32,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView>
    where
        F: 'static + FnMut(&mut AppearanceSettingsPageView, &EditorEvent, &mut ViewContext<Self>),
    {
        let editor = ctx.add_typed_action_view(|ctx| {
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(ui_font_size),
                    ..Default::default()
                },
                ..Default::default()
            };
            EditorView::single_line(options, ctx)
        });

        editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(buffer_text, ctx);
        });

        ctx.subscribe_to_view(&editor, move |me, _, event, ctx| {
            event_handler(me, event, ctx);
        });

        editor
    }

    pub fn new(ctx: &mut ViewContext<AppearanceSettingsPageView>) -> Self {
        let (
            ui_font_size,
            monospace_font_size,
            line_height_ratio,
            monospace_font_weight,
            notebook_font_size,
            match_notebook_to_monospace_font_size,
        ) = {
            let appearance = Appearance::as_ref(ctx);
            let font_settings = FontSettings::as_ref(ctx);
            (
                appearance.ui_font_size(),
                appearance.monospace_font_size(),
                appearance.line_height_ratio(),
                appearance.monospace_font_weight(),
                *font_settings.notebook_font_size,
                *font_settings.match_notebook_to_monospace_font_size,
            )
        };

        let font_size_editor = Self::editor(
            |me, event, ctx| me.handle_font_size_editor_event(event, ctx),
            &format!("{monospace_font_size}"),
            ui_font_size,
            ctx,
        );

        let notebook_font_size_editor = Self::editor(
            |me, event, ctx| me.handle_notebook_font_size_editor_event(event, ctx),
            &format!("{notebook_font_size}"),
            ui_font_size,
            ctx,
        );

        if match_notebook_to_monospace_font_size {
            notebook_font_size_editor.update(ctx, |editor_view, ctx| {
                editor_view.set_interaction_state(InteractionState::Disabled, ctx);
            })
        }

        ctx.subscribe_to_model(&GPUState::handle(ctx), |_, _, event, ctx| {
            if matches!(event, GPUStateEvent::LowPowerGPUAvailable) {
                ctx.notify();
            }
        });

        let appearance_handle = Appearance::handle(ctx);
        ctx.subscribe_to_model(&appearance_handle, Self::handle_appearance_update);

        ctx.subscribe_to_model(&PaneSettings::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_model(&TerminalSettings::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_model(
            &FontSettings::handle(ctx),
            |me, font_settings, event, ctx| match event {
                FontSettingsChangedEvent::NotebookFontSize { .. }
                | FontSettingsChangedEvent::MatchNotebookToMonospaceFontSize { .. } => {
                    let font_settings = font_settings.as_ref(ctx);
                    let should_match_notebook_to_monospace_font_size =
                        *font_settings.match_notebook_to_monospace_font_size;
                    let notebook_font_size = *font_settings.notebook_font_size;

                    me.notebook_font_size_editor
                        .update(ctx, move |editor, ctx| {
                            let interaction_state = if should_match_notebook_to_monospace_font_size
                            {
                                InteractionState::Disabled
                            } else {
                                InteractionState::Editable
                            };
                            editor.set_buffer_text(&format!("{notebook_font_size}"), ctx);
                            editor.set_interaction_state(interaction_state, ctx);
                        });

                    ctx.notify();
                }
                FontSettingsChangedEvent::EnforceMinimumContrast { .. } => {
                    me.enforce_min_contrast_dropdown
                        .update(ctx, |dropdown, ctx| {
                            let enforce_minimum_contrast =
                                *FontSettings::as_ref(ctx).enforce_minimum_contrast;
                            let name = Self::enforce_minimum_contrast_dropdown_item_label(
                                enforce_minimum_contrast,
                            );
                            dropdown.set_selected_by_name(name, ctx);
                        });
                    ctx.notify();
                }
                FontSettingsChangedEvent::UseThinStrokes { .. } => {
                    me.thin_strokes_dropdown.update(ctx, |dropdown, ctx| {
                        let thin_strokes = *FontSettings::as_ref(ctx).use_thin_strokes;
                        dropdown.set_selected_by_name(
                            Self::thin_strokes_dropdown_item_label(thin_strokes),
                            ctx,
                        );
                    });
                    ctx.notify();
                }
                _ => {}
            },
        );

        let block_list_settings_handle = BlockListSettings::handle(ctx);
        ctx.subscribe_to_model(&block_list_settings_handle, |_, _, _, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_model(&Prompt::handle(ctx), Self::handle_prompt_update);

        let ligature_settings_handle = LigatureSettings::handle(ctx);
        ctx.subscribe_to_model(&ligature_settings_handle, |_, _, _, ctx| ctx.notify());

        ctx.subscribe_to_model(&InputModeSettings::handle(ctx), |me, _, _, ctx| {
            me.input_mode_dropdown.update(ctx, |dropdown, ctx| {
                let input_mode = *InputModeSettings::as_ref(ctx).input_mode;
                dropdown
                    .set_selected_by_name(Self::input_mode_dropdown_item_label(input_mode), ctx);
                ctx.notify();
            });
            ctx.notify()
        });

        ctx.subscribe_to_model(&InputSettings::handle(ctx), |_, _, event, ctx| {
            if matches!(event, InputSettingsChangedEvent::InputBoxTypeSetting { .. }) {
                ctx.notify();
            }
        });
        ctx.subscribe_to_model(&AppIconSettings::handle(ctx), |_, _, _, ctx| ctx.notify());
        // The light and dark theme pickers follow the theme settings (changed from the gallery or
        // the command palette too) and the set of installed themes.
        ctx.subscribe_to_model(&ThemeSettings::handle(ctx), |me, _, _, ctx| {
            me.refresh_system_theme_dropdowns(ctx);
            ctx.notify();
        });
        ctx.subscribe_to_model(&WarpConfig::handle(ctx), |me, _, event, ctx| {
            if let WarpConfigUpdateEvent::Themes = event {
                me.refresh_system_theme_dropdowns(ctx);
                ctx.notify();
            }
        });
        ctx.subscribe_to_model(&SessionSettings::handle(ctx), |_, _, _, ctx| ctx.notify());
        ctx.subscribe_to_model(&BlockListSettings::handle(ctx), |_, _, _, ctx| ctx.notify());
        ctx.subscribe_to_model(&WindowSettings::handle(ctx), |me, _, evt, ctx| {
            match evt {
                WindowSettingsChangedEvent::NewWindowsNumColumns { .. } => {
                    // Update the value of the columns input to match the new setting
                    me.new_window_columns_editor.update(ctx, |editor, ctx| {
                        editor.set_buffer_text(
                            &format!(
                                "{}",
                                WindowSettings::handle(ctx)
                                    .as_ref(ctx)
                                    .new_windows_num_columns
                                    .value()
                            ),
                            ctx,
                        );
                    });
                }
                WindowSettingsChangedEvent::NewWindowsNumRows { .. } => {
                    // Update the value of the rows input to match the new setting
                    me.new_window_rows_editor.update(ctx, |editor, ctx| {
                        editor.set_buffer_text(
                            &format!(
                                "{}",
                                WindowSettings::handle(ctx)
                                    .as_ref(ctx)
                                    .new_windows_num_rows
                                    .value()
                            ),
                            ctx,
                        );
                    });
                }
                WindowSettingsChangedEvent::BackgroundOpacity { .. } => {
                    // Reset the slider state so that it uses the current opacity value on the next render.
                    me.opacity_state.reset_offset();
                }
                WindowSettingsChangedEvent::BackgroundBlurRadius { .. } => {
                    // Reset the slider state so that it uses the current opacity value on the next render.
                    me.blur_state.reset_offset();
                }
                WindowSettingsChangedEvent::ZoomLevel { .. } => {
                    let zoom_level = *WindowSettings::as_ref(ctx).zoom_level;

                    me.zoom_level_dropdown.update(ctx, |dropdown, ctx| {
                        dropdown.set_selected_by_action(
                            AppearancePageAction::SetZoomLevel(zoom_level),
                            ctx,
                        );
                    });

                    let zoom_factor = WindowSettings::as_ref(ctx).zoom_level.as_zoom_factor();
                    ctx.set_zoom_factor(zoom_factor);
                }
                _ => {}
            };
            ctx.notify();
        });
        ctx.subscribe_to_model(&TabSettings::handle(ctx), |me, _, event, ctx| {
            me.handle_tab_settings_event(event, ctx)
        });

        // we need to update the switch if the setting gets changed elsewhere, like command palette
        ctx.subscribe_to_model(&AppEditorSettings::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        let line_height_editor = Self::editor(
            |me, event, ctx| me.handle_line_editor_event(event, ctx),
            &format!("{line_height_ratio}"),
            ui_font_size,
            ctx,
        );

        let window_settings = WindowSettings::as_ref(ctx);
        let (num_columns, num_rows) = (
            *window_settings.new_windows_num_columns.value(),
            *window_settings.new_windows_num_rows.value(),
        );
        let new_window_columns_editor = Self::editor(
            |me, event, ctx| {
                me.update_new_windows_num_columns(
                    matches!(event, EditorEvent::Blurred | EditorEvent::Enter),
                    ctx,
                );
                if let EditorEvent::Escape = event {
                    ctx.emit(SettingsPageEvent::FocusModal);
                }
            },
            &format!("{num_columns}"),
            ui_font_size,
            ctx,
        );

        let new_window_rows_editor = Self::editor(
            |me, event, ctx| {
                me.update_new_windows_num_rows(
                    matches!(event, EditorEvent::Blurred | EditorEvent::Enter),
                    ctx,
                );
                if let EditorEvent::Escape = event {
                    ctx.emit(SettingsPageEvent::FocusModal);
                }
            },
            &format!("{num_rows}"),
            ui_font_size,
            ctx,
        );

        // Uncaged: system fonts are enumerated when a font dropdown is opened, not when
        // this page is constructed. Enumeration makes CoreText map every font file on the
        // machine (measured at ~1.17 GB of mmaps), and constructing this page is not a
        // signal anyone wants the font list -- a restored Settings tab constructs it at
        // startup, which is exactly how "on settings open" became "on every launch".
        // See `request_system_fonts_if_needed` and the dropdown subscriptions below.
        let font_family_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = FilterableDropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            // Initialize dropdown with the default font in case system fonts failed to load.
            dropdown.add_items(vec![Self::default_font_item(ctx, false)], ctx);
            dropdown.set_selected_by_index(0, ctx);
            dropdown
        });

        ctx.subscribe_to_view(&font_family_dropdown, |me, _, event, ctx| {
            if matches!(event, FilterableDropdownEvent::ToggleExpanded) {
                me.request_system_fonts_if_needed(ctx);
            }
        });

        let ai_font_family_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = FilterableDropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            // Initialize dropdown with the default font in case system fonts failed to load.
            dropdown.add_items(vec![Self::default_font_item(ctx, true)], ctx);
            dropdown.set_selected_by_index(0, ctx);
            dropdown
        });

        ctx.subscribe_to_view(&ai_font_family_dropdown, |me, _, event, ctx| {
            if matches!(event, FilterableDropdownEvent::ToggleExpanded) {
                me.request_system_fonts_if_needed(ctx);
            }
        });

        let font_weight_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            let selectable_weights = [Weight::Normal, Weight::Bold];
            let items = selectable_weights
                .iter()
                .map(|weight| {
                    DropdownItem::new(
                        weight.to_string(),
                        AppearancePageAction::SetFontWeight(*weight),
                    )
                })
                .collect();
            dropdown.add_items(items, ctx);
            dropdown.set_selected_by_name(monospace_font_weight.to_string(), ctx);
            dropdown
        });

        let thin_strokes_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            let values = vec![
                ThinStrokes::Never,
                ThinStrokes::OnLowDpiDisplays,
                ThinStrokes::OnHighDpiDisplays,
                ThinStrokes::Always,
            ];

            let current_value = ctx.rendering_config().glyphs.use_thin_strokes;
            let selected_index = values
                .iter()
                .position(|val| *val == current_value)
                .unwrap_or_else(|| {
                    log::error!("Could not find current ThinStrokes value in dropdown option list");
                    0
                });

            dropdown.add_items(
                values
                    .into_iter()
                    .map(|val| {
                        DropdownItem::new(
                            Self::thin_strokes_dropdown_item_label(val),
                            AppearancePageAction::SetThinStrokes(val),
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_by_index(selected_index, ctx);

            dropdown
        });

        let input_mode_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            let values = vec![
                InputMode::PinnedToBottom,
                InputMode::Waterfall,
                InputMode::PinnedToTop,
            ];
            let current_value = *InputModeSettings::as_ref(ctx).input_mode.value();
            let selected_index: usize = values
                .iter()
                .position(|val| *val == current_value)
                .unwrap_or_else(|| {
                    log::error!("Could not find current InputMode value in dropdown option list");
                    0
                });

            dropdown.add_items(
                values
                    .into_iter()
                    .map(|val| {
                        DropdownItem::new(
                            Self::input_mode_dropdown_item_label(val),
                            AppearancePageAction::SetInputMode {
                                new_mode: val,
                                from_binding: false,
                            },
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_by_index(selected_index, ctx);

            dropdown
        });

        let enforce_min_contrast_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            let values = vec![
                EnforceMinimumContrast::Always,
                EnforceMinimumContrast::OnlyNamedColors,
                EnforceMinimumContrast::Never,
            ];
            let current_value = *FontSettings::as_ref(ctx)
                .enforce_minimum_contrast;
            let selected_index = values.iter().position(|val| *val == current_value).unwrap_or_else(|| {
                log::error!("Could not find current EnforceMinimumContrast value in dropdown option list");
                0
            });

            dropdown.add_items(
                values.into_iter().map(|val| {
                    DropdownItem::new(
                        Self::enforce_minimum_contrast_dropdown_item_label(val),
                        AppearancePageAction::SetEnforceMinimumContrast(val),
                    )
                }).collect(),
                ctx,
            );
            dropdown.set_selected_by_index(selected_index, ctx);
            dropdown
        });

        let context_chips = Self::get_context_chip_renderers(ctx);

        let alt_screen_padding_editor = {
            let width_and_height_editor_options = SingleLineEditorOptions {
                text: TextOptions::ui_font_size(appearance_handle.as_ref(ctx)),
                ..Default::default()
            };
            ctx.add_typed_action_view(|ctx| {
                let mut editor =
                    EditorView::single_line(width_and_height_editor_options.clone(), ctx);
                if let AltScreenPaddingMode::Custom { uniform_padding } =
                    *TerminalSettings::as_ref(ctx).alt_screen_padding
                {
                    let val = format!("{:.1}", uniform_padding.as_f32());
                    // Do a system edit to avoid counting this update as part of telemetry.
                    editor.system_reset_buffer_text(val.trim_end_matches(".0"), ctx);
                }
                editor
            })
        };
        ctx.subscribe_to_view(&alt_screen_padding_editor, |me, _, event, ctx| {
            me.handle_alt_screen_padding_editor_event(event, ctx);
        });

        let light_theme_dropdown = Self::build_system_theme_dropdown(SystemTheme::Light, ctx);
        let dark_theme_dropdown = Self::build_system_theme_dropdown(SystemTheme::Dark, ctx);
        // A searchable dropdown shows whichever item the search or arrow keys highlighted, even
        // when it closes without a choice. Snap both back to the saved themes when they close, so
        // what they show is what's set. A real choice has already been saved by then.
        for dropdown in [&light_theme_dropdown, &dark_theme_dropdown] {
            ctx.subscribe_to_view(dropdown, |me, _, event, ctx| {
                if matches!(event, FilterableDropdownEvent::Close) {
                    me.refresh_system_theme_dropdowns(ctx);
                }
            });
        }

        let header_toolbar_inline_editor =
            ctx.add_typed_action_view(HeaderToolbarInlineEditor::new);

        AppearanceSettingsPageView {
            page: Self::build_page(ctx),
            window_id: ctx.window_id(),
            ai_font_family_dropdown,
            notebook_font_size_editor,
            font_size_editor,
            line_height_editor,
            new_window_columns_editor,
            valid_new_window_columns: true,
            new_window_rows_editor,
            valid_new_window_rows: true,
            opacity_state: Default::default(),
            blur_state: Default::default(),
            font_family_dropdown,
            font_weight_dropdown,
            thin_strokes_dropdown,
            input_mode_dropdown,
            light_theme_dropdown,
            dark_theme_dropdown,
            enforce_min_contrast_dropdown,
            workspace_decorations_dropdown: Self::build_workspace_decoration_visibility_dropdown(
                ctx,
            ),
            tab_close_button_position_dropdown: Self::build_tab_close_button_position_dropdown(ctx),
            zoom_level_dropdown: Self::build_zoom_level_dropdown(ctx),
            zoom_reset_button_mouse_state: MouseStateHandle::default(),
            available_families: Default::default(),
            system_fonts_requested: false,
            view_font_type: Default::default(),
            color_picker_dot_states: (0..directory_tab_colors(ctx).len())
                .map(|_| {
                    (0..TAB_COLOR_OPTIONS.len() + 1)
                        .map(|_| MouseStateHandle::default())
                        .collect()
                })
                .collect(),
            directory_tab_color_delete_buttons: build_directory_delete_buttons(ctx),
            header_toolbar_inline_editor,
            alt_screen_padding_editor,
            context_chips,
            ps1_grid_info: None,
        }
    }

    /// The page, card by card: what you look at most (theme, text) first, then the window, the
    /// tab bar, the input, the terminal content and the cursor, and the Dock last.
    fn build_page(ctx: &mut ViewContext<Self>) -> PageType<Self> {
        let mut categories = vec![Category::new(
            "Theme",
            vec![
                Box::new(CurrentThemeWidget::default()),
                Box::new(SyncThemeWithOsWidget::default()),
            ],
        )];

        let font_settings = FontSettings::as_ref(ctx);
        let mut text_widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = vec![
            Box::new(TerminalFontWidget::default()),
            Box::new(FontWeightWidget),
            Box::new(FontSizeWidget),
            Box::new(LineHeightWidget::default()),
            Box::new(AIFontWidget::default()),
            Box::new(NotebookFontSizeWidget::default()),
        ];
        let ligature_settings = LigatureSettings::as_ref(ctx);
        if FeatureFlag::Ligatures.is_enabled()
            && ligature_settings
                .ligature_rendering_enabled
                .is_supported_on_current_platform()
        {
            text_widgets.push(Box::new(LigaturesWidget::default()));
        }
        if font_settings
            .use_thin_strokes
            .is_supported_on_current_platform()
        {
            text_widgets.push(Box::new(ThinStrokesWidget::default()));
        }
        if font_settings
            .enforce_minimum_contrast
            .is_supported_on_current_platform()
        {
            text_widgets.push(Box::new(MinimumContrastWidget::default()));
        }
        categories.push(Category::new("Text", text_widgets));

        let window_settings = WindowSettings::as_ref(ctx);
        let mut window_widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = vec![];
        if window_settings
            .show_launch_animation
            .is_supported_on_current_platform()
        {
            window_widgets.push(Box::new(LaunchAnimationWidget::default()));
        }
        if window_settings
            .background_opacity
            .is_supported_on_current_platform()
        {
            window_widgets.push(Box::new(WindowOpacityWidget));
        }
        if window_settings
            .background_blur_radius
            .is_supported_on_current_platform()
        {
            window_widgets.push(Box::new(WindowBlurWidget));
        }
        if window_settings
            .background_blur_texture
            .is_supported_on_current_platform()
        {
            window_widgets.push(Box::new(WindowBlurTextureWidget::default()));
        }
        if window_settings
            .open_windows_at_custom_size
            .is_supported_on_current_platform()
        {
            window_widgets.push(Box::new(CustomWindowSizeWidget::default()));
        }
        if FeatureFlag::UIZoom.is_enabled() {
            window_widgets.push(Box::new(ZoomLevelWidget));
        }
        if window_settings
            .left_panel_visibility_across_tabs
            .is_supported_on_current_platform()
        {
            window_widgets.push(Box::new(ToolsPanelStateScopeWidget::default()));
        }
        if !window_widgets.is_empty() {
            categories.push(Category::new("Window", window_widgets));
        }

        let tab_settings = TabSettings::as_ref(ctx);
        let mut tab_widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = vec![];
        if FeatureFlag::FullScreenZenMode.is_enabled()
            && tab_settings
                .workspace_decoration_visibility
                .is_supported_on_current_platform()
        {
            tab_widgets.push(Box::new(ZenModeWidget::default()));
        }
        tab_widgets.push(Box::new(TabIndicatorWidget::default()));
        if !FeatureFlag::OpenWarpNewSettingsModes.is_enabled() {
            tab_widgets.push(Box::new(CodeReviewButtonWidget::default()));
        }
        if FeatureFlag::TabCloseButtonOnLeft.is_enabled() {
            tab_widgets.push(Box::new(TabCloseButtonPositionWidget::default()));
        }
        tab_widgets.push(Box::new(PreserveActiveTabColorWidget::default()));

        if FeatureFlag::VerticalTabs.is_enabled() {
            tab_widgets.push(Box::new(VerticalTabsWidget::default()));
            tab_widgets.push(Box::new(
                ShowVerticalTabPanelInRestoredWindowsWidget::default(),
            ));
            tab_widgets.push(Box::new(
                HideTitleBarSearchBarInVerticalTabsWidget::default(),
            ));
            tab_widgets.push(Box::new(
                UseLatestUserPromptAsConversationTitleInTabNamesWidget::default(),
            ));
            if FeatureFlag::ConfigurableToolbar.is_enabled() {
                tab_widgets.push(Box::new(EditToolbarWidget));
            }
        }

        if FeatureFlag::DirectoryTabColors.is_enabled() {
            let add_picker = ctx.add_typed_action_view(DirectoryColorAddPicker::new);
            ctx.subscribe_to_view(&add_picker, |me, _, event, ctx| {
                me.handle_directory_color_add_picker_event(event, ctx);
            });
            tab_widgets.push(Box::new(DirectoryTabColorsWidget { add_picker }));
        }
        categories.push(Category::new("Tabs", tab_widgets));

        // Uncaged: the onboarding "Customize UI" choices live on their own
        // dedicated top-level "Customize UI" settings page (see
        // customize_ui_page.rs), not buried among the Appearance sections.

        // The PromptWidget handles its own look for the PS1 and Uncaged input types.
        categories.push(Category::new(
            "Input",
            vec![
                Box::new(InputTypeWidget::default()),
                Box::new(PromptWidget::default()),
                Box::new(InputModeWidget::default()),
            ],
        ));

        let mut block_widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = vec![
            Box::new(CompactModeWidget::default()),
            Box::new(JumpToBottomOfBlockWidget::default()),
        ];
        if FeatureFlag::MinimalistUI.is_enabled() {
            block_widgets.push(Box::new(ShowBlockDividersWidget::default()));
        }
        block_widgets.push(Box::new(DimInactivePanesWidget::default()));
        block_widgets.push(Box::new(FocusFollowsMouseWidget::default()));
        block_widgets.push(Box::new(AltScreenPaddingWidget::default()));
        categories.push(Category::new("Blocks & panes", block_widgets));

        categories.push(Category::new(
            "Cursor",
            vec![
                Box::new(CursorTypeWidget::default()),
                Box::new(BlinkingCursorWidget::default()),
            ],
        ));

        if AppIconSettings::as_ref(ctx).is_supported_on_current_platform() {
            categories.push(Category::new(
                "Dock",
                vec![Box::new(CustomAppIconWidget::default())],
            ));
        }

        PageType::new_categorized(categories, None)
    }

    /// A light or dark theme picker for "Sync with OS".
    fn build_system_theme_dropdown(
        appearance: SystemTheme,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<FilterableDropdown<AppearancePageAction>> {
        // Searchable, like the font pickers: with every built-in theme plus downloads and the
        // user's own, the list is long, and the plain dropdown couldn't scroll to its end — so the
        // user's themes, which sort last, were out of reach.
        ctx.add_typed_action_view(|ctx| {
            let mut dropdown = FilterableDropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);
            let selected = Self::system_theme_for(appearance, ctx);
            dropdown.set_items(Self::system_theme_items(appearance, &selected, ctx), ctx);
            dropdown.set_selected_by_action(
                AppearancePageAction::SetSystemTheme {
                    appearance,
                    theme: selected,
                },
                ctx,
            );
            dropdown
        })
    }

    /// The theme set for `appearance` under "Sync with OS".
    fn system_theme_for(appearance: SystemTheme, app: &AppContext) -> ThemeKind {
        let selected = ThemeSettings::as_ref(app).selected_system_themes.value();
        match appearance {
            SystemTheme::Light => selected.light.clone(),
            SystemTheme::Dark => selected.dark.clone(),
        }
    }

    /// Every installed theme, in the gallery's order: the ones that ship, then downloads, then
    /// the user's own, each alphabetically. The referral reward themes are left out — Uncaged has
    /// no referrals — unless one is somehow already the selection.
    fn system_theme_items(
        appearance: SystemTheme,
        selected: &ThemeKind,
        app: &AppContext,
    ) -> Vec<DropdownItem<AppearancePageAction>> {
        let group_order = |group: ThemeGroup| match group {
            ThemeGroup::System => 0,
            ThemeGroup::Community => 1,
            ThemeGroup::Mine => 2,
        };
        let mut themes: Vec<(u8, String, ThemeKind)> = WarpConfig::as_ref(app)
            .theme_config()
            .theme_items()
            .filter(|(kind, _)| {
                !matches!(
                    kind,
                    ThemeKind::SentReferralReward | ThemeKind::ReceivedReferralReward
                ) || *kind == selected
            })
            .map(|(kind, _)| (group_order(kind.group()), kind.to_string(), kind.clone()))
            .collect();
        themes.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        themes
            .into_iter()
            .map(|(_, name, theme)| {
                DropdownItem::new(
                    name,
                    AppearancePageAction::SetSystemTheme { appearance, theme },
                )
            })
            .collect()
    }

    /// Brings the light and dark theme pickers in line with the installed themes and the
    /// current selection.
    fn refresh_system_theme_dropdowns(&mut self, ctx: &mut ViewContext<Self>) {
        for (appearance, dropdown) in [
            (SystemTheme::Light, self.light_theme_dropdown.clone()),
            (SystemTheme::Dark, self.dark_theme_dropdown.clone()),
        ] {
            dropdown.update(ctx, |dropdown, ctx| {
                let selected = Self::system_theme_for(appearance, ctx);
                dropdown.set_items(Self::system_theme_items(appearance, &selected, ctx), ctx);
                dropdown.set_selected_by_action(
                    AppearancePageAction::SetSystemTheme {
                        appearance,
                        theme: selected,
                    },
                    ctx,
                );
            });
        }
    }

    /// Sets the theme for one system appearance. The AppearanceManager follows the theme
    /// settings, so if the system is in that appearance now the window changes straight away.
    fn set_system_theme(
        &mut self,
        appearance: SystemTheme,
        theme: ThemeKind,
        ctx: &mut ViewContext<Self>,
    ) {
        let current = ThemeSettings::as_ref(ctx)
            .selected_system_themes
            .value()
            .clone();
        let updated = match appearance {
            SystemTheme::Light => SelectedSystemThemes {
                light: theme,
                dark: current.dark,
            },
            SystemTheme::Dark => SelectedSystemThemes {
                light: current.light,
                dark: theme,
            },
        };
        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| {
            report_if_error!(theme_settings
                .selected_system_themes
                .set_value(updated, ctx));
        });
        ctx.notify();
    }

    fn set_alt_screen_padding_editor_text(&mut self, ctx: &mut ViewContext<Self>) {
        if let AltScreenPaddingMode::Custom { uniform_padding } =
            *TerminalSettings::as_ref(ctx).alt_screen_padding
        {
            self.alt_screen_padding_editor.update(ctx, |editor, ctx| {
                let val = format!("{:.1}", uniform_padding.as_f32());
                editor.set_buffer_text(val.trim_end_matches(".0"), ctx);
            });
            ctx.notify();
        }
    }

    fn handle_appearance_update(
        &mut self,
        handle: ModelHandle<Appearance>,
        event: &AppearanceEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            AppearanceEvent::MonospaceFontFamilyChanged { .. } => {
                self.update_font_dropdown(ctx);
            }
            AppearanceEvent::MonospaceFontSizeChanged { .. } => {
                let font_size = handle.as_ref(ctx).monospace_font_size();
                self.font_size_editor.update(ctx, move |editor, ctx| {
                    editor.set_buffer_text(&format!("{font_size}"), ctx);
                });
            }
            AppearanceEvent::MonospaceFontWeightChanged { .. } => {
                let font_weight = handle.as_ref(ctx).monospace_font_weight();
                self.font_weight_dropdown.update(ctx, |dropdown, ctx| {
                    dropdown.set_selected_by_name(font_weight.to_string(), ctx);
                });
            }
            AppearanceEvent::LineHeightRatioChanged { .. } => {
                let line_height_ratio = handle.as_ref(ctx).line_height_ratio();
                self.line_height_editor.update(ctx, move |editor, ctx| {
                    editor.set_buffer_text(&format!("{line_height_ratio}"), ctx);
                });
            }
            AppearanceEvent::ThemeChanged => {
                // Context-chip colors are theme-derived, so rebuild the Input
                // preview chips when the theme changes to keep them in sync.
                self.context_chips = Self::get_context_chip_renderers(ctx);
            }
            _ => {}
        }

        ctx.notify();
    }

    fn get_context_chip_renderers(app: &AppContext) -> Vec<ContextChipRenderer> {
        let appearance = Appearance::as_ref(app);
        let prompt = Prompt::as_ref(app);
        prompt
            .chip_kinds()
            .into_iter()
            .filter_map(|kind| {
                ContextChipRenderer::default_from_kind(kind, ChipAvailability::Enabled, appearance)
            })
            .collect()
    }

    fn handle_prompt_update(
        &mut self,
        _prompt: ModelHandle<Prompt>,
        _event: &PromptEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        self.context_chips = Self::get_context_chip_renderers(ctx);
    }

    fn default_font_item<V>(
        ctx: &mut ViewContext<V>,
        is_ai_font: bool,
    ) -> DropdownItem<AppearancePageAction>
    where
        V: View,
    {
        let font_name = if is_ai_font {
            AIFontName::default_value()
        } else {
            MonospaceFontName::default_value()
        };
        let mut initial_dropdown_item = DropdownItem::new(
            default_font_label(is_ai_font),
            if is_ai_font {
                AppearancePageAction::SetAIFontFamily(font_name.clone())
            } else {
                AppearancePageAction::SetFontFamily(font_name.clone())
            },
        );

        // If we're on a non-Linux platform, render the dropdown item in the
        // actual font.  We currently don't do this on Linux because
        // pre-loading all of the fonts is too expensive.
        if cfg!(not(any(target_os = "linux", target_os = "freebsd"))) {
            if let Some(family_id) = ctx.font_cache().family_id_for_name(&font_name) {
                initial_dropdown_item = initial_dropdown_item.with_font_override(family_id);
            }
        }

        initial_dropdown_item
    }

    fn input_mode_dropdown_item_label(val: InputMode) -> &'static str {
        match val {
            InputMode::PinnedToBottom => "Pin to the bottom (Uncaged mode)",
            InputMode::PinnedToTop => "Pin to the top (Reverse mode)",
            InputMode::Waterfall => "Start at the top (Classic mode)",
        }
    }

    fn thin_strokes_dropdown_item_label(val: ThinStrokes) -> &'static str {
        match val {
            ThinStrokes::Never => "Never",
            ThinStrokes::OnLowDpiDisplays => "On low-DPI displays",
            ThinStrokes::OnHighDpiDisplays => "On high-DPI displays",
            ThinStrokes::Always => "Always",
        }
    }

    fn enforce_minimum_contrast_dropdown_item_label(val: EnforceMinimumContrast) -> &'static str {
        match val {
            EnforceMinimumContrast::Always => "Always",
            EnforceMinimumContrast::OnlyNamedColors => "Only for named colors",
            EnforceMinimumContrast::Never => "Never",
        }
    }

    fn workspace_decoration_visibility_dropdown_item_label(
        value: WorkspaceDecorationVisibility,
    ) -> &'static str {
        match value {
            WorkspaceDecorationVisibility::AlwaysShow => "Always",
            WorkspaceDecorationVisibility::HideFullscreen => "When windowed",
            WorkspaceDecorationVisibility::OnHover => "Only on hover",
        }
    }

    fn tab_close_button_position_dropdown_item_label(
        value: TabCloseButtonPosition,
    ) -> &'static str {
        match value {
            TabCloseButtonPosition::Right => "Right",
            TabCloseButtonPosition::Left => "Left",
        }
    }

    fn handle_alt_screen_padding_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Edited(EditOrigin::UserTyped | EditOrigin::UserInitiated) => {
                let buffer_text = self.alt_screen_padding_editor.as_ref(ctx).buffer_text(ctx);

                if let Ok(padding) = buffer_text.parse::<f32>() {
                    if padding >= 0. {
                        TerminalSettings::handle(ctx).update(ctx, |terminal_settings, ctx| {
                            let new_mode = AltScreenPaddingMode::Custom {
                                uniform_padding: padding.into_pixels(),
                            };
                            report_if_error!(terminal_settings
                                .alt_screen_padding
                                .set_value(new_mode, ctx));
                            send_telemetry_from_ctx!(
                                TelemetryEvent::UpdateAltScreenPaddingMode { new_mode },
                                ctx
                            );
                        });
                    }
                }

                ctx.notify();
            }
            EditorEvent::Escape | EditorEvent::Blurred | EditorEvent::Enter => {
                self.set_alt_screen_padding_editor_text(ctx);
                ctx.emit(SettingsPageEvent::FocusModal)
            }
            _ => {}
        }
    }

    pub fn handle_font_size_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Blurred | EditorEvent::Enter => self.set_font_size(ctx),
            EditorEvent::Escape => ctx.emit(SettingsPageEvent::FocusModal),
            _ => {}
        }
    }

    pub fn handle_notebook_font_size_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Blurred | EditorEvent::Enter => self.set_notebook_font_size(ctx),
            EditorEvent::Escape => ctx.emit(SettingsPageEvent::FocusModal),
            _ => {}
        }
    }

    pub fn handle_line_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Blurred | EditorEvent::Enter => self.set_line_height_ratio(ctx),
            EditorEvent::Escape => ctx.emit(SettingsPageEvent::FocusModal),
            _ => {}
        }
    }

    /// Updates the prompt that is shown when PS1 is selected, in the 'Prompt' section.
    pub(super) fn set_ps1_info(
        &mut self,
        ps1_grid_info: Option<(BlockGrid, SizeInfo)>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.ps1_grid_info = ps1_grid_info;
        ctx.notify();
    }

    pub(super) fn get_ps1_info(&self) -> Option<&(BlockGrid, SizeInfo)> {
        self.ps1_grid_info.as_ref()
    }

    fn update_new_windows_num_columns(&mut self, blurred: bool, ctx: &mut ViewContext<Self>) {
        let user_input = self.new_window_columns_editor.as_ref(ctx).buffer_text(ctx);
        if let Some(columns) = Self::parse_new_window_size(user_input) {
            self.valid_new_window_columns = true;
            if blurred {
                self.set_new_windows_num_columns(columns, ctx);
            }
        } else {
            self.valid_new_window_columns = false;
            if blurred {
                let window_settings: &WindowSettings = WindowSettings::as_ref(ctx);
                // Revert to the saved value
                self.set_new_windows_num_columns(
                    *window_settings.new_windows_num_columns.value(),
                    ctx,
                );
            }
        }
        if blurred {
            ctx.focus_self();
        }
        ctx.notify();
    }

    fn update_new_windows_num_rows(&mut self, blurred: bool, ctx: &mut ViewContext<Self>) {
        let user_input = self.new_window_rows_editor.as_ref(ctx).buffer_text(ctx);
        if let Some(rows) = Self::parse_new_window_size(user_input) {
            self.valid_new_window_rows = true;
            if blurred {
                self.set_new_windows_num_rows(rows, ctx);
            }
        } else {
            self.valid_new_window_rows = false;
            if blurred {
                let window_settings: &WindowSettings = WindowSettings::as_ref(ctx);
                // Revert to the saved value
                self.set_new_windows_num_columns(
                    *window_settings.new_windows_num_columns.value(),
                    ctx,
                );
            }
        }
        if blurred {
            ctx.focus_self();
        }
        ctx.notify();
    }

    fn parse_new_window_size(user_input: String) -> Option<u16> {
        user_input.parse::<u16>().ok().filter(|parsed| {
            (MIN_NEW_WINDOW_ROWS_OR_COLS..=MAX_NEW_WINDOW_ROWS_OR_COLS).contains(parsed)
        })
    }

    fn set_font_size(&mut self, ctx: &mut ViewContext<Self>) {
        let user_input = self.font_size_editor.as_ref(ctx).buffer_text(ctx);
        if let Ok(num) = user_input.parse::<usize>() {
            if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&num) {
                FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
                    report_if_error!(font_settings
                        .monospace_font_size
                        .set_value(num as f32, ctx,));
                });
            }
        }
    }

    pub fn set_font_weight(&mut self, value: Weight, ctx: &mut ViewContext<Self>) {
        FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
            report_if_error!(font_settings.monospace_font_weight.set_value(value, ctx))
        });
    }

    fn set_notebook_font_size(&mut self, ctx: &mut ViewContext<Self>) {
        let user_input = self.notebook_font_size_editor.as_ref(ctx).buffer_text(ctx);
        if let Ok(num) = user_input.parse::<usize>() {
            if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&num) {
                FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
                    report_if_error!(font_settings.notebook_font_size.set_value(num as f32, ctx,));
                });
            }
        }
    }

    fn set_opacity(
        &mut self,
        opacity_value: f32,
        should_set_defaults: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        if should_set_defaults {
            send_telemetry_from_ctx!(
                TelemetryEvent::SetOpacity {
                    opacity: opacity_value as u8
                },
                ctx
            );
        }
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            report_if_error!(window_settings
                .background_opacity
                .set_value(opacity_value as u8, ctx));
        });
        ctx.notify();
    }

    fn set_blur(
        &mut self,
        blur_value: f32,
        should_set_defaults: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        if should_set_defaults {
            send_telemetry_from_ctx!(
                TelemetryEvent::SetBlurRadius {
                    blur_radius: blur_value as u8
                },
                ctx
            );
        }

        ctx.windows()
            .set_all_windows_background_blur_radius(blur_value as u8);

        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            report_if_error!(window_settings
                .background_blur_radius
                .set_value(blur_value as u8, ctx));
        });
        ctx.notify()
    }

    fn reset_line_height_ratio(&mut self, ctx: &mut ViewContext<Self>) {
        self.line_height_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(&format!("{DEFAULT_UI_LINE_HEIGHT_RATIO}"), ctx);
        });

        send_telemetry_from_ctx!(
            TelemetryEvent::SetLineHeight {
                new_value: DEFAULT_UI_LINE_HEIGHT_RATIO
            },
            ctx
        );

        FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
            report_if_error!(font_settings
                .line_height_ratio
                .set_value(DEFAULT_UI_LINE_HEIGHT_RATIO, ctx));
        });
    }

    fn set_line_height_ratio(&mut self, ctx: &mut ViewContext<Self>) {
        let user_input = self.line_height_editor.as_ref(ctx).buffer_text(ctx);
        let Ok(new_line_height) = user_input.parse::<f32>() else {
            return;
        };

        let appearance = Appearance::as_ref(ctx);
        let current_line_height = appearance.ui_builder().line_height_ratio();

        if (current_line_height - new_line_height).abs() > f32::EPSILON {
            send_telemetry_from_ctx!(
                TelemetryEvent::SetLineHeight {
                    new_value: new_line_height
                },
                ctx
            );

            if (MIN_LINE_SPACING..=MAX_LINE_SPACING).contains(&new_line_height) {
                FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
                    report_if_error!(font_settings
                        .line_height_ratio
                        .set_value(new_line_height, ctx));
                });
            }
        }
    }

    pub fn toggle_open_windows_at_custom_size(&mut self, ctx: &mut ViewContext<Self>) {
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            let current_val = window_settings.open_windows_at_custom_size.value();
            let new_val: bool = !current_val;
            send_telemetry_from_ctx!(
                TelemetryEvent::ToggleNewWindowsAtCustomSize { enabled: new_val },
                ctx
            );
            report_if_error!(window_settings
                .open_windows_at_custom_size
                .set_value(new_val, ctx));
        });
        ctx.notify();
    }

    fn set_new_windows_num_columns(&mut self, columns: u16, ctx: &mut ViewContext<Self>) {
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            send_telemetry_from_ctx!(TelemetryEvent::SetNewWindowsAtCustomSize, ctx);
            report_if_error!(window_settings
                .new_windows_num_columns
                .set_value(columns, ctx));
        });
    }

    fn set_new_windows_num_rows(&mut self, rows: u16, ctx: &mut ViewContext<Self>) {
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            send_telemetry_from_ctx!(TelemetryEvent::SetNewWindowsAtCustomSize, ctx);
            report_if_error!(window_settings.new_windows_num_rows.set_value(rows, ctx));
        });
    }

    fn update_font_dropdown(&mut self, ctx: &mut ViewContext<Self>) {
        let monospace_font_family = Appearance::as_ref(ctx).monospace_font_family();
        let ai_font_family = Appearance::as_ref(ctx).ai_font_family();

        self.font_family_dropdown.update(ctx, |dropdown, ctx| {
            // Get the family name of the current monospace font.
            // We check the font_cache for the current monospace family.
            // We also make sure that
            // - If the current monospace family is in our available_families map,
            //   we update its entry to ensure it has the correct family_id
            // - Otherwise, we add a new entry for the current monospace family
            let font_name = ctx
                .font_cache()
                .load_family_name_from_id(monospace_font_family);

            if let Some(font_name) = &font_name {
                self.available_families
                    .entry(font_name.clone())
                    .and_modify(|entry| entry.0 = Some(monospace_font_family))
                    .or_insert((Some(monospace_font_family), FontType::Monospace));
            }
            let font_name = font_name.unwrap_or_default();

            let mut items = self
                .available_families
                .iter()
                .filter_map(|(name, (family, font_type))| {
                    // Only add the item if it's not a default font and then check whether we want
                    // to see it depending on it being monospace and user's preference. We also
                    // want to include the currently selected font, even if it's non-monospace and
                    // user hasn't chosen to view all system fonts.
                    let include_in_dropdown = name != &MonospaceFontName::default_value()
                        && (matches!(self.view_font_type, FontType::Any)
                            || matches!(font_type, FontType::Monospace)
                            || *name == font_name);
                    if include_in_dropdown {
                        let name_move = name.clone();
                        let mut dropdown =
                            DropdownItem::new(name, AppearancePageAction::SetFontFamily(name_move));

                        // If we're on a non-Linux platform, render the dropdown item in the
                        // actual font.  We currently don't do this on Linux because
                        // pre-loading all of the fonts is too expensive.
                        if cfg!(not(any(target_os = "linux", target_os = "freebsd"))) {
                            if let Some(family_id) = family {
                                dropdown = dropdown.with_font_override(*family_id)
                            }
                        }

                        Some(dropdown)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            // Sort the font names by alphabetical order.
            items.sort_by(|a, b| a.display_text.cmp(&b.display_text));
            // Prepend the default item
            items.insert(0, Self::default_font_item(ctx, false));
            dropdown.set_items(items, ctx);

            if !font_name.is_empty() {
                let label = if font_name == MonospaceFontName::default_value() {
                    &default_font_label(false)
                } else {
                    &font_name
                };
                dropdown.set_selected_by_name(label, ctx);
            }
        });

        self.ai_font_family_dropdown.update(ctx, |dropdown, ctx| {
            // Get the family name of the current agent mode font.
            // We check the font_cache for the current agent mode family.
            // We also make sure that
            // - If the current family is in our available_families map,
            //   we update its entry to ensure it has the correct family_id
            // - Otherwise, we add a new entry for the current agent mode family
            let font_name = ctx.font_cache().load_family_name_from_id(ai_font_family);

            if let Some(font_name) = &font_name {
                self.available_families
                    .entry(font_name.clone())
                    .and_modify(|entry| entry.0 = Some(ai_font_family))
                    .or_insert((Some(ai_font_family), FontType::Any));
            }
            let font_name = font_name.unwrap_or_default();

            let mut items = self
                .available_families
                .iter()
                .filter_map(|(name, (family, _font_type))| {
                    if name == &AIFontName::default_value() {
                        return None;
                    }

                    let name_move = name.clone();
                    let mut dropdown =
                        DropdownItem::new(name, AppearancePageAction::SetAIFontFamily(name_move));

                    // If we're on a non-Linux platform, render the dropdown item in the
                    // actual font.  We currently don't do this on Linux because
                    // pre-loading all of the fonts is too expensive.
                    if cfg!(not(any(target_os = "linux", target_os = "freebsd"))) {
                        if let Some(family_id) = family {
                            dropdown = dropdown.with_font_override(*family_id)
                        }
                    }

                    Some(dropdown)
                })
                .collect::<Vec<_>>();

            // Sort the font names by alphabetical order.
            items.sort_by(|a, b| a.display_text.cmp(&b.display_text));
            // Prepend the default item
            items.insert(0, Self::default_font_item(ctx, true));
            dropdown.set_items(items, ctx);

            if !font_name.is_empty() {
                let label = if font_name == AIFontName::default_value() {
                    &default_font_label(true)
                } else {
                    &font_name
                };
                dropdown.set_selected_by_name(label, ctx);
            }
        });

        ctx.notify();
    }

    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    /// Kicks off system-font enumeration the first time a font dropdown is opened.
    ///
    /// The fonts::Cache memoises the result, so repeated opens after the first are a
    /// cheap clone; the guard here only avoids re-spawning while the first load is still
    /// in flight.
    fn request_system_fonts_if_needed(&mut self, ctx: &mut ViewContext<Self>) {
        #[cfg(not(target_family = "wasm"))]
        {
            if self.system_fonts_requested || ChannelState::channel() == Channel::Integration {
                return;
            }
            self.system_fonts_requested = true;
            let all_system_fonts = warpui::fonts::Cache::handle(ctx)
                .update(ctx, |font_cache, ctx| font_cache.all_system_fonts(ctx));
            ctx.spawn(all_system_fonts, Self::set_system_fonts);
        }
    }

    pub fn set_system_fonts(
        &mut self,
        available_families: Vec<(Option<FamilyId>, FontInfo)>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.available_families = available_families
            .into_iter()
            .map(|(family_id, font_info)| {
                let font_type = if font_info.is_monospace {
                    FontType::Monospace
                } else {
                    FontType::Any
                };
                (font_info.family_name, (family_id, font_type))
            })
            .collect();

        // Add Hack font to the available monospace families so user could switch back to it.
        if let Some(family_id) = ctx
            .font_cache()
            .family_id_for_name(DEFAULT_MONOSPACE_FONT_NAME)
        {
            self.available_families.insert(
                String::from(DEFAULT_MONOSPACE_FONT_NAME),
                (Some(family_id), FontType::Monospace),
            );
        }

        self.update_font_dropdown(ctx);
    }

    pub fn set_font_family(&mut self, name: &str, ctx: &mut ViewContext<Self>) {
        FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
            report_if_error!(font_settings
                .monospace_font_name
                .set_value(name.to_string(), ctx));
            if *font_settings.match_ai_font_to_terminal_font.value() {
                report_if_error!(font_settings.ai_font_name.set_value(name.to_string(), ctx))
            }
        });
    }

    pub fn toggle_match_ai_font_to_terminal_font(&mut self, ctx: &mut ViewContext<Self>) {
        FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
            report_if_error!(font_settings
                .match_ai_font_to_terminal_font
                .toggle_and_save_value(ctx));
            if *font_settings.match_ai_font_to_terminal_font.value() {
                let font_name = font_settings.monospace_font_name.value().clone();
                self.ai_font_family_dropdown.update(ctx, |dropdown, ctx| {
                    dropdown.clear_filter(ctx);
                });
                report_if_error!(font_settings.ai_font_name.set_value(font_name, ctx))
            }
        });
    }

    pub fn set_ai_font_family(&mut self, name: &str, ctx: &mut ViewContext<Self>) {
        FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
            report_if_error!(font_settings.ai_font_name.set_value(name.to_string(), ctx))
        });
    }

    fn set_thin_strokes(&mut self, value: &ThinStrokes, ctx: &mut ViewContext<Self>) {
        FontSettings::handle(ctx).update(ctx, |font_settings, ctx| {
            match font_settings.use_thin_strokes.set_value(*value, ctx) {
                Ok(_) => {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::ThinStrokesSettingChanged { new_value: *value },
                        ctx
                    );
                }
                Err(e) => {
                    report_error!(e);
                }
            }
        });
    }

    pub fn toggle_jump_to_bottom_of_block_button(&mut self, ctx: &mut ViewContext<Self>) {
        let block_list_settings = BlockListSettings::handle(ctx);
        let new_value = {
            !*block_list_settings
                .as_ref(ctx)
                .show_jump_to_bottom_of_block_button
                .value()
        };
        send_telemetry_from_ctx!(
            TelemetryEvent::ToggleJumpToBottomofBlockButton { enabled: new_value },
            ctx
        );
        ctx.update_model(&block_list_settings, move |block_list_settings, ctx| {
            report_if_error!(block_list_settings
                .show_jump_to_bottom_of_block_button
                .set_value(new_value, ctx));
        });
    }

    pub fn toggle_show_block_dividers(&mut self, ctx: &mut ViewContext<Self>) {
        let block_list_settings = BlockListSettings::handle(ctx);
        let new_value = { !*block_list_settings.as_ref(ctx).show_block_dividers.value() };
        send_telemetry_from_ctx!(
            TelemetryEvent::ToggleShowBlockDividers { enabled: new_value },
            ctx
        );
        ctx.update_model(&block_list_settings, move |block_list_settings, ctx| {
            report_if_error!(block_list_settings
                .show_block_dividers
                .set_value(new_value, ctx));
        });
    }

    pub fn toggle_launch_animation(&mut self, ctx: &mut ViewContext<Self>) {
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            let shown = *window_settings.show_launch_animation.value();
            report_if_error!(window_settings.show_launch_animation.set_value(!shown, ctx));
        });
    }

    pub fn toggle_compact_mode(&mut self, ctx: &mut ViewContext<Self>) {
        TerminalSettings::handle(ctx).update(ctx, |terminal_settings, ctx| {
            let current_value = *terminal_settings.spacing_mode.value();
            report_if_error!(terminal_settings
                .spacing_mode
                .set_value(current_value.other_mode(), ctx));
        });
    }

    pub fn toggle_cursor_blink(&mut self, ctx: &mut ViewContext<Self>) {
        AppEditorSettings::handle(ctx).update(ctx, |me, ctx| {
            me.toggle_cursor_blink(ctx);
        })
    }

    pub fn toggle_respect_system_theme(&mut self, ctx: &mut ViewContext<Self>) {
        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| {
            report_if_error!(theme_settings.use_system_theme.toggle_and_save_value(ctx));
        });
        ctx.notify();
    }

    pub fn toggle_dim_inactive_panes(&mut self, ctx: &mut ViewContext<Self>) {
        PaneSettings::handle(ctx).update(ctx, |pane_settings, ctx| {
            match pane_settings
                .should_dim_inactive_panes
                .toggle_and_save_value(ctx)
            {
                Ok(new_value) => {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::ToggleDimInactivePanes { enabled: new_value },
                        ctx
                    );
                }
                Err(e) => {
                    report_error!(e);
                }
            }
        });
    }

    pub fn toggle_blur_texture(&mut self, ctx: &mut ViewContext<Self>) {
        let blur_enabled = WindowSettings::handle(ctx).read(ctx, |window_settings, _ctx| {
            *window_settings.background_blur_texture.value()
        });
        ctx.windows()
            .set_all_windows_background_blur_texture(!blur_enabled);
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            report_if_error!(window_settings
                .background_blur_texture
                .toggle_and_save_value(ctx));
        });
        ctx.notify();
    }

    pub fn toggle_left_panel_visibility(&mut self, ctx: &mut ViewContext<Self>) {
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            report_if_error!(window_settings
                .left_panel_visibility_across_tabs
                .toggle_and_save_value(ctx));
        });
        ctx.notify();
    }

    pub fn set_input_mode(
        &mut self,
        new_mode: InputMode,
        from_binding: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let old_mode = *InputModeSettings::as_ref(ctx).input_mode.value();
        send_telemetry_from_ctx!(TelemetryEvent::InputModeChanged { old_mode, new_mode }, ctx);
        InputModeSettings::handle(ctx).update(ctx, |input_mode, ctx| {
            report_if_error!(input_mode.input_mode.set_value(new_mode, ctx));
        });
        let item_name = Self::input_mode_dropdown_item_label(new_mode);

        if from_binding {
            // If this update is from a command palette action, we need to update the dropdown
            // If not, we can't update it because there is a circular view reference, but the dropdown
            // will update it itself.  Not great state management - I think ideally the dropdowns would have
            // a model they are listening to.
            self.input_mode_dropdown.update(ctx, |input_dropdown, ctx| {
                input_dropdown.set_selected_by_name(item_name, ctx);
                ctx.notify();
            });
        }
    }

    fn set_input_type(&mut self, new_type: InputBoxType, ctx: &mut ViewContext<Self>) {
        let old_type = InputSettings::as_ref(ctx).input_type(ctx);

        if old_type != new_type {
            InputSettings::handle(ctx).update(ctx, |input_type_settings, ctx| {
                report_if_error!(input_type_settings.input_box_type.set_value(new_type, ctx));
            });

            let is_udi_enabled = new_type == InputBoxType::Universal;
            send_telemetry_from_ctx!(
                TelemetryEvent::InputUXModeChanged {
                    is_udi_enabled,
                    origin: InputUXChangeOrigin::Settings
                },
                ctx
            );

            // Selecting classic mode must also enable honor_ps1 so the mode takes
            // effect immediately (input_type() requires honor_ps1 to return classic).
            SessionSettings::handle(ctx).update(ctx, |session_settings, ctx| {
                report_if_error!(session_settings
                    .honor_ps1
                    .set_value(new_type == InputBoxType::Classic, ctx));
            });

            ctx.notify();
        }
    }

    fn toggle_show_dock_icon(&mut self, ctx: &mut ViewContext<Self>) {
        AppIconSettings::handle(ctx).update(ctx, |app_icon_settings, ctx| {
            report_if_error!(app_icon_settings.show_dock_icon.toggle_and_save_value(ctx));
        });
    }

    fn set_cursor_type(&mut self, new_cursor_type: CursorDisplayType, ctx: &mut ViewContext<Self>) {
        AppEditorSettings::handle(ctx).update(ctx, |app_editor_settings, ctx| {
            report_if_error!(app_editor_settings
                .cursor_display_type
                .set_value(new_cursor_type, ctx));
            send_telemetry_from_ctx!(
                TelemetryEvent::CursorDisplayType {
                    cursor: new_cursor_type.to_string(),
                },
                ctx
            );
        });
    }

    fn toggle_all_available_fonts(&mut self, ctx: &mut ViewContext<Self>) {
        self.view_font_type = self.view_font_type.toggle();
        self.update_font_dropdown(ctx);
    }

    fn toggle_tab_indicators(&mut self, ctx: &mut ViewContext<Self>) {
        let tab_settings = TabSettings::handle(ctx);
        let new_value = { !*tab_settings.as_ref(ctx).show_indicators.value() };

        ctx.update_model(&tab_settings, move |tab_settings, ctx| {
            report_if_error!(tab_settings.show_indicators.set_value(new_value, ctx));
        });

        send_telemetry_from_ctx!(
            TelemetryEvent::ToggleTabIndicators { enabled: new_value },
            ctx
        );
    }

    fn toggle_show_code_review_button(&mut self, ctx: &mut ViewContext<Self>) {
        let tab_settings = TabSettings::handle(ctx);
        let new_value = !*tab_settings.as_ref(ctx).show_code_review_button.value();

        ctx.update_model(&tab_settings, move |tab_settings, ctx| {
            report_if_error!(tab_settings
                .show_code_review_button
                .set_value(new_value, ctx));
        });
    }

    fn toggle_preserve_active_tab_color(&mut self, ctx: &mut ViewContext<Self>) {
        let tab_settings = TabSettings::handle(ctx);
        let new_value = !*tab_settings.as_ref(ctx).preserve_active_tab_color.value();

        ctx.update_model(&tab_settings, move |tab_settings, ctx| {
            report_if_error!(tab_settings
                .preserve_active_tab_color
                .set_value(new_value, ctx));
        });

        send_telemetry_from_ctx!(
            TelemetryEvent::TogglePreserveActiveTabColor { enabled: new_value },
            ctx
        );
    }

    fn toggle_vertical_tabs(&mut self, ctx: &mut ViewContext<Self>) {
        let tab_settings = TabSettings::handle(ctx);
        let new_value = !*tab_settings.as_ref(ctx).use_vertical_tabs.value();

        ctx.update_model(&tab_settings, move |tab_settings, ctx| {
            report_if_error!(tab_settings.use_vertical_tabs.set_value(new_value, ctx));
        });
    }

    fn toggle_show_vertical_tab_panel_in_restored_windows(&mut self, ctx: &mut ViewContext<Self>) {
        TabSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(settings
                .show_vertical_tab_panel_in_restored_windows
                .toggle_and_save_value(ctx));
        });
    }

    fn toggle_hide_title_bar_search_bar_in_vertical_tabs(&mut self, ctx: &mut ViewContext<Self>) {
        TabSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(settings
                .hide_title_bar_search_bar_in_vertical_tabs
                .toggle_and_save_value(ctx));
        });
    }

    fn toggle_use_latest_user_prompt_as_conversation_title_in_tab_names(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        TabSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(settings
                .use_latest_user_prompt_as_conversation_title_in_tab_names
                .toggle_and_save_value(ctx));
        });
    }

    /// Set the workspace decoration visibility to a particular value.
    fn set_workspace_decoration_visibility(
        &mut self,
        new_value: WorkspaceDecorationVisibility,
        ctx: &mut ViewContext<Self>,
    ) {
        let previous_value = TabSettings::handle(ctx).update(ctx, |tab_settings, ctx| {
            let prev_value = *tab_settings.workspace_decoration_visibility.value();
            report_if_error!(tab_settings
                .workspace_decoration_visibility
                .set_value(new_value, ctx));
            prev_value
        });
        send_telemetry_from_ctx!(
            TelemetryEvent::ToggleWorkspaceDecorationVisibility {
                previous_value,
                new_value
            },
            ctx
        );
    }

    /// Toggle among the supported workspace decoration visibility values.
    fn toggle_workspace_decoration_visiblity(&mut self, ctx: &mut ViewContext<Self>) {
        let (new_value, previous_value) =
            TabSettings::handle(ctx).update(ctx, |tab_settings, ctx| {
                let previous_value = *tab_settings.workspace_decoration_visibility.value();
                let new_value = previous_value.toggled();
                report_if_error!(tab_settings
                    .workspace_decoration_visibility
                    .set_value(new_value, ctx));
                (new_value, previous_value)
            });
        send_telemetry_from_ctx!(
            TelemetryEvent::ToggleWorkspaceDecorationVisibility {
                previous_value,
                new_value
            },
            ctx
        );
    }

    fn build_workspace_decoration_visibility_dropdown(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AppearancePageAction>> {
        ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            let values = [
                WorkspaceDecorationVisibility::AlwaysShow,
                WorkspaceDecorationVisibility::OnHover,
                WorkspaceDecorationVisibility::HideFullscreen,
            ];

            let current_value = TabSettings::as_ref(ctx).workspace_decoration_visibility;
            let selected_index = values.iter().position(|val| *val == current_value).unwrap_or_else(|| {
                log::error!("Could not find current WorkspaceDecorationVisibility value in dropdown option list");
                0
            });

            dropdown.set_items(values.into_iter().map(|value| {
                DropdownItem::new(Self::workspace_decoration_visibility_dropdown_item_label(value), AppearancePageAction::SetWorkspaceDecorationVisibility(value))
            }).collect(), ctx);
            dropdown.set_selected_by_index(selected_index, ctx);

            dropdown
        })
    }

    fn build_tab_close_button_position_dropdown(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AppearancePageAction>> {
        ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            let values = [
                TabCloseButtonPosition::Right,
                TabCloseButtonPosition::Left,
            ];

            let current_value = TabSettings::as_ref(ctx).close_button_position;
            let selected_index = values.iter().position(|val| *val == current_value).unwrap_or_else(|| {
                log::error!("Could not find current TabCloseButtonPosition value in dropdown option list");
                0
            });

            dropdown.set_items(values.into_iter().map(|value| {
                DropdownItem::new(Self::tab_close_button_position_dropdown_item_label(value), AppearancePageAction::SetTabCloseButtonPosition(value))
            }).collect(), ctx);
            dropdown.set_selected_by_index(selected_index, ctx);

            dropdown
        })
    }

    fn build_zoom_level_dropdown(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AppearancePageAction>> {
        ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);

            dropdown.set_items(
                crate::window_settings::ZoomLevel::VALUES
                    .iter()
                    .map(|&value| {
                        DropdownItem::new(
                            format!("{value}%"),
                            AppearancePageAction::SetZoomLevel(value),
                        )
                    })
                    .collect(),
                ctx,
            );

            let current_value = *WindowSettings::as_ref(ctx).zoom_level.value();
            dropdown.set_selected_by_action(AppearancePageAction::SetZoomLevel(current_value), ctx);

            dropdown
        })
    }

    fn handle_directory_color_add_picker_event(
        &mut self,
        event: &DirectoryColorAddPickerEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            DirectoryColorAddPickerEvent::Selected(path) => {
                add_directory_tab_color_path(path.clone(), ctx);
            }
            DirectoryColorAddPickerEvent::RequestAddFromFilePicker => {
                open_directory_tab_color_folder_picker(ctx);
            }
        }
    }

    fn handle_tab_settings_event(
        &mut self,
        event: &TabSettingsChangedEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        if let TabSettingsChangedEvent::WorkspaceDecorationVisibility { .. } = event {
            let value = TabSettings::as_ref(ctx).workspace_decoration_visibility;
            let name = Self::workspace_decoration_visibility_dropdown_item_label(value);
            self.workspace_decorations_dropdown
                .update(ctx, |dropdown, ctx| {
                    dropdown.set_selected_by_name(name, ctx);
                });
        }
        if let TabSettingsChangedEvent::DirectoryTabColors { .. } = event {
            let count = directory_tab_colors(ctx).len();
            self.color_picker_dot_states.resize_with(count, || {
                (0..TAB_COLOR_OPTIONS.len() + 1)
                    .map(|_| MouseStateHandle::default())
                    .collect()
            });
            self.directory_tab_color_delete_buttons = build_directory_delete_buttons(ctx);
        }
        ctx.notify();
    }

    fn toggle_ligature_rendering(&mut self, ctx: &mut ViewContext<Self>) {
        if FeatureFlag::Ligatures.is_enabled() {
            let ligature_settings = LigatureSettings::handle(ctx);
            let new_value = !*ligature_settings
                .as_ref(ctx)
                .ligature_rendering_enabled
                .value();

            ligature_settings.update(ctx, |settings, ctx| {
                report_if_error!(settings
                    .ligature_rendering_enabled
                    .set_value(new_value, ctx));
            });

            send_telemetry_from_ctx!(
                TelemetryEvent::ToggleLigatureRendering { enabled: new_value },
                ctx
            );
        }
    }

    pub fn toggle_input_mode(&mut self, ctx: &mut ViewContext<Self>) {
        // Get the current input type
        let current_type = InputSettings::as_ref(ctx).input_type(ctx);

        // Toggle between Universal and Classic
        let new_type = match current_type {
            InputBoxType::Universal => InputBoxType::Classic,
            InputBoxType::Classic => InputBoxType::Universal,
        };

        // Update the setting
        self.set_input_type(new_type, ctx);
    }

    pub fn update_tab_close_button_position(
        &mut self,
        position: TabCloseButtonPosition,
        ctx: &mut ViewContext<Self>,
    ) {
        TabSettings::handle(ctx).update(ctx, |tab_settings, ctx| {
            report_if_error!(tab_settings.close_button_position.set_value(position, ctx));
        });
        send_telemetry_from_ctx!(
            TelemetryEvent::TabCloseButtonPositionUpdated { position },
            ctx
        );
        ctx.notify();
    }
}

// ── Row helpers ──────────────────────────────────────────────────────────────
//
// Every row on the page is drawn by one of these, so the label (and its short description) sits
// on the left and the control on the right, in the same column, at the same size.

/// A row with a switch on the right.
fn switch_row(
    label: &str,
    description: Option<&str>,
    state: &SwitchStateHandle,
    checked: bool,
    action: AppearancePageAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    render_body_item::<AppearancePageAction>(
        label.to_string(),
        None,
        LocalOnlyIconState::Hidden,
        ToggleState::Enabled,
        appearance,
        appearance
            .ui_builder()
            .switch(state.clone())
            .check(checked)
            .build()
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
            .finish(),
        description.map(str::to_owned),
    )
}

/// A row with one of the page's dropdowns on the right.
fn dropdown_row(
    label: &str,
    description: Option<&str>,
    dropdown: &ViewHandle<Dropdown<AppearancePageAction>>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    render_dropdown_item(
        appearance,
        label,
        description,
        None,
        LocalOnlyIconState::Hidden,
        None,
        dropdown,
    )
}

/// A row whose control is a dropdown-sized element that isn't a [`Dropdown`] (the searchable font
/// pickers). Spaced like [`dropdown_row`], whose dropdown carries its own vertical margin.
fn dropdown_like_row(
    label: &str,
    description: Option<&str>,
    control: Box<dyn Element>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let mut text = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    text.add_child(
        Align::new(render_body_item_label::<AppearancePageAction>(
            label.to_string(),
            None,
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
        ))
        .left()
        .finish(),
    );
    if let Some(description) = description {
        text.add_child(render_settings_description(
            description.to_string(),
            appearance,
        ));
    }
    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.,
                    Container::new(text.finish())
                        .with_padding_right(ROW_CONTROL_GAP)
                        .finish(),
                )
                .finish(),
            )
            .with_child(
                ConstrainedBox::new(control)
                    .with_width(DROPDOWN_WIDTH)
                    .finish(),
            )
            .finish(),
    )
    .with_padding_bottom(HEADER_PADDING - DROPDOWN_PADDING)
    .finish()
}

/// A short numeric text field, red-edged while its contents aren't valid. With `on_dismiss`,
/// clicking away commits it.
fn number_input(
    editor: &ViewHandle<EditorView>,
    valid: bool,
    on_dismiss: Option<AppearancePageAction>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            width: Some(NUMBER_INPUT_WIDTH),
            padding: Some(Coords {
                top: 5.,
                bottom: 5.,
                left: 8.,
                right: 8.,
            }),
            background: Some(appearance.theme().surface_2().into()),
            border_color: (!valid).then(|| themes::theme::Fill::error().into()),
            ..Default::default()
        })
        .build()
        .finish();
    match on_dismiss {
        Some(action) => Dismiss::new(input)
            .on_dismiss(move |ctx, _app| ctx.dispatch_typed_action(action.clone()))
            .finish(),
        None => input,
    }
}

/// A row with a short text field on the right.
fn input_row(
    label: &str,
    description: Option<&str>,
    toggle_state: ToggleState,
    control: Box<dyn Element>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    render_body_item::<AppearancePageAction>(
        label.to_string(),
        None,
        LocalOnlyIconState::Hidden,
        toggle_state,
        appearance,
        control,
        description.map(str::to_owned),
    )
}

/// A quiet "Reset" link, shown beside a control whose value differs from its default.
fn reset_link(
    mouse_state: &MouseStateHandle,
    action: AppearancePageAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    appearance
        .ui_builder()
        .button(ButtonVariant::Text, mouse_state.clone())
        .with_style(UiComponentStyles {
            font_size: Some(CONTENT_FONT_SIZE),
            font_color: Some(theme.disabled_text_color(theme.surface_1()).into_solid()),
            padding: Some(Coords::uniform(4.)),
            ..Default::default()
        })
        .with_text_label("Reset".to_owned())
        .build()
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
}

/// A secondary button for the Theme card.
fn theme_card_button(
    label: &str,
    mouse_state: &MouseStateHandle,
    action: WorkspaceAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, mouse_state.clone())
        .with_style(UiComponentStyles {
            font_size: Some(CONTENT_FONT_SIZE + 1.),
            font_weight: Some(Weight::Semibold),
            padding: Some(Coords::uniform(7.).left(14.).right(14.)),
            ..Default::default()
        })
        .with_centered_text_label(label.into())
        .build()
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
}

// ── Theme ────────────────────────────────────────────────────────────────────

/// The theme in effect, with the ways to change it: the gallery, which is the one place themes
/// are browsed and switched, and the theme creator.
#[derive(Default)]
struct CurrentThemeWidget {
    browse_button_state: MouseStateHandle,
    create_button_state: MouseStateHandle,
}

impl SettingsWidget for CurrentThemeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "theme themes current gallery browse download community create custom theme creator \
         background backgrounds color colors customize"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme_settings = ThemeSettings::as_ref(app);
        let kind = active_theme_kind(theme_settings, app);
        let definition = WarpConfig::as_ref(app).theme_config().theme(&kind);
        let detail = match respect_system_theme(theme_settings) {
            RespectSystemTheme::Off => "Current theme",
            RespectSystemTheme::On(_) => match app.system_theme() {
                SystemTheme::Light => "Current theme, following your system's light mode",
                SystemTheme::Dark => "Current theme, following your system's dark mode",
            },
        };

        let preview = Container::new(theme::render_preview(
            &definition,
            appearance.monospace_font_family(),
            Some(CURRENT_THEME_PREVIEW_SCALE),
        ))
        .with_corner_radius(warp_core::ui::radius::small())
        .with_margin_right(12.)
        .finish();

        let text = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                Text::new_inline(kind.to_string(), appearance.ui_font_family(), 14.)
                    .with_color(appearance.theme().active_ui_text_color().into())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .with_child(render_settings_description(detail.to_string(), appearance))
            .finish();

        let current = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(preview)
                .with_child(Shrinkable::new(1., text).finish())
                .finish(),
        )
        .with_padding_bottom(HEADER_PADDING)
        .finish();

        // A wrap, so the buttons stack rather than overflow in a narrow pane.
        let buttons = Container::new(
            Wrap::row()
                .with_spacing(8.)
                .with_run_spacing(8.)
                .with_child(theme_card_button(
                    "Browse gallery",
                    &self.browse_button_state,
                    WorkspaceAction::ShowThemeGallery,
                    appearance,
                ))
                .with_child(theme_card_button(
                    "Create a theme",
                    &self.create_button_state,
                    WorkspaceAction::ShowSettingsPage(SettingsSection::ThemeCreator),
                    appearance,
                ))
                .finish(),
        )
        .with_padding_bottom(HEADER_PADDING)
        .finish();

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(current)
            .with_child(buttons)
            .finish()
    }
}

/// "Sync with OS", and — while it is on — the theme for each system appearance.
#[derive(Default)]
struct SyncThemeWithOsWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for SyncThemeWithOsWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "sync with os system theme themes light dark mode appearance automatic switch"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let syncing = matches!(
            respect_system_theme(ThemeSettings::as_ref(app)),
            RespectSystemTheme::On(_)
        );
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(switch_row(
                "Sync with OS",
                Some("Switch between a light and a dark theme when your system does."),
                &self.switch_state,
                syncing,
                AppearancePageAction::ToggleRespectSystemTheme,
                appearance,
            ));
        if syncing {
            column.add_child(dropdown_like_row(
                "Light theme",
                None,
                ChildView::new(&view.light_theme_dropdown).finish(),
                appearance,
            ));
            column.add_child(dropdown_like_row(
                "Dark theme",
                None,
                ChildView::new(&view.dark_theme_dropdown).finish(),
                appearance,
            ));
        }
        column.finish()
    }
}

// ── Dock ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct CustomAppIconWidget {
    show_dock_icon_switch_state: SwitchStateHandle,
}

impl SettingsWidget for CustomAppIconWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "customize custom app icon icons dock cmd tab app switcher"
    }

    fn should_render(&self, app: &AppContext) -> bool {
        AppIconSettings::as_ref(app)
            .show_dock_icon
            .is_supported_on_current_platform()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        // Uncaged ships a single app icon, so there is no icon to pick — only whether it shows.
        switch_row(
            "Show Uncaged in the Dock",
            Some("When off, Uncaged is reached from its windows and the app switcher only."),
            &self.show_dock_icon_switch_state,
            *AppIconSettings::as_ref(app).show_dock_icon,
            AppearancePageAction::ToggleShowDockIcon,
            appearance,
        )
    }
}

// ── Window ───────────────────────────────────────────────────────────────────

#[derive(Default)]
struct CustomWindowSizeWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for CustomWindowSizeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "open windows with custom size columns rows new window"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let enabled = *WindowSettings::as_ref(app)
            .open_windows_at_custom_size
            .value();
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(switch_row(
                "Custom size for new windows",
                Some("Open new windows at a set number of columns and rows."),
                &self.switch_state,
                enabled,
                AppearancePageAction::ToggleOpenWindowsAtCustomSize,
                appearance,
            ));
        if enabled {
            column.add_child(input_row(
                "Columns",
                None,
                ToggleState::Enabled,
                number_input(
                    &view.new_window_columns_editor,
                    view.valid_new_window_columns,
                    Some(AppearancePageAction::SetNewWindowsCustomColumns),
                    appearance,
                ),
                appearance,
            ));
            column.add_child(input_row(
                "Rows",
                None,
                ToggleState::Enabled,
                number_input(
                    &view.new_window_rows_editor,
                    view.valid_new_window_rows,
                    Some(AppearancePageAction::SetNewWindowsCustomRows),
                    appearance,
                ),
                appearance,
            ));
        }
        column.finish()
    }
}

struct WindowOpacityWidget;

impl SettingsWidget for WindowOpacityWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "window opacity transparency transparent"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let window_settings = WindowSettings::as_ref(app);
        if !window_settings
            .background_opacity
            .is_configurable(view.window_id, app)
        {
            return input_row(
                "Window opacity",
                Some("Transparency is not supported with your graphics drivers."),
                ToggleState::Disabled,
                Empty::new().finish(),
                appearance,
            );
        }

        // Skip the warning for OpenGL, since WGPU often incorrectly reports it as not supporting
        // alpha.
        let warning = app
            .windows()
            .platform_window(view.window_id)
            .filter(|window| {
                !window.supports_transparency() && window.graphics_backend() != GraphicsBackend::Gl
            })
            .map(|_| {
                let mut message = Cow::Borrowed(
                    "The selected graphics settings may not support transparent windows.",
                );
                let gpu_settings = GPUSettings::as_ref(app);
                if (gpu_settings
                    .prefer_low_power_gpu
                    .is_supported_on_current_platform()
                    && GPUState::as_ref(app).is_low_power_gpu_available())
                    || gpu_settings
                        .preferred_backend
                        .is_supported_on_current_platform()
                {
                    message
                        .to_mut()
                        .push_str(" Try another graphics backend or GPU in Features > System.");
                }
                message.into_owned()
            });

        let opacity = *window_settings.background_opacity;
        render_slider_row(
            "Window opacity",
            warning,
            SliderRow {
                state: view.opacity_state.clone(),
                value: opacity,
                range: BackgroundOpacity::MIN as f32..BackgroundOpacity::MAX as f32,
                value_label: format!("{opacity}%"),
                on_drag: AppearancePageAction::OpacitySliderDragged,
                on_change: AppearancePageAction::SetOpacity,
            },
            appearance,
        )
    }
}

struct WindowBlurWidget;

impl SettingsWidget for WindowBlurWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "window blur radius"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let blur = *WindowSettings::as_ref(app).background_blur_radius;
        render_slider_row(
            "Window blur",
            Some("Blurs what shows through a transparent window.".to_string()),
            SliderRow {
                state: view.blur_state.clone(),
                value: blur,
                range: BackgroundBlurRadius::MIN as f32..BackgroundBlurRadius::MAX as f32,
                value_label: format!("{blur}"),
                on_drag: AppearancePageAction::BlurSliderDragged,
                on_change: AppearancePageAction::SetBlur,
            },
            appearance,
        )
    }
}

#[derive(Default)]
struct LaunchAnimationWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for LaunchAnimationWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "launch animation splash startup start logo"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Launch animation",
            Some("Show the Uncaged mark for a moment when the app starts."),
            &self.switch_state,
            *WindowSettings::as_ref(app).show_launch_animation.value(),
            AppearancePageAction::ToggleLaunchAnimation,
            appearance,
        )
    }
}

#[derive(Default)]
struct WindowBlurTextureWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for WindowBlurTextureWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "window blur texture acrylic"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let unsupported = app
            .windows()
            .platform_window(view.window_id)
            .is_some_and(|window| {
                !window.supports_transparency() && window.graphics_backend() != GraphicsBackend::Gl
            });
        switch_row(
            "Acrylic blur texture",
            Some(if unsupported {
                "Your hardware may not support transparent windows."
            } else {
                "Give the window blur a frosted, acrylic finish."
            }),
            &self.switch_state,
            *WindowSettings::as_ref(app).background_blur_texture,
            AppearancePageAction::ToggleBlurTexture,
            appearance,
        )
    }
}

#[derive(Default)]
struct ToolsPanelStateScopeWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ToolsPanelStateScopeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "left tools panel open closed across tabs file tree project explorer global search warp drive conversation list"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Same tools panel in every tab",
            Some("Opening or closing the tools panel applies to all tabs, not just this one."),
            &self.switch_state,
            *WindowSettings::as_ref(app).left_panel_visibility_across_tabs,
            AppearancePageAction::ToggleLeftPanelVisibility,
            appearance,
        )
    }
}

struct ZoomLevelWidget;

impl SettingsWidget for ZoomLevelWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "zoom level zoom size scale"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let changed_from_default =
            *WindowSettings::as_ref(app).zoom_level.value() != ZoomLevel::default_value();
        let reset_button = build_reset_button(
            appearance,
            view.zoom_reset_button_mouse_state.clone(),
            changed_from_default,
        )
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(AppearancePageAction::ResetZoomLevel);
        })
        .finish();

        render_dropdown_item(
            appearance,
            "Zoom",
            Some("Scales the whole interface, in every window."),
            Some(reset_button),
            LocalOnlyIconState::Hidden,
            None,
            &view.zoom_level_dropdown,
        )
    }
}

// ── Input ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct InputTypeWidget {
    segment_states: [MouseStateHandle; 2],
}

impl SettingsWidget for InputTypeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "input type warp universal classic style prompt terminal ai developer mode interface shell chips ps1"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let input_type = InputSettings::as_ref(app).input_type(app);
        input_row(
            "Input type",
            Some("Uncaged's own input editor, or your shell's prompt (PS1)."),
            ToggleState::Enabled,
            render_segmented_control(
                vec![
                    Segment {
                        label: "Uncaged",
                        active: input_type == InputBoxType::Universal,
                        mouse_state: self.segment_states[0].clone(),
                        action: AppearancePageAction::SetInputType(InputBoxType::Universal),
                    },
                    Segment {
                        label: "Shell (PS1)",
                        active: input_type == InputBoxType::Classic,
                        mouse_state: self.segment_states[1].clone(),
                        action: AppearancePageAction::SetInputType(InputBoxType::Classic),
                    },
                ],
                appearance,
            ),
            appearance,
        )
    }
}

#[derive(Default)]
struct InputModeWidget {}

impl SettingsWidget for InputModeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "input mode input position pinned top bottom classic waterfall reverse"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        dropdown_row(
            "Input position",
            None,
            &view.input_mode_dropdown,
            appearance,
        )
    }
}

#[derive(Default)]
struct PromptWidget {
    button_mouse_state: MouseStateHandle,
}

impl SettingsWidget for PromptWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "prompt ps1 terminal warp shell custom"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let session_settings = SessionSettings::as_ref(app);
        let honor_ps1 = *session_settings.honor_ps1;
        let background = internal_colors::fg_overlay_1(appearance.theme());

        let body = if honor_ps1 {
            // TODO: we should render something else when the grid info isn't available.
            if let Some((grid, size_info)) = &view.ps1_grid_info {
                let left_padding = size_info.padding_x_px();
                let prompt_grid = BlockGridElement::new(
                    grid,
                    appearance,
                    *FontSettings::as_ref(app).enforce_minimum_contrast,
                    ObfuscateSecrets::No,
                    *size_info,
                )
                .finish();

                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_child(
                        Clipped::new(
                            Container::new(prompt_grid)
                                // Remove any left-padding built into the prompt to make sure it's
                                // left-aligned with the title.
                                .with_padding_left(-left_padding.as_f32())
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish()
            } else {
                Empty::new().finish()
            }
        } else {
            Wrap::row()
                .with_children(view.context_chips.iter().map(|renderer| {
                    Container::new(renderer.render_unused(ChipDragState::Undraggable, appearance))
                        .with_margin_right(4.)
                        .finish()
                }))
                .with_run_spacing(4.)
                .finish()
        };

        let preview = Hoverable::new(self.button_mouse_state.clone(), |hover_state| {
            let (border_color, border_width) = match hover_state.is_hovered() {
                true => (appearance.theme().accent(), 1.0),
                false => (appearance.theme().accent(), 0.0),
            };

            Container::new(body)
                .with_background(background)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                .with_border(Border::all(border_width).with_border_fill(border_color))
                .with_horizontal_padding(24. - border_width)
                .with_vertical_padding(12. - border_width)
                .with_margin_bottom(HEADER_PADDING)
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(WorkspaceAction::OpenPromptEditor {
                open_source: PromptEditorOpenSource::AppearancePage,
            })
        })
        .finish();

        let description = "What shows above the input. Click the preview to edit it.";
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_child(render_body_item_label::<AppearancePageAction>(
                            "Prompt".to_string(),
                            None,
                            None,
                            LocalOnlyIconState::Hidden,
                            ToggleState::Enabled,
                            appearance,
                        ))
                        .with_child(render_settings_description(
                            description.to_string(),
                            appearance,
                        ))
                        .finish(),
                )
                .with_padding_bottom(8.)
                .finish(),
            )
            .with_child(preview)
            .finish()
    }
}

// ── Blocks & panes ───────────────────────────────────────────────────────────

#[derive(Default)]
struct DimInactivePanesWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for DimInactivePanesWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "dim inactive panes"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Dim inactive panes",
            None,
            &self.switch_state,
            *PaneSettings::as_ref(app).should_dim_inactive_panes,
            AppearancePageAction::ToggleDimInactivePanes,
            appearance,
        )
    }
}

#[derive(Default)]
struct FocusFollowsMouseWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for FocusFollowsMouseWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "focus follows mouse hover pane"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Focus follows mouse",
            Some("Focus a pane by moving the pointer over it."),
            &self.switch_state,
            *PaneSettings::as_ref(app).focus_panes_on_hover,
            AppearancePageAction::ToggleFocusPaneOnHover,
            appearance,
        )
    }
}

#[derive(Default)]
struct CompactModeWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for CompactModeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "compact mode spacing padding"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let is_compact_mode = matches!(
            TerminalSettings::as_ref(app).spacing_mode.value(),
            SpacingMode::Compact
        );
        switch_row(
            "Compact mode",
            Some("Less space around blocks, so more output fits."),
            &self.switch_state,
            is_compact_mode,
            AppearancePageAction::ToggleCompactMode,
            appearance,
        )
    }
}

#[derive(Default)]
struct JumpToBottomOfBlockWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for JumpToBottomOfBlockWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "jump to bottom of block button"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Jump to bottom of block button",
            Some("Offer a button that jumps to the end of long output."),
            &self.switch_state,
            *BlockListSettings::as_ref(app)
                .show_jump_to_bottom_of_block_button
                .value(),
            AppearancePageAction::ToggleJumpToBottomOfBlockButton,
            appearance,
        )
    }
}

#[derive(Default)]
struct ShowBlockDividersWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ShowBlockDividersWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "show block dividers"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Block dividers",
            None,
            &self.switch_state,
            *BlockListSettings::as_ref(app).show_block_dividers.value(),
            AppearancePageAction::ToggleShowBlockDividers,
            appearance,
        )
    }
}

#[derive(Default)]
struct AltScreenPaddingWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for AltScreenPaddingWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "alt screen padding border space vim full-screen fullscreen apps"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let custom = matches!(
            TerminalSettings::as_ref(app).alt_screen_padding.value(),
            AltScreenPaddingMode::Custom { .. }
        );
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(switch_row(
                "Custom padding in full-screen apps",
                Some("Use your own padding in apps like vim or less, instead of the usual one."),
                &self.switch_state,
                custom,
                AppearancePageAction::ToggleAltScreenPadding,
                appearance,
            ));

        if custom {
            let buffer_text = view.alt_screen_padding_editor.as_ref(app).buffer_text(app);
            let valid = matches!(buffer_text.parse::<f32>(), Ok(p) if p >= 0.);
            column.add_child(input_row(
                "Padding (px)",
                None,
                ToggleState::Enabled,
                number_input(&view.alt_screen_padding_editor, valid, None, appearance),
                appearance,
            ));
        }
        column.finish()
    }
}

// ── Text ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct TerminalFontWidget {
    all_fonts_switch_state: SwitchStateHandle,
}

impl SettingsWidget for TerminalFontWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text terminal font family monospace system fonts"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(dropdown_like_row(
                "Terminal font",
                None,
                ChildView::new(&view.font_family_dropdown).finish(),
                appearance,
            ))
            .with_child(switch_row(
                "List every system font",
                Some("Include fonts that aren't monospaced in the list above."),
                &self.all_fonts_switch_state,
                view.view_font_type == FontType::Any,
                AppearancePageAction::ToggleAllAvailableFonts,
                appearance,
            ))
            .finish()
    }
}

struct FontWeightWidget;

impl SettingsWidget for FontWeightWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text terminal font weight bold"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        dropdown_row("Font weight", None, &view.font_weight_dropdown, appearance)
    }
}

struct FontSizeWidget;

impl SettingsWidget for FontSizeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text terminal font size px"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        input_row(
            "Font size (px)",
            None,
            ToggleState::Enabled,
            number_input(
                &view.font_size_editor,
                true,
                Some(AppearancePageAction::SetFontSize),
                appearance,
            ),
            appearance,
        )
    }
}

#[derive(Default)]
struct LineHeightWidget {
    reset_state: MouseStateHandle,
}

impl SettingsWidget for LineHeightWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text terminal line height spacing"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let mut control = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        if appearance.line_height_ratio() != DEFAULT_UI_LINE_HEIGHT_RATIO {
            control.add_child(
                Container::new(reset_link(
                    &self.reset_state,
                    AppearancePageAction::LineHeightEditorResetRatio,
                    appearance,
                ))
                .with_margin_right(8.)
                .finish(),
            );
        }
        control.add_child(number_input(
            &view.line_height_editor,
            true,
            Some(AppearancePageAction::SetLineHeight),
            appearance,
        ));
        input_row(
            "Line height",
            Some("A multiple of the font size."),
            ToggleState::Enabled,
            control.finish(),
            appearance,
        )
    }
}

#[derive(Default)]
struct AIFontWidget {
    match_switch_state: SwitchStateHandle,
}

impl SettingsWidget for AIFontWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text agent ai font family match terminal monospace"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(dropdown_like_row(
                "Agent font",
                Some("The font of agent conversations."),
                ChildView::new(&view.ai_font_family_dropdown).finish(),
                appearance,
            ))
            .with_child(switch_row(
                "Agent font matches terminal font",
                None,
                &self.match_switch_state,
                *FontSettings::as_ref(app).match_ai_font_to_terminal_font,
                AppearancePageAction::ToggleMatchAIToTerminalFontFamily,
                appearance,
            ))
            .finish()
    }
}

#[derive(Default)]
struct NotebookFontSizeWidget {
    match_switch_state: SwitchStateHandle,
}

impl SettingsWidget for NotebookFontSizeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text notebook font size match terminal"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let matching = *FontSettings::as_ref(app).match_notebook_to_monospace_font_size;
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(input_row(
                "Notebook font size (px)",
                None,
                (!matching).into(),
                number_input(
                    &view.notebook_font_size_editor,
                    true,
                    Some(AppearancePageAction::SetNotebookFontSize),
                    appearance,
                ),
                appearance,
            ))
            .with_child(switch_row(
                "Notebook font size matches terminal",
                None,
                &self.match_switch_state,
                matching,
                AppearancePageAction::ToggleMatchNotebookToMonospaceFontSize,
                appearance,
            ))
            .finish()
    }
}

#[derive(Default)]
struct ThinStrokesWidget {}

impl SettingsWidget for ThinStrokesWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text thin strokes high dpi"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        dropdown_row(
            "Thin strokes",
            Some("Draw text with lighter strokes."),
            &view.thin_strokes_dropdown,
            appearance,
        )
    }
}

#[derive(Default)]
struct MinimumContrastWidget {}

impl SettingsWidget for MinimumContrastWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text minimum contrast high"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        dropdown_row(
            "Minimum contrast",
            Some("Adjust text colors that are too close to the background."),
            &view.enforce_min_contrast_dropdown,
            appearance,
        )
    }
}

#[derive(Default)]
struct LigaturesWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for LigaturesWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "text font ligatures"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Ligatures",
            Some("Join character pairs like -> in the terminal. May reduce performance."),
            &self.switch_state,
            *LigatureSettings::as_ref(app)
                .ligature_rendering_enabled
                .value(),
            AppearancePageAction::ToggleLigatureRendering,
            appearance,
        )
    }
}

// ── Cursor ───────────────────────────────────────────────────────────────────

struct CursorTypeWidget {
    segment_states: Vec<MouseStateHandle>,
}

impl Default for CursorTypeWidget {
    fn default() -> Self {
        Self {
            segment_states: all::<CursorDisplayType>()
                .map(|_| Default::default())
                .collect(),
        }
    }
}

impl SettingsWidget for CursorTypeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "cursor shape cursor type block bar beam underline"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let settings = AppEditorSettings::as_ref(app);
        if *settings.vim_mode.value() {
            return input_row(
                "Cursor shape",
                Some("Vim mode sets the cursor shape."),
                ToggleState::Disabled,
                Empty::new().finish(),
                appearance,
            );
        }

        let current = *settings.cursor_display_type.value();
        let segments = all::<CursorDisplayType>()
            .zip(self.segment_states.iter().cloned())
            .map(|(cursor, mouse_state)| Segment {
                label: cursor_display_type_label(cursor),
                active: cursor == current,
                mouse_state,
                action: AppearancePageAction::SetCursorType(cursor),
            })
            .collect();
        input_row(
            "Cursor shape",
            None,
            ToggleState::Enabled,
            render_segmented_control(segments, appearance),
            appearance,
        )
    }
}

/// The name of a cursor shape, for its segment.
fn cursor_display_type_label(cursor: CursorDisplayType) -> &'static str {
    match cursor {
        CursorDisplayType::Bar => "Bar",
        CursorDisplayType::Block => "Block",
        CursorDisplayType::Underline => "Underline",
    }
}

#[derive(Default)]
struct BlinkingCursorWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for BlinkingCursorWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "blinking cursor blink"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Blinking cursor",
            None,
            &self.switch_state,
            AppEditorSettings::as_ref(app).cursor_blink.value() == &CursorBlink::Enabled,
            AppearancePageAction::ToggleCursorBlink,
            appearance,
        )
    }
}

// ── Tabs ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct TabCloseButtonPositionWidget {}

impl SettingsWidget for TabCloseButtonPositionWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "tab bar close button position left right"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        dropdown_row(
            "Close button position",
            None,
            &view.tab_close_button_position_dropdown,
            appearance,
        )
    }
}

#[derive(Default)]
struct TabIndicatorWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for TabIndicatorWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "tab indicator"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Tab indicators",
            None,
            &self.switch_state,
            *TabSettings::as_ref(app).show_indicators,
            AppearancePageAction::ToggleTabIndicators,
            appearance,
        )
    }
}

#[derive(Default)]
struct CodeReviewButtonWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for CodeReviewButtonWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "code review button tab bar"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Code review button",
            None,
            &self.switch_state,
            *TabSettings::as_ref(app).show_code_review_button,
            AppearancePageAction::ToggleShowCodeReviewButton,
            appearance,
        )
    }
}

#[derive(Default)]
struct PreserveActiveTabColorWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for PreserveActiveTabColorWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "tab color preserve new inherit active"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "New tabs keep the active tab's color",
            None,
            &self.switch_state,
            *TabSettings::as_ref(app).preserve_active_tab_color,
            AppearancePageAction::TogglePreserveActiveTabColor,
            appearance,
        )
    }
}

#[derive(Default)]
struct VerticalTabsWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for VerticalTabsWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "vertical tabs sidebar layout"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Vertical tabs",
            Some("List tabs in a panel at the side instead of a bar along the top."),
            &self.switch_state,
            *TabSettings::as_ref(app).use_vertical_tabs,
            AppearancePageAction::ToggleVerticalTabs,
            appearance,
        )
    }
}

#[derive(Default)]
struct ShowVerticalTabPanelInRestoredWindowsWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ShowVerticalTabPanelInRestoredWindowsWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "vertical tabs panel restore window session snapshot"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Show the vertical tabs panel in restored windows",
            Some("Reopened windows show the panel even if it was closed when they were saved."),
            &self.switch_state,
            *TabSettings::as_ref(app).show_vertical_tab_panel_in_restored_windows,
            AppearancePageAction::ToggleShowVerticalTabPanelInRestoredWindows,
            appearance,
        )
    }
}

#[derive(Default)]
struct HideTitleBarSearchBarInVerticalTabsWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for HideTitleBarSearchBarInVerticalTabsWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "hide title bar search bar vertical tabs chrome minimal"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Hide the title bar search with vertical tabs",
            Some("Search stays available from the command palette and its shortcut."),
            &self.switch_state,
            *TabSettings::as_ref(app).hide_title_bar_search_bar_in_vertical_tabs,
            AppearancePageAction::ToggleHideTitleBarSearchBarInVerticalTabs,
            appearance,
        )
    }
}

#[derive(Default)]
struct UseLatestUserPromptAsConversationTitleInTabNamesWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for UseLatestUserPromptAsConversationTitleInTabNamesWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "latest user prompt conversation title tab names vertical tabs oz third-party agent"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        switch_row(
            "Name agent tabs after your latest prompt",
            Some("Instead of the generated conversation title, in vertical tabs."),
            &self.switch_state,
            *TabSettings::as_ref(app).use_latest_user_prompt_as_conversation_title_in_tab_names,
            AppearancePageAction::ToggleUseLatestUserPromptAsConversationTitleInTabNames,
            appearance,
        )
    }
}

#[derive(Default)]
struct ZenModeWidget {}

impl SettingsWidget for ZenModeWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "zen mode minimal tab bar window decoration"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        dropdown_row(
            "Show the tab bar",
            None,
            &view.workspace_decorations_dropdown,
            appearance,
        )
    }
}

#[derive(Default)]
struct EditToolbarWidget;

impl SettingsWidget for EditToolbarWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "edit toolbar header panel buttons configure arrange layout chip chips rearrange re-arrange customize"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let label = render_body_item_label::<AppearancePageAction>(
            "Header toolbar layout".to_string(),
            None,
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
        );
        let editor = Container::new(ChildView::new(&view.header_toolbar_inline_editor).finish())
            .with_padding_bottom(HEADER_PADDING)
            .finish();

        Flex::column()
            .with_child(Container::new(label).with_margin_bottom(4.).finish())
            .with_child(editor)
            .finish()
    }
}

/// Returns the visible directory tab colors from the persisted setting.
fn build_directory_delete_buttons(
    ctx: &mut ViewContext<AppearanceSettingsPageView>,
) -> Vec<ViewHandle<ActionButton>> {
    directory_tab_colors(ctx)
        .into_iter()
        .map(|(dir_path, _)| {
            let delete_path = PathBuf::from(&dir_path);
            ctx.add_typed_action_view(move |_| {
                ActionButton::new("", NakedTheme)
                    .with_icon(Icon::X)
                    .with_size(ButtonSize::XSmall)
                    .on_click(move |ctx| {
                        ctx.dispatch_typed_action(
                            AppearancePageAction::RemoveDefaultDirectoryTabColor {
                                path: delete_path.clone(),
                            },
                        );
                    })
            })
        })
        .collect()
}

fn add_directory_tab_color_path(path: PathBuf, ctx: &mut ViewContext<AppearanceSettingsPageView>) {
    TabSettings::handle(ctx).update(ctx, |settings, ctx| {
        let current = settings.directory_tab_colors.value();
        let key = canonical_directory_key(&path);
        let dominated_by_existing = current
            .0
            .get(&key)
            .is_some_and(|c| *c != DirectoryTabColor::Suppressed);
        if !dominated_by_existing {
            let new_value = current.with_color(&path, DirectoryTabColor::Unassigned);
            let _ = settings.directory_tab_colors.set_value(new_value, ctx);
        }
    });
}

fn open_directory_tab_color_folder_picker(ctx: &mut ViewContext<AppearanceSettingsPageView>) {
    let file_picker_config = FilePickerConfiguration::new().folders_only();
    ctx.open_file_picker(
        move |result, ctx| {
            if let Ok(paths) = result {
                if let Some(directory_path) = paths.first() {
                    add_directory_tab_color_path(PathBuf::from(directory_path), ctx);
                }
            }
        },
        file_picker_config,
    );
}

fn directory_tab_colors(app: &AppContext) -> Vec<(String, DirectoryTabColor)> {
    let configured = &TabSettings::as_ref(app).directory_tab_colors.value().0;
    let mut sorted: Vec<_> = configured
        .iter()
        .filter(|(_, color)| !matches!(color, DirectoryTabColor::Suppressed))
        .map(|(path, color)| (path.clone(), *color))
        .collect();
    sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
    sorted
}

struct DirectoryTabColorsWidget {
    add_picker: ViewHandle<DirectoryColorAddPicker>,
}

impl SettingsWidget for DirectoryTabColorsWidget {
    type View = AppearanceSettingsPageView;

    fn search_terms(&self) -> &str {
        "directory tab color folder codebase repo"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();

        let mut content = Flex::column().with_spacing(8.);
        let header_text = Flex::column()
            .with_spacing(4.)
            .with_child(
                Text::new(
                    "Directory tab colors",
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(theme.active_ui_text_color().into())
                .soft_wrap(false)
                .finish(),
            )
            .with_child(
                Text::new(
                    "Automatically color tabs based on the directory or repo you're working in.",
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(theme.nonactive_ui_text_color().into())
                .finish(),
            )
            .finish();
        content.add_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_child(Shrinkable::new(1., header_text).finish())
                .with_child(ChildView::new(&self.add_picker).finish())
                .finish(),
        );

        let home_dir =
            dirs::home_dir().and_then(|home_dir| home_dir.to_str().map(|s| s.to_owned()));
        for (idx, (dir_path, current_color)) in directory_tab_colors(app).into_iter().enumerate() {
            let Some(dot_mouse_states) = view.color_picker_dot_states.get(idx).cloned() else {
                log::error!("Missing color picker dot states for directory index {idx}");
                continue;
            };

            let friendly_path = user_friendly_path(&dir_path, home_dir.as_deref()).to_string();
            let path_label = Shrinkable::new(
                1.,
                Text::new(
                    friendly_path,
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(theme.nonactive_ui_text_color().into())
                .soft_wrap(false)
                .finish(),
            )
            .finish();

            let mut dots_row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);

            // None = default (no-color) dot, Some = ANSI color dot
            let color_options =
                std::iter::once(None).chain(TAB_COLOR_OPTIONS.iter().copied().map(Some));
            for (ansi_id, mouse_state) in color_options.zip(dot_mouse_states.iter().cloned()) {
                let tab_color = match ansi_id {
                    None => DirectoryTabColor::Unassigned,
                    Some(id) => DirectoryTabColor::Color(id),
                };
                let dot_color = match ansi_id {
                    None => pathfinder_color::ColorU::transparent_black(),
                    Some(id) => id.to_ansi_color(&theme.terminal_colors().normal).into(),
                };
                let is_selected = current_color == tab_color;
                let tooltip_text = match ansi_id {
                    None => "Default (no color)".to_string(),
                    Some(id) => id.to_string(),
                };
                let dir_path_clone = PathBuf::from(&dir_path);

                dots_row.add_child(
                    render_color_dot(
                        mouse_state,
                        dot_color,
                        is_selected,
                        theme.accent().into(),
                        ansi_id.is_none(),
                        theme.foreground(),
                        tooltip_text,
                        appearance,
                    )
                    .on_click(move |ctx, _, _| {
                        if !is_selected {
                            ctx.dispatch_typed_action(
                                AppearancePageAction::SetDefaultDirectoryTabColor {
                                    path: dir_path_clone.clone(),
                                    color: tab_color,
                                },
                            );
                        }
                    })
                    .finish(),
                );
            }

            let mut right_controls = Flex::row()
                .with_spacing(8.)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            right_controls.add_child(dots_row.finish());
            if let Some(delete_button) = view.directory_tab_color_delete_buttons.get(idx) {
                right_controls.add_child(ChildView::new(delete_button).finish());
            }
            let right_controls = right_controls.finish();

            let row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_child(path_label)
                .with_child(right_controls)
                .finish();

            content.add_child(
                Container::new(row)
                    .with_horizontal_padding(16.)
                    .with_vertical_padding(8.)
                    .with_background(theme.surface_1())
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                        warp_core::ui::radius::SMALL,
                    )))
                    .finish(),
            );
        }

        Container::new(content.finish())
            .with_padding_bottom(HEADER_PADDING)
            .finish()
    }
}

impl SettingsPageMeta for AppearanceSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::Appearance
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
    }

    fn on_page_selected(&mut self, _: bool, ctx: &mut ViewContext<Self>) {
        ctx.notify();
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<AppearanceSettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<AppearanceSettingsPageView>) -> Self {
        SettingsPageViewHandle::Appearance(view_handle)
    }
}
