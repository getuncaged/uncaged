use uncaged_ui::elements::Element;
use uncaged_ui::{AppContext, Entity, ModelHandle, View, ViewContext};

use crate::terminal::input::inline_menu::model::InlineMenuModel;
use crate::terminal::input::inline_menu::{InlineMenuAction, InlineMenuMessageProvider};
use crate::terminal::input::message_bar::common::render_standard_message_bar;
use crate::terminal::input::message_bar::{EmptyMessageProducer, MessageProvider};

pub struct InlineMenuMessageBarArgs<A: InlineMenuAction, T: 'static + Send + Sync = ()> {
    pub inline_menu_model: ModelHandle<InlineMenuModel<A, T>>,
}

/// Renders contextual hint text at the bottom of the agent view status bar.
pub struct InlineMenuMessageBar<A: InlineMenuAction, T: 'static + Send + Sync = ()> {
    inline_menu_model: ModelHandle<InlineMenuModel<A, T>>,
}

impl<A: InlineMenuAction, T: 'static + Send + Sync> Entity for InlineMenuMessageBar<A, T> {
    type Event = ();
}

impl<A: InlineMenuAction, T: 'static + Send + Sync> InlineMenuMessageBar<A, T> {
    pub fn new(args: InlineMenuMessageBarArgs<A, T>, ctx: &mut ViewContext<Self>) -> Self {
        let InlineMenuMessageBarArgs { inline_menu_model } = args;

        ctx.subscribe_to_model(&inline_menu_model, |_, _, _, ctx| {
            ctx.notify();
        });

        Self { inline_menu_model }
    }
}

pub const INLINE_MENU_BORDER_WIDTH: f32 = 1.;

impl<A: InlineMenuAction, T: 'static + Send + Sync> View for InlineMenuMessageBar<A, T> {
    fn ui_name() -> &'static str {
        "InlineMenuMessageBar"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let inline_menu_model = self.inline_menu_model.as_ref(app);

        let message = InlineMenuMessageProvider::<A>::default()
            .produce_message(InlineMenuMessageArgs {
                inline_menu_model,
                app,
            })
            .or_else(|| {
                EmptyMessageProducer.produce_message(InlineMenuMessageArgs {
                    inline_menu_model,
                    app,
                })
            })
            .expect("Empty message producer always returns Some().");

        // Uncaged: the inline menu is a rounded card whose own border closes this edge.
        render_standard_message_bar(message, None, app)
    }
}

/// Arguments for inline menu message producers.
#[derive(Copy, Clone)]
pub struct InlineMenuMessageArgs<'a, A: InlineMenuAction, T = ()> {
    pub inline_menu_model: &'a InlineMenuModel<A, T>,
    pub app: &'a AppContext,
}
