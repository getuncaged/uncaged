use ui_components::{button, Component as _, Options as _};
use warp_core::ui::appearance::Appearance;
use warp_core::ui::icons::Icon;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::Fill;
use warpui_core::elements::{
    Border, ClippedScrollStateHandle, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    Empty, Flex, FormattedTextElement, MainAxisSize, ParentElement, Radius, Shrinkable,
};
use warpui_core::fonts::Weight;
use warpui_core::keymap::Keystroke;
use warpui_core::prelude::Align;
use warpui_core::text_layout::TextAlignment;
use warpui_core::ui_components::components::{UiComponent as _, UiComponentStyles};
use warpui_core::{
    AppContext, Element, Entity, ModelHandle, SingletonEntity as _, TypedActionView, View,
    ViewContext,
};

use super::OnboardingSlide;
use crate::model::{AiAccessChoice, OnboardingStateModel};
use crate::slides::{bottom_nav, layout, slide_content};

// ── Connect-gallery data (plain-data mirror of the app-crate uncaged views) ──
//
// The onboarding crate cannot reference the app crate, so `app/src/root_view.rs`
// converts `crate::uncaged::{catalog_sections, connections, engine_active}` into
// these structs and pushes them into the slide via
// `AgentOnboardingView::set_connect_gallery`. The per-preset / per-connection
// vendor `icon` is stamped by root_view from `crate::uncaged::preset_icon`, so
// the branding stays in its one home.

/// One connectable platform tile in a gallery section.
#[derive(Clone, Debug)]
pub struct ConnectPresetView {
    pub id: String,
    pub label: String,
    pub blurb: String,
    pub local: bool,
    pub needs_key: bool,
    /// "anthropic" | "openai_compatible" | "cli".
    pub wire: String,
    pub icon: Icon,
}

/// A gallery section grouping presets that connect the same way.
#[derive(Clone, Debug)]
pub struct ConnectSectionView {
    pub title: String,
    pub subtitle: String,
    pub presets: Vec<ConnectPresetView>,
}

/// A saved connection as the gallery's "Connected" roster sees it.
#[derive(Clone, Debug)]
pub struct ConnectConnectionView {
    pub id: String,
    pub preset: String,
    pub label: String,
    pub endpoint: String,
    pub model: String,
    /// "Ready" | "Needs key" | "Incomplete".
    pub status: String,
    pub local: bool,
    pub usable: bool,
    pub is_active: bool,
    pub icon: Icon,
}

/// The full connect gallery snapshot: catalog + roster + engine availability.
#[derive(Clone, Debug, Default)]
pub struct ConnectGalleryData {
    pub sections: Vec<ConnectSectionView>,
    pub connections: Vec<ConnectConnectionView>,
    pub engine_available: bool,
}

impl ConnectGalleryData {
    /// The number of preset tiles across all sections (the size the connect
    /// button pool must cover, in flattened section order).
    fn preset_count(&self) -> usize {
        self.sections.iter().map(|s| s.presets.len()).sum()
    }
}

#[derive(Debug, Clone)]
pub enum AiAccessSlideAction {
    /// Connect a catalog preset by id (a gallery tile's "Connect" button).
    ConnectPreset(String),
    /// Make an already-saved but inactive connection active (a roster "Use").
    UseConnection(String),
    BackClicked,
    NextClicked,
    SetUpLaterClicked,
}

/// Emitted to the parent onboarding view so the (app-crate) connect actions can
/// be handled at the root level — the onboarding crate can't reference the
/// uncaged bridge directly.
#[derive(Debug, Clone)]
pub enum AiAccessSlideEvent {
    /// The user asked to connect a catalog preset (root_view calls
    /// `uncaged::connect` + activates no-key local/CLI presets).
    ConnectPresetRequested(String),
    /// The user asked to activate an existing connection by id.
    ActivateConnectionRequested(String),
}

