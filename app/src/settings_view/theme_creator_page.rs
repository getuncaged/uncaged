//! Uncaged: a dedicated, full settings page for building a custom theme.
//!
//! Theme creation used to live in a small modal that only offered "generate a
//! theme from an image" — which is useless if you don't happen to have an image.
//! This page instead starts from the current Uncaged palette and lets you edit
//! it directly: every colour, the background gradient, opacity, the 16 terminal
//! colours, and an optional background image.
//!
//! The editor itself is [`ThemeCreatorBody`], hosted here as a child view so the
//! exact same widget can be reused rather than duplicated. This page owns the
//! side effects the modal used to own: applying a saved theme, opening the image
//! file picker, and surfacing errors.

use settings::Setting as _;
use warpui::elements::Element;
use warpui::platform::{FilePickerConfiguration, FileType};
use warpui::presenter::ChildView;
use warpui::{
    AppContext, Entity, SingletonEntity, TypedActionView, UpdateModel, View, ViewContext,
    ViewHandle,
};

use super::settings_page::{
    MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget,
};
use super::SettingsSection;
use crate::appearance::Appearance;
use crate::settings::ThemeSettings;
use crate::themes::theme_creator_body::{
    ThemeCreatorBody, ThemeCreatorBodyAction, ThemeCreatorBodyEvent,
};
use crate::user_config::{load_theme_configs, themes_dir, WarpConfig};
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;
use crate::report_if_error;

/// This page emits no events; the empty enum satisfies the `Entity` bound.
pub enum ThemeCreatorPageEvent {}

/// The page has no actions of its own — every control belongs to the hosted
/// [`ThemeCreatorBody`], which dispatches its own typed actions.
#[derive(Debug, Clone)]
pub enum ThemeCreatorPageAction {}

pub struct ThemeCreatorPageView {
    page: PageType<Self>,
    body: ViewHandle<ThemeCreatorBody>,
}

impl ThemeCreatorPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let body = ctx.add_typed_action_view(ThemeCreatorBody::new);
        ctx.subscribe_to_view(&body, |me, _, event, ctx| {
            me.handle_body_event(event, ctx);
        });

        Self {
            page: PageType::new_monolith(
                ThemeCreatorWidget,
                Some("Create your own custom theme"),
                false,
            ),
            body,
        }
    }

    /// Seeds the editor with a fresh copy of the current palette every time the
    /// page is shown, so it never displays a previous session's half-finished edits.
    pub fn on_shown(&mut self, ctx: &mut ViewContext<Self>) {
        self.body.update(ctx, |body, ctx| body.on_shown(ctx));
    }

    fn handle_body_event(&mut self, event: &ThemeCreatorBodyEvent, ctx: &mut ViewContext<Self>) {
        match event {
            // Saving writes the .yaml into the themes dir; reload the on-disk theme
            // config so the new theme exists, then make it the active theme.
            ThemeCreatorBodyEvent::SetCustomTheme { theme } => {
                let theme = theme.clone();
                ctx.spawn(
                    async move { load_theme_configs(&themes_dir()) },
                    move |_me, loaded_themes, ctx| {
                        ctx.update_model(&WarpConfig::handle(ctx), move |warp_config, ctx| {
                            warp_config.update_theme_config(loaded_themes, ctx);
                        });
                        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| {
                            report_if_error!(theme_settings.theme_kind.set_value(theme, ctx));
                        });
                    },
                );
            }
            ThemeCreatorBodyEvent::OpenFilePicker => self.open_image_picker(ctx),
            ThemeCreatorBodyEvent::ShowErrorToast { message } => {
                let window_id = ctx.window_id();
                let message = message.clone();
                ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    toast_stack.add_ephemeral_toast(
                        DismissibleToast::error(message),
                        window_id,
                        ctx,
                    );
                });
            }
            // On a page there is nothing to dismiss — the editor simply resets.
            ThemeCreatorBodyEvent::Close => {}
        }
    }

    fn open_image_picker(&mut self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        let body_id = self.body.id();
        ctx.open_file_picker(
            move |result, ctx| {
                let action = match result {
                    Ok(paths) => match paths.into_iter().next() {
                        Some(path) => {
                            ThemeCreatorBodyAction::HandleImageSelected(std::path::PathBuf::from(
                                path,
                            ))
                        }
                        None => ThemeCreatorBodyAction::FilePickerCancelled,
                    },
                    Err(err) => {
                        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                            toast_stack.add_ephemeral_toast(
                                DismissibleToast::error(format!("{err}")),
                                window_id,
                                ctx,
                            );
                        });
                        ThemeCreatorBodyAction::FilePickerCancelled
                    }
                };
                ctx.dispatch_typed_action_for_view(window_id, body_id, &action);
            },
            FilePickerConfiguration::new().set_allowed_file_types(vec![FileType::Image]),
        );
    }
}

impl Entity for ThemeCreatorPageView {
    type Event = ThemeCreatorPageEvent;
}

impl TypedActionView for ThemeCreatorPageView {
    type Action = ThemeCreatorPageAction;

    fn handle_action(&mut self, _action: &Self::Action, _ctx: &mut ViewContext<Self>) {}
}

impl View for ThemeCreatorPageView {
    fn ui_name() -> &'static str {
        "ThemeCreatorPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for ThemeCreatorPageView {
    fn section() -> SettingsSection {
        SettingsSection::ThemeCreator
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
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

impl From<ViewHandle<ThemeCreatorPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<ThemeCreatorPageView>) -> Self {
        SettingsPageViewHandle::ThemeCreator(view_handle)
    }
}

/// The whole page is one widget: the hosted editor.
struct ThemeCreatorWidget;

impl SettingsWidget for ThemeCreatorWidget {
    type View = ThemeCreatorPageView;

    fn search_terms(&self) -> &str {
        "theme custom colours colors create background gradient opacity blur window terminal colors \
         ansi cursor accent text background image share"
    }

    fn render(
        &self,
        view: &Self::View,
        _appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        ChildView::new(&view.body).finish()
    }
}
