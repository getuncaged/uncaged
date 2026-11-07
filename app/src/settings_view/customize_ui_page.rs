//! Uncaged: a dedicated, findable home for the interface choices offered on the
//! onboarding "Customize UI" slide.
//!
//! Every control here reads/writes the SAME `Setting` field the slide applies
//! (see `app/src/settings/onboarding.rs::apply_ui_customization_settings`) and
//! the scattered controls on the Code and Agents pages, so setup and Settings
//! stay in sync. The Warp Drive ("Drive") toggle in particular has no other home
//! in the Uncaged sidebar, so this page is its only surface.
//!
//! Presented as its own top-level sidebar entry (not buried among the many
//! Appearance sections) so it is easy to find and read.

use ::settings::{Setting, ToggleableSetting};
use uncaged_ui::elements::Element;
use uncaged_ui::ui_components::components::UiComponent;
use uncaged_ui::ui_components::switch::SwitchStateHandle;
use uncaged_ui::{
    AppContext, Entity, SingletonEntity, TypedActionView, UpdateModel, View, ViewContext,
    ViewHandle,
};

use super::settings_page::{
    render_body_item, Category, MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, ToggleState,
};
use super::SettingsSection;
use crate::appearance::Appearance;
use crate::drive::settings::WarpDriveSettings;
use crate::features::FeatureFlag;
use crate::report_if_error;
use crate::settings::{AISettings, CodeSettings};
use crate::workspace::tab_settings::TabSettings;

#[derive(Debug, Clone)]
pub enum CustomizeUiSettingsPageAction {
    ProjectExplorer,
    ConversationHistory,
    GlobalSearch,
    WarpDrive,
    CodeReviewButton,
}

/// This page emits no events; the empty enum satisfies the `Entity` bound.
pub enum CustomizeUiSettingsPageEvent {}

pub struct CustomizeUiSettingsPageView {
    page: PageType<Self>,
}

impl CustomizeUiSettingsPageView {
    pub fn new(_ctx: &mut ViewContext<Self>) -> Self {
        // Mirrors the onboarding "Customize your Uncaged" slide's Tools panel and
        // Code review cards, so those choices are findable in one dedicated place.
        // Its Tab styling card lives in Appearance > Tabs, beside the options that
        // depend on it.
        Self {
            page: PageType::new_categorized(
                vec![
                    Category::new(
                        "Tools panel",
                        vec![
                            Box::new(ProjectExplorerWidget::default()),
                            Box::new(ConversationHistoryWidget::default()),
                            Box::new(GlobalSearchWidget::default()),
                            Box::new(DriveWidget::default()),
                        ],
                    )
                    .with_subtitle("Choose which tools appear in the left tools panel."),
                    Category::new(
                        "Code review",
                        vec![Box::new(CodeReviewButtonWidget::default())],
                    ),
                ],
                None,
            ),
        }
    }
}

impl Entity for CustomizeUiSettingsPageView {
    type Event = CustomizeUiSettingsPageEvent;
}

impl TypedActionView for CustomizeUiSettingsPageView {
    type Action = CustomizeUiSettingsPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            CustomizeUiSettingsPageAction::ProjectExplorer => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.show_project_explorer.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            CustomizeUiSettingsPageAction::ConversationHistory => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .show_conversation_history
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            CustomizeUiSettingsPageAction::GlobalSearch => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.show_global_search.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            CustomizeUiSettingsPageAction::WarpDrive => {
                WarpDriveSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.enable_warp_drive.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            CustomizeUiSettingsPageAction::CodeReviewButton => {
                let tab_settings = TabSettings::handle(ctx);
                let new_value = !*tab_settings.as_ref(ctx).show_code_review_button.value();
                ctx.update_model(&tab_settings, move |tab_settings, ctx| {
                    report_if_error!(tab_settings
                        .show_code_review_button
                        .set_value(new_value, ctx));
                });
                ctx.notify();
            }
        }
    }
}

impl View for CustomizeUiSettingsPageView {
    fn ui_name() -> &'static str {
        "CustomizeUiPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for CustomizeUiSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::CustomizeUi
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        FeatureFlag::OpenWarpNewSettingsModes.is_enabled()
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

impl From<ViewHandle<CustomizeUiSettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<CustomizeUiSettingsPageView>) -> Self {
        SettingsPageViewHandle::CustomizeUi(view_handle)
    }
}

// --- toggles ---
//
// Labels match the onboarding "Customize UI" cards/chips and the tools-panel tab
// tooltips ("Project explorer", "Global search", "Drive"). Each toggle reads
// the live `Setting` value and dispatches the page action that persists it.

#[derive(Default)]
struct ProjectExplorerWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ProjectExplorerWidget {
    type View = CustomizeUiSettingsPageView;

    fn search_terms(&self) -> &str {
        "customize ui project explorer file tree left tools panel"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<CustomizeUiSettingsPageAction>(
            "Project explorer".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.show_project_explorer)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(CustomizeUiSettingsPageAction::ProjectExplorer);
                })
                .finish(),
            Some("Adds an IDE-style project explorer / file tree to the left tools panel.".into()),
        )
    }
}

#[derive(Default)]
struct ConversationHistoryWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ConversationHistoryWidget {
    type View = CustomizeUiSettingsPageView;

    fn search_terms(&self) -> &str {
        "customize ui conversation history agent conversations left tools panel"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);

        render_body_item::<CustomizeUiSettingsPageAction>(
            "Conversation history".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*ai_settings.show_conversation_history)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(CustomizeUiSettingsPageAction::ConversationHistory);
                })
                .finish(),
            Some("Show past agent conversations in the left tools panel.".into()),
        )
    }
}

#[derive(Default)]
struct GlobalSearchWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for GlobalSearchWidget {
    type View = CustomizeUiSettingsPageView;

    fn search_terms(&self) -> &str {
        "customize ui global file search left tools panel"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<CustomizeUiSettingsPageAction>(
            "Global file search".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.show_global_search)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(CustomizeUiSettingsPageAction::GlobalSearch);
                })
                .finish(),
            Some("Adds global file search to the left tools panel.".into()),
        )
    }
}

#[derive(Default)]
struct DriveWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for DriveWidget {
    type View = CustomizeUiSettingsPageView;

    fn search_terms(&self) -> &str {
        "customize ui drive warp drive command history workflows notebooks environment variables left tools panel"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let settings = WarpDriveSettings::as_ref(app);

        render_body_item::<CustomizeUiSettingsPageAction>(
            "Drive".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*settings.enable_warp_drive)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(CustomizeUiSettingsPageAction::WarpDrive);
                })
                .finish(),
            Some(
                "Save and reuse Workflows, Notebooks, and Environment Variables from the left tools panel."
                    .into(),
            ),
        )
    }
}

#[derive(Default)]
struct CodeReviewButtonWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for CodeReviewButtonWidget {
    type View = CustomizeUiSettingsPageView;

    fn search_terms(&self) -> &str {
        "customize ui code review button tab bar"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let tab_settings = TabSettings::as_ref(app);

        render_body_item::<CustomizeUiSettingsPageAction>(
            "Show code review button".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*tab_settings.show_code_review_button)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(CustomizeUiSettingsPageAction::CodeReviewButton);
                })
                .finish(),
            Some("Adds a code review button to the tab bar.".into()),
        )
    }
}
