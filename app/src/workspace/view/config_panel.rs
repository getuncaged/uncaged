//! The Config panel content — lists the user's portable config files/dirs
//! (settings, keybindings, themes, workflows, …) under the local data dir
//! (`~/.uncaged`), and hosts Back up / Restore actions.
//!
//! Rendered inline by the left tool-panel (see `left_panel.rs`). Clicking a
//! file opens it in an editor tab; clicking a directory reveals it. Fully local
//! — the same whitelist that `settings_backup` uses. No cloud, no account.

use std::path::PathBuf;

use warpui::elements::{
    ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CrossAxisAlignment,
    Element, Fill, Flex, Hoverable, MainAxisSize, MouseStateHandle, ParentElement, ScrollbarWidth,
    Text,
};
use warpui::platform::Cursor;

use crate::appearance::Appearance;
use crate::ui_components::icons::Icon;
use crate::WorkspaceAction;

/// The portable config items, matching `settings_backup`'s whitelist. `(label,
/// relative path under the data dir)`.
const CONFIG_ITEMS: &[(&str, &str)] = &[
    ("Settings", "settings.toml"),
    ("Keybindings", "keybindings.yaml"),
    ("Themes", "themes"),
    ("Workflows", "workflows"),
    ("Notebooks", "notebooks"),
    ("Launch configs", "launch_configurations"),
    ("Snippets", "snippets"),
];

pub struct ConfigEntry {
    pub label: &'static str,
    pub path: PathBuf,
}

/// Returns the config items that actually exist under the data dir.
pub fn read_config_entries() -> Vec<ConfigEntry> {
    let dir = warp_core::paths::data_dir();
    CONFIG_ITEMS
        .iter()
        .filter_map(|(label, item)| {
            let path = dir.join(item);
            path.exists().then_some(ConfigEntry { label, path })
        })
        .collect()
}

/// A generic clickable row (icon + label) dispatching a `WorkspaceAction`.
fn action_row(
    icon: Icon,
    label: &str,
    action: WorkspaceAction,
    mouse_state: MouseStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font_family = appearance.ui_font_family();
    let text_color = theme.main_text_color(theme.surface_1()).into_solid();
    let icon_fill = theme.nonactive_ui_text_color();
    let hover_bg = theme.surface_2().into_solid();
    let label_owned = label.to_string();

    Hoverable::new(mouse_state, move |state| {
        let icon = ConstrainedBox::new(icon.to_warpui_icon(icon_fill).finish())
            .with_width(16.)
            .with_height(16.)
            .finish();
        let label_el = Text::new_inline(label_owned.clone(), ui_font_family, 12.)
            .with_color(text_color)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(8.)
            .with_child(Container::new(icon).with_margin_left(6.).finish())
            .with_child(label_el)
            .finish();
        let mut container = Container::new(row).with_vertical_padding(6.);
        if state.is_hovered() {
            container = container.with_background_color(hover_bg);
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
    .finish()
}

/// Renders the Config panel: each config file/dir (opens on click), then the
/// Back up / Restore actions. `row_states` must be at least as long as
/// `entries`; `action_states` supplies the trailing action rows' hover states.
pub fn render_config_content(
    entries: &[ConfigEntry],
    row_states: &[MouseStateHandle],
    action_states: &[MouseStateHandle; 4],
    scroll_state: &ClippedScrollStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    let mut column = Flex::column().with_main_axis_size(MainAxisSize::Min);
    for (i, entry) in entries.iter().enumerate() {
        let state = row_states.get(i).cloned().unwrap_or_default();
        column.add_child(action_row(
            Icon::Code2,
            entry.label,
            WorkspaceAction::OpenConfigFile {
                path: entry.path.clone(),
            },
            state,
            appearance,
        ));
    }

    column.add_child(action_row(
        Icon::Download,
        "Back up config…",
        WorkspaceAction::BackUpSettings,
        action_states[0].clone(),
        appearance,
    ));
    column.add_child(action_row(
        Icon::Import,
        "Restore config…",
        WorkspaceAction::RestoreSettings,
        action_states[1].clone(),
        appearance,
    ));
    column.add_child(action_row(
        Icon::UploadCloud,
        "Sync to gist…",
        WorkspaceAction::PushConfigToGist,
        action_states[2].clone(),
        appearance,
    ));
    column.add_child(action_row(
        Icon::CloudOffline,
        "Restore from gist…",
        WorkspaceAction::PullConfigFromGist,
        action_states[3].clone(),
        appearance,
    ));

    let body = Container::new(column.finish()).with_padding_top(4.).finish();

    ClippedScrollable::vertical(
        scroll_state.clone(),
        body,
        ScrollbarWidth::Auto,
        theme.disabled_text_color(theme.background()).into(),
        theme.main_text_color(theme.background()).into(),
        Fill::None,
    )
    .finish()
}