/// The "Connect your model" slide (built-in agent path). Renders the real
/// connect gallery — a "Connected" roster plus catalog sections of one-click
/// connectable platforms (local runtimes, CLI agents, API-key providers) — and
/// a "Set up later" link that finishes onboarding without a model.
pub struct AiAccessSlide {
    onboarding_state: ModelHandle<OnboardingStateModel>,
    back_button: button::Button,
    next_button: button::Button,
    set_up_later_button: button::Button,
    scroll_state: ClippedScrollStateHandle,
    /// The catalog + roster snapshot, pushed by root_view via the parent view.
    gallery: ConnectGalleryData,
    /// One button per preset tile, in flattened section order. Kept in lockstep
    /// with `gallery.preset_count()` so `render` can index straight into it.
    connect_buttons: Vec<button::Button>,
    /// One "Use" button per roster connection, indexed by connection position.
    use_buttons: Vec<button::Button>,
}

impl AiAccessSlide {
    pub(crate) fn new(onboarding_state: ModelHandle<OnboardingStateModel>) -> Self {
        Self {
            onboarding_state,
            back_button: button::Button::default(),
            next_button: button::Button::default(),
            set_up_later_button: button::Button::default(),
            scroll_state: ClippedScrollStateHandle::new(),
            gallery: ConnectGalleryData::default(),
            connect_buttons: Vec::new(),
            use_buttons: Vec::new(),
        }
    }

    /// Replace the connect-gallery snapshot and resize the button pools to match.
    /// Called by the parent onboarding view whenever root_view rebuilds the data
    /// (initial load and after every connect/activate).
    pub(crate) fn set_connect_gallery(
        &mut self,
        data: ConnectGalleryData,
        ctx: &mut ViewContext<Self>,
    ) {
        self.connect_buttons
            .resize_with(data.preset_count(), button::Button::default);
        self.use_buttons
            .resize_with(data.connections.len(), button::Button::default);
        self.gallery = data;
        ctx.notify();
    }

