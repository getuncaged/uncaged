//! Uncaged: the theme gallery — a browsable grid of themes to install.
//!
//! Everything that ships with the app stays bundled; this is purely additive, and adds the
//! community themes that live in `getuncaged/uncaged-themes`. The grid itself is
//! [`ThemeGalleryBody`], hosted here as a child view. This page owns the side effects: reloading
//! the theme config after an install so the new theme appears in the picker, and surfacing errors.

use settings::Setting as _;
use warpui::elements::Element;
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
use crate::report_if_error;
use crate::settings::ThemeSettings;
use crate::themes::theme::{CustomTheme, ThemeKind, COMMUNITY_SUBFOLDER};
use crate::themes::theme_gallery_body::{ThemeGalleryBody, ThemeGalleryBodyEvent};
use crate::user_config::{load_theme_configs, themes_dir, WarpConfig};
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;

pub enum ThemeGalleryPageEvent {}

/// The page has no actions of its own — every control belongs to the hosted grid.
#[derive(Debug, Clone)]
pub enum ThemeGalleryPageAction {}

pub struct ThemeGalleryPageView {
    page: PageType<Self>,
    body: ViewHandle<ThemeGalleryBody>,
}

impl ThemeGalleryPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let body = ctx.add_typed_action_view(ThemeGalleryBody::new);
        ctx.subscribe_to_view(&body, |me, _, event, ctx| {
            me.handle_body_event(event, ctx);
        });

        Self {
            // The page owns its scrolling: a grid of themes is taller than any window, and the
            // body renders no scrollable of its own.
            page: PageType::new_monolith(ThemeGalleryWidget, Some("Theme gallery"), true),
            body,
        }
    }

    fn handle_body_event(&mut self, event: &ThemeGalleryBodyEvent, ctx: &mut ViewContext<Self>) {
        match event {
            // The themes dir is watched, so an install would eventually be noticed on its own —
            // but reload explicitly rather than racing the watcher's debounce, so the theme can be
            // applied in the same breath.
            ThemeGalleryBodyEvent::Installed { name } => {
                let name = name.clone();
                ctx.spawn(
                    async move { load_theme_configs(&themes_dir()) },
                    move |_me, loaded_themes, ctx| {
                        ctx.update_model(&WarpConfig::handle(ctx), move |warp_config, ctx| {
                            warp_config.update_theme_config(loaded_themes, ctx);
                        });

                        let path = themes_dir()
                            .join(COMMUNITY_SUBFOLDER)
                            .join(format!("{}.yaml", slug_of(&name)));
                        let theme = ThemeKind::Custom(CustomTheme::new(name.clone(), path));

                        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| {
                            report_if_error!(theme_settings.theme_kind.set_value(theme, ctx));
                        });
                    },
                );
            }
            ThemeGalleryBodyEvent::ShowErrorToast { message } => {
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
        }
    }
}

/// Mirrors the slug the gallery installed the theme under.
///
/// The catalogue's slug is already a file stem, and this reproduces it from the display name the
/// same way the editor does, so the two stay in step.
fn slug_of(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

impl Entity for ThemeGalleryPageView {
    type Event = ThemeGalleryPageEvent;
}

impl TypedActionView for ThemeGalleryPageView {
    type Action = ThemeGalleryPageAction;

    fn handle_action(&mut self, _action: &Self::Action, _ctx: &mut ViewContext<Self>) {}
}

impl View for ThemeGalleryPageView {
    fn ui_name() -> &'static str {
        "ThemeGalleryPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for ThemeGalleryPageView {
    fn section() -> SettingsSection {
        SettingsSection::ThemeGallery
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

impl From<ViewHandle<ThemeGalleryPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<ThemeGalleryPageView>) -> Self {
        SettingsPageViewHandle::ThemeGallery(view_handle)
    }
}

/// The whole page is one widget: the hosted grid.
struct ThemeGalleryWidget;

impl SettingsWidget for ThemeGalleryWidget {
    type View = ThemeGalleryPageView;

    fn search_terms(&self) -> &str {
        "theme gallery browse download install community system themes marketplace preview"
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
