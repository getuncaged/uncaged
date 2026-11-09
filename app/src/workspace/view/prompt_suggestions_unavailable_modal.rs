use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use uncaged_core::ui::appearance::Appearance;
use uncaged_core::ui::theme::Fill;
use uncaged_ui::elements::{
    Align, Border, ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DropShadow, Flex, FormattedTextElement, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, Stack,
};
use uncaged_ui::fonts::Weight;
use uncaged_ui::keymap::FixedBinding;
use uncaged_ui::platform::Cursor;
use uncaged_ui::ui_components::button::ButtonVariant;
use uncaged_ui::ui_components::components::{UiComponent, UiComponentStyles};
use uncaged_ui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext,
};

use crate::brand;
use crate::settings_view::SettingsSection;
use crate::ui_components::blended_colors;
use crate::workspace::WorkspaceAction;

const MODAL_WIDTH: f32 = 480.;
const CORNER_RADIUS: f32 = 12.;
const PANEL_PADDING: f32 = 24.;
const CLOSE_BUTTON_DIAMETER: f32 = 20.;

fn title() -> String {
    format!("How to use AI features in {}", brand::NAME)
}

fn body() -> String {
    format!(
        "To use AI features in {}, connect a model: add an API key (OpenAI, Anthropic, or \
         Google), add a custom inference endpoint (OpenRouter, LiteLLM), or log in using your \
         SuperGrok subscription.",
        brand::NAME
    )
}

pub fn init(app: &mut AppContext) {
    use uncaged_ui::keymap::macros::*;

    app.register_fixed_bindings([FixedBinding::new(
        "escape",
        PromptSuggestionsUnavailableModalAction::Close,
        id!("PromptSuggestionsUnavailableModal"),
    )]);
}

#[derive(Clone, Debug)]
pub enum PromptSuggestionsUnavailableModalAction {
    Close,
    SetUpByok,
}

#[derive(Clone, Copy, Debug)]
pub enum PromptSuggestionsUnavailableModalEvent {
    Close,
}

#[derive(Default)]
struct StateHandles {
    close_button: MouseStateHandle,
    byok_button: MouseStateHandle,
}

/// Explains how to connect a model, shown when the user activates Prompt Suggestions while no
/// model is available. Its button opens the AI settings page.
#[derive(Default)]
pub struct PromptSuggestionsUnavailableModal {
    state_handles: StateHandles,
}

impl PromptSuggestionsUnavailableModal {
    fn render_buttons(&self, appearance: &Appearance) -> Box<dyn Element> {
        let byok_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Secondary,
                self.state_handles.byok_button.clone(),
            )
            .with_style(UiComponentStyles {
                font_size: Some(14.),
                height: Some(32.),
                ..Default::default()
            })
            .with_centered_text_label("Bring your own AI".to_string())
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(PromptSuggestionsUnavailableModalAction::SetUpByok);
            })
            .finish();

        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_child(byok_button)
            .finish()
    }

    fn render_close_button(&self, appearance: &Appearance) -> Box<dyn Element> {
        appearance
            .ui_builder()
            .close_button(
                CLOSE_BUTTON_DIAMETER,
                self.state_handles.close_button.clone(),
            )
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(PromptSuggestionsUnavailableModalAction::Close)
            })
            .finish()
    }
}

impl Entity for PromptSuggestionsUnavailableModal {
    type Event = PromptSuggestionsUnavailableModalEvent;
}

impl View for PromptSuggestionsUnavailableModal {
    fn ui_name() -> &'static str {
        "PromptSuggestionsUnavailableModal"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let bg = blended_colors::neutral_1(theme);
        let font_family = appearance.ui_font_family();

        let title = FormattedTextElement::from_str(title(), font_family, 18.)
            .with_color(blended_colors::text_main(theme, bg))
            .with_weight(Weight::Bold)
            .finish();

        let body = FormattedTextElement::from_str(body(), font_family, 14.)
            .with_color(blended_colors::text_sub(theme, bg))
            .finish();

        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Container::new(title).with_margin_bottom(12.).finish())
            .with_child(Container::new(body).with_margin_bottom(20.).finish());

        let content = content.with_child(self.render_buttons(appearance)).finish();

        let mut modal = Stack::new();
        modal.add_child(
            Container::new(
                ConstrainedBox::new(content)
                    .with_width(MODAL_WIDTH)
                    .finish(),
            )
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(CORNER_RADIUS)))
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .with_uniform_padding(PANEL_PADDING)
            .with_drop_shadow(DropShadow::default())
            .finish(),
        );
        modal.add_positioned_child(
            self.render_close_button(appearance),
            OffsetPositioning::offset_from_parent(
                vec2f(-8., 8.),
                ParentOffsetBounds::ParentByPosition,
                ParentAnchor::TopRight,
                ChildAnchor::TopRight,
            ),
        );

        let mut stack = Stack::new();
        stack.add_positioned_child(
            modal.finish(),
            OffsetPositioning::offset_from_parent(
                vec2f(0., 0.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::Center,
                ChildAnchor::Center,
            ),
        );

        Container::new(Align::new(stack.finish()).finish())
            .with_background(Fill::Solid(ColorU::new(97, 97, 97, 255)).with_opacity(50))
            .finish()
    }
}

impl TypedActionView for PromptSuggestionsUnavailableModal {
    type Action = PromptSuggestionsUnavailableModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            PromptSuggestionsUnavailableModalAction::Close => {
                ctx.emit(PromptSuggestionsUnavailableModalEvent::Close);
            }
            PromptSuggestionsUnavailableModalAction::SetUpByok => {
                // Deferred so the close-driven refocus below doesn't steal focus from
                // the settings page this opens.
                ctx.dispatch_typed_action_deferred(WorkspaceAction::ShowSettingsPageWithSearch {
                    search_query: "api".to_string(),
                    section: Some(SettingsSection::WarpAgent),
                });
                ctx.emit(PromptSuggestionsUnavailableModalEvent::Close);
            }
        }
    }
}