    // The final DES-816 visual exports have not landed yet, so the right panel
    // reuses the existing bundled agent welcome image.
    pub(crate) const VISUAL_IMAGE_PATHS: &'static [&'static str] =
        &["async/png/onboarding/welcome_agent.png"];

    fn render_content(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let bottom_nav = Align::new(self.render_bottom_nav(appearance, app)).finish();

        slide_content::onboarding_slide_content(
            vec![
                Align::new(self.render_header(appearance)).left().finish(),
                Align::new(self.render_gallery(appearance)).left().finish(),
                Align::new(self.render_set_up_later(appearance))
                    .left()
                    .finish(),
            ],
            bottom_nav,
            self.scroll_state.clone(),
            appearance,
        )
    }

    fn render_header(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();

        let title = appearance
            .ui_builder()
            .paragraph("Connect your model")
            .with_style(UiComponentStyles {
                font_size: Some(36.),
                font_weight: Some(Weight::Medium),
                ..Default::default()
            })
            .build()
            .finish();

        let subtitle = FormattedTextElement::from_str(
            "Use your own model — it's free — or set it up later.",
            appearance.ui_font_family(),
            16.,
        )
        .with_color(internal_colors::text_sub(
            theme,
            theme.background().into_solid(),
        ))
        .with_weight(Weight::Normal)
        .with_alignment(TextAlignment::Left)
        .with_line_height_ratio(1.0)
        .finish();

        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(title)
            .with_child(Container::new(subtitle).with_margin_top(16.).finish())
            .finish()
    }

    // ── gallery primitives (mirror ai_page.rs ConnectModelWidget) ───────────
    const GALLERY_ICON_SIZE: f32 = 20.;

    /// Row / tile title (14px semibold).
    fn tile_title(text: &str, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        appearance
            .ui_builder()
            .paragraph(text.to_string())
            .with_style(UiComponentStyles {
                font_size: Some(14.),
                font_weight: Some(Weight::Semibold),
                font_color: Some(internal_colors::text_main(
                    theme,
                    theme.background().into_solid(),
                )),
                ..Default::default()
            })
            .build()
            .finish()
    }

    /// Muted supporting text (section subtitle / tile blurb).
    fn tile_muted(text: &str, size: f32, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        FormattedTextElement::from_str(text.to_string(), appearance.ui_font_family(), size)
            .with_color(internal_colors::text_sub(
                theme,
                theme.background().into_solid(),
            ))
            .with_weight(Weight::Normal)
            .with_alignment(TextAlignment::Left)
            .with_line_height_ratio(1.2)
            .finish()
    }

    /// A pill-shaped status/state badge (10px semibold colored text on a subtle
    /// fill).
    fn pill(text: &str, text_color: Fill, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let label = appearance
            .ui_builder()
            .paragraph(text.to_string())
            .with_style(UiComponentStyles {
                font_size: Some(10.),
                font_weight: Some(Weight::Semibold),
                font_color: Some(text_color.into_solid()),
                ..Default::default()
            })
            .build()
            .finish();
        Container::new(label)
            .with_horizontal_padding(7.)
            .with_vertical_padding(3.)
            .with_background(internal_colors::fg_overlay_2(theme))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.)))
            .finish()
    }

    /// A fixed-size leading vendor icon. Icons have no intrinsic size, so they
    /// must be boxed in a ConstrainedBox (else the debug infinite-Y panic).
    fn leading_icon(icon: Icon, appearance: &Appearance) -> Box<dyn Element> {
        ConstrainedBox::new(Box::new(
            icon.to_warpui_icon(appearance.theme().active_ui_text_color()),
        ))
        .with_width(Self::GALLERY_ICON_SIZE)
        .with_height(Self::GALLERY_ICON_SIZE)
        .finish()
    }

    /// A card wrapper for a roster row / gallery tile. The active connection
    /// gets an accent border.
    fn card(child: Box<dyn Element>, accent: bool, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let border = if accent {
            theme.accent()
        } else {
            Fill::Solid(internal_colors::neutral_4(theme))
        };
        Container::new(child)
            .with_uniform_padding(12.)
            .with_background(internal_colors::fg_overlay_1(theme))
            .with_border(Border::all(1.).with_border_fill(border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .finish()
    }

    /// One "Connected" roster row: vendor icon + label/subtitle + status pill
    /// and, when inactive-but-usable, a "Use" button that activates it.
    fn render_connection_row(
        &self,
        appearance: &Appearance,
        index: usize,
        conn: &ConnectConnectionView,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();

        let mut name_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_child(Self::tile_title(&conn.label, appearance));
        if conn.is_active {
            name_row = name_row.with_child(Self::pill("Active", theme.accent(), appearance));
        }
        if conn.local {
            name_row = name_row.with_child(Self::pill(
                "Local",
                theme.nonactive_ui_text_color(),
                appearance,
            ));
        }

        let subtitle_text = if conn.model.trim().is_empty() {
            conn.endpoint.clone()
        } else {
            format!("{} · {}", conn.model, conn.endpoint)
        };

        let left = Flex::column()
            .with_spacing(3.)
            .with_child(name_row.finish())
            .with_child(Self::tile_muted(&subtitle_text, 11., appearance))
            .finish();

        let status_color = Fill::Solid(if conn.status == "Ready" {
            theme.ansi_fg_green()
        } else {
            theme.ui_warning_color()
        });
        let mut right = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_child(Self::pill(&conn.status, status_color, appearance));
        if !conn.is_active && conn.usable {
            if let Some(button) = self.use_buttons.get(index) {
                let id = conn.id.clone();
                right = right.with_child(button.render(
                    appearance,
                    button::Params {
                        content: button::Content::Label("Use".into()),
                        theme: &button::themes::Secondary,
                        options: button::Options {
                            size: button::Size::Small,
                            on_click: Some(Box::new(move |ctx, _app, _pos| {
                                ctx.dispatch_typed_action(AiAccessSlideAction::UseConnection(
                                    id.clone(),
                                ));
                            })),
                            ..button::Options::default(appearance)
                        },
                    },
                ));
            }
        }

        // Flat row: icon + shrinkable text + fixed right controls, all direct
        // children of a MainAxisSize::Max row. The Shrinkable must NOT be nested
        // in an intermediate sub-row (that sub-row measures with an infinite
        // width constraint and the flex panics).
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(11.)
            .with_child(Self::leading_icon(conn.icon, appearance))
            .with_child(Shrinkable::new(1., left).finish())
            .with_child(right.finish())
            .finish();

        Self::card(row, conn.is_active, appearance)
    }

    /// One catalog preset tile: vendor icon + label/blurb + a right slot that is
    /// a state pill when connected, else a "Connect" button.
    #[allow(clippy::too_many_arguments)]
    fn render_preset_tile(
        &self,
        appearance: &Appearance,
        button_index: usize,
        preset: &ConnectPresetView,
        is_active: bool,
        is_connected: bool,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();

        let left = Flex::column()
            .with_spacing(3.)
            .with_child(Self::tile_title(&preset.label, appearance))
            .with_child(Self::tile_muted(&preset.blurb, 11., appearance))
            .finish();

        let right: Box<dyn Element> = if is_active {
            Self::pill("In use", theme.accent(), appearance)
        } else if is_connected {
            Self::pill("Connected", Fill::Solid(theme.ansi_fg_green()), appearance)
        } else if let Some(button) = self.connect_buttons.get(button_index) {
            let id = preset.id.clone();
            button.render(
                appearance,
                button::Params {
                    content: button::Content::IconAndLabel(Icon::Plus, "Connect".into()),
                    theme: &button::themes::Secondary,
                    options: button::Options {
                        size: button::Size::Small,
                        on_click: Some(Box::new(move |ctx, _app, _pos| {
                            ctx.dispatch_typed_action(AiAccessSlideAction::ConnectPreset(
                                id.clone(),
                            ));
                        })),
                        ..button::Options::default(appearance)
                    },
                },
            )
        } else {
            Empty::new().finish()
        };

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(11.)
            .with_child(Self::leading_icon(preset.icon, appearance))
            .with_child(Shrinkable::new(1., left).finish())
            .with_child(right)
            .finish();

        Self::card(row, is_active, appearance)
    }

    /// The full connect gallery: a "Connected" roster (when non-empty) followed
    /// by each catalog section of connectable preset tiles.
    fn render_gallery(&self, appearance: &Appearance) -> Box<dyn Element> {
        let data = &self.gallery;

        // Presets already connected (so we don't re-offer "Connect"), and the
        // active preset (so its tile reads "In use").
        let connected: std::collections::HashSet<&str> =
            data.connections.iter().map(|c| c.preset.as_str()).collect();
        let active_preset: Option<&str> = data
            .connections
            .iter()
            .find(|c| c.is_active)
            .map(|c| c.preset.as_str());

        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.);

        // "Connected" roster.
        if !data.connections.is_empty() {
            column = column.with_child(
                Container::new(Self::tile_title("Connected", appearance))
                    .with_margin_bottom(2.)
                    .finish(),
            );
            for (index, conn) in data.connections.iter().enumerate() {
                column = column.with_child(self.render_connection_row(appearance, index, conn));
            }
        }

        // Catalog sections. `button_index` walks the flattened preset order in
        // lockstep with the connect-button pool, advanced unconditionally so the
        // positional pool never desyncs.
        let mut button_index = 0usize;
        for section in &data.sections {
            column = column.with_child(
                Container::new(
                    Flex::column()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.)
                        .with_child(Self::tile_title(&section.title, appearance))
                        .with_child(Self::tile_muted(&section.subtitle, 12., appearance))
                        .finish(),
                )
                .with_margin_top(8.)
                .with_margin_bottom(2.)
                .finish(),
            );

            for preset in &section.presets {
                let idx = button_index;
                button_index += 1;
                let is_active = active_preset == Some(preset.id.as_str());
                let is_connected = connected.contains(preset.id.as_str());
                column = column.with_child(self.render_preset_tile(
                    appearance,
                    idx,
                    preset,
                    is_active,
                    is_connected,
                ));
            }
        }

        Container::new(column.finish())
            .with_margin_top(32.)
            .finish()
    }

    /// A muted "Set up later" link below the gallery that finishes onboarding
    /// without a model.
    fn render_set_up_later(&self, appearance: &Appearance) -> Box<dyn Element> {
        let button = self.set_up_later_button.render(
            appearance,
            button::Params {
                content: button::Content::Label("Set up later".into()),
                theme: &button::themes::Naked,
                options: button::Options {
                    on_click: Some(Box::new(|ctx, _app, _pos| {
                        ctx.dispatch_typed_action(AiAccessSlideAction::SetUpLaterClicked);
                    })),
                    ..button::Options::default(appearance)
                },
            },
        );
        Container::new(button).with_margin_top(16.).finish()
    }

    fn render_bottom_nav(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let back_button = self.back_button.render(
            appearance,
            button::Params {
                content: button::Content::Label("Back".into()),
                theme: &button::themes::Naked,
                options: button::Options {
                    on_click: Some(Box::new(|ctx, _app, _pos| {
                        ctx.dispatch_typed_action(AiAccessSlideAction::BackClicked);
                    })),
                    ..button::Options::default(appearance)
                },
            },
        );

        let enter = Keystroke::parse("enter").unwrap_or_default();
        let next_button = self.next_button.render(
            appearance,
            button::Params {
                content: button::Content::Label("Next".into()),
                theme: &button::themes::Primary,
                options: button::Options {
                    keystroke: Some(enter),
                    on_click: Some(Box::new(|ctx, _app, _pos| {
                        ctx.dispatch_typed_action(AiAccessSlideAction::NextClicked);
                    })),
                    ..button::Options::default(appearance)
                },
            },
        );

        let (step_index, step_count) = self.onboarding_state.as_ref(app).progress();
        bottom_nav::onboarding_bottom_nav(
            appearance,
            step_index,
            step_count,
            Some(back_button),
            Some(next_button),
        )
    }

    fn render_visual(&self) -> Box<dyn Element> {
        layout::onboarding_right_panel_with_bg(
            Self::VISUAL_IMAGE_PATHS[0],
            layout::FOREGROUND_LAYOUT_DEFAULT,
        )
    }
}

