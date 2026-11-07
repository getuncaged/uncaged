use chrono::{DateTime, Utc};
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::radius;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::WarpTheme;
use warp_util::path::user_friendly_path;
use warpui::color::ColorU;
use warpui::elements::{
    ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DispatchEventResult,
    Element, Empty, EventHandler, Expanded, Fill as ElementFill, Flex, Highlight, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseInBehavior, MouseStateHandle, OffsetPositioning,
    ParentAnchor, ParentElement, ParentOffsetBounds, Radius, SavePosition, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::TextStyle;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{AppContext, SingletonEntity, ViewHandle};

use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::conversation::ConversationStatus;
use crate::ai::agent_conversations_model::{
    AgentConversationEntry, AgentConversationEntryId, AgentConversationProvenance,
};
use crate::appearance::Appearance;
use crate::editor::EditorView;
use crate::menu::Menu;
use crate::ui_components::icons::Icon;
use crate::ui_components::menu_button::{icon_button_with_context_menu, MenuDirection};
use crate::util::truncation::truncate_from_end;
use crate::workspace::view::conversation_list::view::ConversationListViewAction;

/// Maximum length for tooltip text before truncation
const MAX_TOOLTIP_LENGTH: usize = 80;

/// Spacing between icon and title
const ICON_SPACING: f32 = 4.;

/// Uncaged: diameter of the status dot, and the leading slot that keeps titles aligned whether
/// or not a row has one.
const STATUS_DOT_SIZE: f32 = 6.;
const STATUS_SLOT_WIDTH: f32 = 14.;

/// Generate a position ID for a conversation list item
fn conversation_item_position_id(id: &AgentConversationEntryId) -> String {
    match id {
        AgentConversationEntryId::Conversation(conv_id) => {
            format!("conversation_list_item_{conv_id}")
        }
        AgentConversationEntryId::AmbientRun(task_id) => {
            format!("conversation_list_task_{task_id}")
        }
    }
}

/// Uncaged: every row in the list (conversations, section headers, "View all") is one line of
/// this height. The uniform list sizes all rows from the first, so they have to match.
pub const LIST_ROW_HEIGHT: f32 = 30.;

#[derive(Clone, Default)]
pub struct ItemState {
    pub mouse_state: MouseStateHandle,
    pub title_mouse_state: MouseStateHandle,
    pub overflow_button_state: MouseStateHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OverflowMenuDisplay {
    Closed,
    /// Menu was opened from the kebab button.
    OpenAtKebab,
    /// Menu was opened from a right click (at the click position).
    OpenAtRightClickPosition,
}

pub struct ItemProps<'a> {
    pub conversation: &'a AgentConversationEntry,
    pub highlight_indices: Option<&'a Vec<usize>>,
    pub is_selected: bool,
    pub is_focused_conversation: bool,
    pub index: usize,
    pub state: &'a ItemState,
    pub overflow_menu: &'a ViewHandle<Menu<ConversationListViewAction>>,
    pub overflow_menu_display: OverflowMenuDisplay,
    pub conversation_id: AgentConversationEntryId,
    pub is_renaming: bool,
    pub can_rename: bool,
    pub rename_editor: Option<&'a ViewHandle<EditorView>>,
    pub list_position_id: &'a str,
    pub tooltip_opens_right: bool,
}

pub fn render_item(props: ItemProps<'_>, app: &AppContext) -> Box<dyn Element> {
    let ItemProps {
        conversation,
        highlight_indices,
        is_selected,
        is_focused_conversation,
        index,
        state,
        overflow_menu,
        overflow_menu_display,
        conversation_id,
        is_renaming,
        can_rename,
        rename_editor,
        list_position_id,
        tooltip_opens_right,
    } = props;
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let ui_builder = appearance.ui_builder().clone();
    let font_family = appearance.ui_font_family();
    let font_size = appearance.ui_font_size();

    let title_font_size = font_size + 1.;
    let mut title_text = Text::new_inline(
        conversation.display.title.clone(),
        font_family,
        title_font_size,
    )
    .with_color(theme.main_text_color(theme.background()).into());

    if let Some(indices) = highlight_indices {
        if !indices.is_empty() {
            let highlight = Highlight::new()
                .with_properties(Properties::default().weight(Weight::Bold))
                .with_text_style(
                    TextStyle::new()
                        .with_foreground_color(theme.main_text_color(theme.background()).into())
                        .with_background_color(
                            internal_colors::accent_overlay_3(theme).into_solid(),
                        ),
                );
            title_text = title_text.with_single_highlight(highlight, indices.clone());
        }
    }

    let title_element: Box<dyn Element> =
        if let Some(rename_editor) = rename_editor.filter(|_| is_renaming) {
            render_inline_rename_editor(rename_editor, appearance)
        } else {
            title_text.finish()
        };
    let title_element = if can_rename && !is_renaming {
        let title_mouse_state = state.title_mouse_state.clone();
        Hoverable::new(title_mouse_state, move |_| title_element)
            .on_double_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(ConversationListViewAction::StartRename {
                    id: conversation_id,
                });
            })
            .with_cursor(Cursor::PointingHand)
            .finish()
    } else {
        title_element
    };

    // Uncaged: one line per conversation: a status dot only while something needs attention,
    // the title, and a compact age on the right. The directory and status read out in the
    // tooltip instead of a second line.
    let status = conversation.display.status.to_conversation_status();
    let status_dot = status_dot_color(&status, theme).map(|color| {
        ConstrainedBox::new(
            Container::new(Empty::new().finish())
                .with_background(color)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(STATUS_DOT_SIZE / 2.)))
                .finish(),
        )
        .with_width(STATUS_DOT_SIZE)
        .with_height(STATUS_DOT_SIZE)
        .finish()
    });
    let status_slot = ConstrainedBox::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_children(status_dot)
            .finish(),
    )
    .with_width(STATUS_SLOT_WIDTH)
    .finish();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(status_slot)
        .with_child(Expanded::new(1.0, title_element).finish());
    if is_cloud_run(conversation) {
        row.add_child(
            Container::new(
                ConstrainedBox::new(
                    Icon::Cloud
                        .to_warpui_icon(theme.sub_text_color(theme.background()))
                        .finish(),
                )
                .with_width(font_size)
                .with_height(font_size)
                .finish(),
            )
            .with_margin_left(ICON_SPACING)
            .finish(),
        );
    }
    row.add_child(
        Container::new(
            Text::new_inline(
                compact_age(conversation.display.last_updated),
                font_family,
                font_size - 1.,
            )
            .with_color(theme.sub_text_color(theme.background()).into())
            .finish(),
        )
        .with_margin_left(8.)
        .finish(),
    );
    let row = row.finish();

    let can_open = conversation.capabilities.can_open;
    let mut tooltip_text = truncate_from_end(&conversation.display.title, MAX_TOOLTIP_LENGTH);
    let details = format_item_subtext(conversation, app)
        .into_iter()
        .chain(status_dot_color(&status, theme).map(|_| status.to_string()))
        .collect::<Vec<_>>();
    if !details.is_empty() {
        tooltip_text = format!("{tooltip_text}\n{}", details.join(" · "));
    }
    let overflow_button_state = state.overflow_button_state.clone();
    let hoverable = Hoverable::new(state.mouse_state.clone(), move |_| {
        // Uncaged: the row keeps its height (the uniform list relies on it) but splits it so
        // the text is centred in its highlight and adjacent highlights don't run together.
        let container = Container::new(row)
            .with_horizontal_padding(8.)
            .with_margin_top(1.)
            .with_margin_bottom(1.)
            .with_margin_left(4.)
            .with_margin_right(4.)
            .with_corner_radius(radius::small());

        let container = if is_focused_conversation {
            container.with_background(theme.surface_overlay_2())
        } else if is_selected || !matches!(overflow_menu_display, OverflowMenuDisplay::Closed) {
            container.with_background(theme.surface_overlay_1())
        } else {
            container
        };

        let mut stack = Stack::new().with_child(container.finish());

        // We show the overflow menu button when the item is selected, or the overflow menu is already open.
        if !is_renaming
            && (is_selected || !matches!(overflow_menu_display, OverflowMenuDisplay::Closed))
        {
            let button_style = UiComponentStyles::default()
                .set_background(theme.surface_2().into())
                .set_border_color(theme.surface_3().into());
            let menu_direction = if tooltip_opens_right {
                MenuDirection::Right
            } else {
                MenuDirection::Left
            };
            let overflow_button = icon_button_with_context_menu(
                Icon::DotsVertical,
                move |ctx, _, _| {
                    ctx.dispatch_typed_action(ConversationListViewAction::ToggleOverflowMenu {
                        conversation_id,
                        position: None,
                    });
                },
                overflow_button_state.clone(),
                overflow_menu,
                matches!(overflow_menu_display, OverflowMenuDisplay::OpenAtKebab),
                menu_direction,
                Some(Cursor::PointingHand),
                Some(button_style),
                appearance,
            );
            // The kebab button is pinned to the right edge of the item regardless of which
            // side of the screen the conversation list panel is on; only the menu's open
            // direction (handled above) flips with `tooltip_opens_right`.
            let overflow_offset = OffsetPositioning::offset_from_parent(
                vec2f(-6., 0.),
                ParentOffsetBounds::ParentByPosition,
                ParentAnchor::MiddleRight,
                ChildAnchor::MiddleRight,
            );
            // Use add_positioned_child (not overlay) so button stays within item bounds
            stack.add_positioned_child(overflow_button.finish(), overflow_offset);
        }

        // Hide the tooltip when the overflow menu is being shown so that they don't overlap.
        if !is_renaming
            && is_selected
            && matches!(overflow_menu_display, OverflowMenuDisplay::Closed)
        {
            let tooltip = ui_builder.tool_tip(tooltip_text).build().finish();
            let (parent_anchor, child_anchor, offset_x) = if tooltip_opens_right {
                (ParentAnchor::MiddleRight, ChildAnchor::MiddleLeft, 4.)
            } else {
                (ParentAnchor::MiddleLeft, ChildAnchor::MiddleRight, -4.)
            };
            let tooltip_offset = OffsetPositioning::offset_from_parent(
                vec2f(offset_x, 0.),
                ParentOffsetBounds::WindowByPosition,
                parent_anchor,
                child_anchor,
            );
            stack.add_positioned_overlay_child(tooltip, tooltip_offset);
        }
        stack.finish()
    })
    .on_right_click({
        let list_position_id = list_position_id.to_string();
        move |ctx, _, position| {
            let Some(parent_bounds) = ctx.element_position_by_id(&list_position_id) else {
                log::warn!("Could not retrieve the position of the conversation list for overflow menu display.");
                return;
            };

            let offset = position - parent_bounds.origin();
            ctx.dispatch_typed_action(ConversationListViewAction::ToggleOverflowMenu {
                conversation_id,
                position: Some(offset),
            });
        }
    })
    .with_defer_events_to_children();

    let hoverable_element = if can_open && !is_renaming {
        hoverable
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(ConversationListViewAction::OpenItem {
                    id: conversation_id,
                });
            })
            .finish()
    } else {
        hoverable.finish()
    };

    let event_handler = EventHandler::new(hoverable_element)
        .on_mouse_in(
            move |ctx, _, _| {
                ctx.dispatch_typed_action(ConversationListViewAction::SetSelectedIndex(index));
                DispatchEventResult::PropagateToParent
            },
            Some(MouseInBehavior {
                fire_on_synthetic_events: false,
                // `fire_when_covered: false` makes hover ignore the row when
                // it's covered by an overlay (e.g. a modal), so the
                // selection/tooltip don't leak through — matching clicks.
                fire_when_covered: false,
            }),
        )
        .finish();

    let position_id = conversation_item_position_id(&conversation_id);

    ConstrainedBox::new(SavePosition::new(event_handler, &position_id).finish())
        .with_height(LIST_ROW_HEIGHT)
        .finish()
}

