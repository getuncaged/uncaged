use pathfinder_color::ColorU;
use ui_components::{button, Component as _, Options as _};
use warp_core::send_telemetry_from_ctx;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warpui_core::elements::shimmering_text::{
    ShimmerConfig, ShimmeringTextElement, ShimmeringTextStateHandle,
};
use warpui_core::elements::{
    Align, ConstrainedBox, Container, CrossAxisAlignment, Flex, FormattedTextElement,
    MainAxisAlignment, MainAxisSize, ParentElement,
};
use warpui_core::keymap::Keystroke;
use warpui_core::text_layout::TextAlignment;
use warpui_core::{
    AppContext, Element, Entity, ModelHandle, SingletonEntity as _, TypedActionView, View,
    ViewContext,
};

use super::OnboardingSlide;
use crate::model::OnboardingStateModel;
use crate::OnboardingEvent;

#[derive(Clone, Debug)]
pub enum IntroSlideEvent {
    LoginRequested,
}

#[derive(Clone, Debug)]
pub enum IntroSlideAction {
    GetStartedClicked,
    LoginClicked,
}

pub struct IntroSlide {
    onboarding_state: ModelHandle<OnboardingStateModel>,
    get_started_button: button::Button,
    shimmering_title_handle: ShimmeringTextStateHandle,
}

impl IntroSlide {
    pub(crate) fn new(onboarding_state: ModelHandle<OnboardingStateModel>) -> Self {
        Self {
            onboarding_state,
            get_started_button: button::Button::default(),
            shimmering_title_handle: ShimmeringTextStateHandle::new(),
        }
    }
}

impl Entity for IntroSlide {
    type Event = IntroSlideEvent;
}

impl View for IntroSlide {
    fn ui_name() -> &'static str {
        "IntroSlide"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let content = self.render_centered_content(appearance);
        let constrained = ConstrainedBox::new(content).with_max_width(421.).finish();
        // Background is rendered by the parent onboarding view (including background images).
        Container::new(Align::new(constrained).finish()).finish()
    }
}

impl IntroSlide {
    fn get_started_clicked(&mut self, ctx: &mut ViewContext<Self>) {
        send_telemetry_from_ctx!(OnboardingEvent::GetStartedClicked, ctx);

        self.onboarding_state.update(ctx, |model, ctx| {
            model.next(ctx);
        });
    }
}

impl OnboardingSlide for IntroSlide {
    fn on_enter(&mut self, _ctx: &mut ViewContext<Self>) {
        // Uncaged: Welcome is a real first slide — wait for the user to click
        // "Get started" instead of auto-advancing straight past it.
    }
}

impl IntroSlide {
    fn render_centered_content(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();

        // The Uncaged mark, in the ember brand accent (was a 20%-opacity ghost).
        let logo_fill = internal_colors::accent(theme);
        let logo = ConstrainedBox::new(Icon::Oz.to_warpui_icon(logo_fill).finish())
            .with_width(64.)
            .with_height(64.)
            .finish();

        // Start the shimmer from a readable secondary tone (was a 20%-opacity
        // ghost) so the title is legible at rest, then sweeps to full ink.
        let base_color: ColorU =
            internal_colors::text_sub(theme, theme.background().into_solid()).into();
        let shimmer_color: ColorU = theme.foreground().into();
        let title = ShimmeringTextElement::new(
            "Welcome to Uncaged",
            appearance.ui_font_family(),
            32.,
            base_color,
            shimmer_color,
            ShimmerConfig::default(),
            self.shimmering_title_handle.clone(),
        )
        .finish();

        let subtitle_color = internal_colors::text_sub(theme, theme.background().into_solid());
        let subtitle = FormattedTextElement::from_str(
            "A free, account-free terminal. Bring your own model.",
            appearance.ui_font_family(),
            16.,
        )
        .with_color(subtitle_color)
        .with_alignment(TextAlignment::Center)
        .with_line_height_ratio(1.0)
        .finish();

        let enter = Keystroke::parse("enter").unwrap_or_default();
        let get_started_button = self.get_started_button.render(
            appearance,
            button::Params {
                content: button::Content::Label("Get started".into()),
                theme: &button::themes::Primary,
                options: button::Options {
                    keystroke: Some(enter),
                    on_click: Some(Box::new(|ctx, _app, _pos| {
                        ctx.dispatch_typed_action(IntroSlideAction::GetStartedClicked);
                    })),
                    ..button::Options::default(appearance)
                },
            },
        );

        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(logo)
            .with_child(title)
            .with_child(Container::new(subtitle).with_margin_top(12.).finish())
            .with_child(
                Container::new(get_started_button)
                    .with_margin_top(24.)
                    .finish(),
            )
            .finish()
    }
}

impl TypedActionView for IntroSlide {
    type Action = IntroSlideAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            IntroSlideAction::GetStartedClicked => {
                self.get_started_clicked(ctx);
            }
            IntroSlideAction::LoginClicked => {
                send_telemetry_from_ctx!(OnboardingEvent::WelcomeLoginClicked, ctx);
                ctx.emit(IntroSlideEvent::LoginRequested);
            }
        }
    }
}
