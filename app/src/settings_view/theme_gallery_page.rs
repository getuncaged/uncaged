//! Uncaged: "Explore themes" — every theme in one grid.
//!
//! Shows what ships with the app, what has been downloaded, what the user made, and what the
//! community gallery offers, with search and a filter across all four. The grid itself is
//! [`ThemeExplorerBody`], hosted here as a child view; this page exists to give it a home in
//! Settings and to raise toasts, which belong to the window rather than the view.

use uncaged_ui::elements::Element;
use uncaged_ui::platform::SystemTheme;
use uncaged_ui::presenter::ChildView;
use uncaged_ui::{
    AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::settings_page::{
    MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget,
};
use super::SettingsSection;
use crate::appearance::Appearance;
use crate::themes::theme_explorer_body::{ThemeExplorerBody, ThemeExplorerBodyEvent};
use crate::view_components::DismissibleToast;
use crate::workspace::{ToastStack, WorkspaceAction};

pub enum ThemeGalleryPageEvent {}

/// The page has no actions of its own — every control belongs to the hosted grid.
#[derive(Debug, Clone)]
pub enum ThemeGalleryPageAction {}

pub struct ThemeGalleryPageView {
    page: PageType<Self>,
    body: ViewHandle<ThemeExplorerBody>,
}

impl ThemeGalleryPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let body = ctx.add_typed_action_view(ThemeExplorerBody::new);
        ctx.subscribe_to_view(&body, |me, _, event, ctx| {
            me.handle_body_event(event, ctx);
        });

        Self {
            // The page owns its scrolling: a grid of themes is taller than any window, and the
            // body renders no scrollable of its own.
            page: PageType::new_monolith(ThemeGalleryWidget, Some("Explore themes"), true),
            body,
        }
    }

    /// Opens or closes picking mode: choosing the light or dark theme for "Sync with OS"
    /// instead of the theme in effect.
    pub fn set_picking(&mut self, picking: Option<SystemTheme>, ctx: &mut ViewContext<Self>) {
        self.body
            .update(ctx, |body, ctx| body.set_picking(picking, ctx));
    }

    fn handle_body_event(&mut self, event: &ThemeExplorerBodyEvent, ctx: &mut ViewContext<Self>) {
        // The explorer applies, installs and deletes on its own — it holds the state those need.
        // What it cannot do is raise a toast, which belongs to the window, or navigate.
        match event {
            ThemeExplorerBodyEvent::ShowErrorToast { message } => {
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
            // A pick only ever starts from Appearance's Theme card, so that is where it ends —
            // where the newly chosen theme shows in its slot. Deferred, because this runs while
            // the explorer is still mid-update and navigating updates the settings view above it.
            ThemeExplorerBodyEvent::PickFinished => {
                ctx.dispatch_typed_action_deferred(WorkspaceAction::ShowSettingsPage(
                    SettingsSection::Appearance,
                ));
            }
        }
    }
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

    fn on_page_selected(&mut self, _allow_steal_focus: bool, ctx: &mut ViewContext<Self>) {
        // The catalogue and the card images are only looked at once the gallery is actually
        // shown, so building Settings — at startup, in every window — costs no network or disk.
        self.body.update(ctx, |body, ctx| body.activate(ctx));
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
        "explore themes theme gallery picker choose switch change browse download install delete \
         community system yours marketplace preview"
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