/// Uncaged: the dot colour for statuses worth a glance; finished and cancelled get none.
fn status_dot_color(status: &ConversationStatus, theme: &WarpTheme) -> Option<ColorU> {
    match status {
        ConversationStatus::InProgress | ConversationStatus::WaitingForEvents => {
            Some(theme.ansi_fg_magenta())
        }
        ConversationStatus::TransientError | ConversationStatus::Blocked { .. } => {
            Some(theme.ansi_fg_yellow())
        }
        ConversationStatus::Error => Some(theme.ansi_fg_red()),
        ConversationStatus::Success | ConversationStatus::Cancelled => None,
    }
}

fn is_cloud_run(conversation: &AgentConversationEntry) -> bool {
    matches!(
        conversation.provenance,
        AgentConversationProvenance::AmbientRun
    ) || conversation.backing.has_ambient_run
        || conversation.identity.ambient_agent_task_id.is_some()
}

/// Uncaged: a short age for the right edge of a row ("now", "5m", "3h", "2d", "3w", "4mo", "1y").
fn compact_age(datetime: DateTime<Utc>) -> String {
    let minutes = (Utc::now() - datetime).num_minutes().max(0);
    let hours = minutes / 60;
    let days = hours / 24;
    match () {
        _ if minutes < 1 => "now".to_owned(),
        _ if hours < 1 => format!("{minutes}m"),
        _ if days < 1 => format!("{hours}h"),
        _ if days < 7 => format!("{days}d"),
        _ if days < 30 => format!("{}w", days / 7),
        _ if days < 365 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

fn render_inline_rename_editor(
    rename_editor: &ViewHandle<EditorView>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    TextInput::new(
        rename_editor.clone(),
        UiComponentStyles::default()
            .set_background(ElementFill::None)
            .set_border_radius(CornerRadius::with_all(Radius::Pixels(0.)))
            .set_border_width(0.)
            .set_font_size(appearance.ui_font_size() + 1.),
    )
    .build()
    .finish()
}
/// Returns the secondary label for a conversation list item:
/// - For local conversations: the working directory.
/// - For tasks: the source (Linear, Slack, CLI, etc.)
fn format_item_subtext(conversation: &AgentConversationEntry, app: &AppContext) -> Option<String> {
    if matches!(
        conversation.provenance,
        AgentConversationProvenance::AmbientRun
    ) {
        return conversation
            .display
            .source
            .as_ref()
            .map(|source| source.display_name().to_string());
    }

    let live_pwd = conversation
        .identity
        .local_conversation_id
        .and_then(|conversation_id| {
            ActiveAgentViewsModel::as_ref(app)
                .get_active_session_for_conversation(conversation_id, app)
                .and_then(|session| session.as_ref(app).current_working_directory().cloned())
        });

    let pwd = live_pwd.or_else(|| conversation.display.working_directory.clone());
    pwd.map(|pwd| {
        let home_dir = dirs::home_dir().and_then(|p| p.to_str().map(String::from));
        user_friendly_path(&pwd, home_dir.as_deref()).into_owned()
    })
}