impl Entity for AiAccessSlide {
    type Event = AiAccessSlideEvent;
}

impl View for AiAccessSlide {
    fn ui_name() -> &'static str {
        "AiAccessSlide"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        layout::static_left(
            || self.render_content(appearance, app),
            || self.render_visual(),
        )
    }
}

impl AiAccessSlide {
    fn next(&mut self, ctx: &mut ViewContext<Self>) {
        self.onboarding_state.update(ctx, |model, ctx| {
            model.next(ctx);
        });
    }
}

impl OnboardingSlide for AiAccessSlide {
    fn on_enter(&mut self, ctx: &mut ViewContext<Self>) {
        self.next(ctx);
    }
}

impl TypedActionView for AiAccessSlide {
    type Action = AiAccessSlideAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AiAccessSlideAction::ConnectPreset(preset_id) => {
                ctx.emit(AiAccessSlideEvent::ConnectPresetRequested(
                    preset_id.clone(),
                ));
            }
            AiAccessSlideAction::UseConnection(id) => {
                ctx.emit(AiAccessSlideEvent::ActivateConnectionRequested(id.clone()));
            }
            AiAccessSlideAction::BackClicked => {
                self.onboarding_state.update(ctx, |model, ctx| {
                    model.back(ctx);
                });
            }
            AiAccessSlideAction::NextClicked => {
                self.next(ctx);
            }
            AiAccessSlideAction::SetUpLaterClicked => {
                // Record that the user opted to finish without connecting (so the
                // post-onboarding Settings redirect is skipped), then advance.
                self.onboarding_state.update(ctx, |model, ctx| {
                    model.set_ai_access_choice(AiAccessChoice::SetUpLater, ctx);
                    model.next(ctx);
                });
            }
        }
    }
}
