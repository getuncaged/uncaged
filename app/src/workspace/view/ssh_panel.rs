//! The SSH hosts panel content — lists hosts from the user's `~/.ssh/config`
//! and connects to one in a new terminal on click.
//!
//! Rendered inline by the left tool-panel (see `left_panel.rs`), so there's no
//! separate view/state machinery. Fully local: it only reads `~/.ssh/config`;
//! the sole network activity is the `ssh` connection the user starts. No cloud,
//! no account.

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

/// Reads host aliases from `~/.ssh/config`. Skips pattern hosts (`*`, `?`) and
/// negations, which aren't directly connectable.
pub fn read_ssh_hosts() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(home.join(".ssh/config")) else {
        return Vec::new();
    };
    let mut hosts = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        // `Host` keyword is case-insensitive in ssh_config.
        let Some(rest) = line
            .strip_prefix("Host ")
            .or_else(|| line.strip_prefix("host "))
        else {
            continue;
        };
        for name in rest.split_whitespace() {
            if name.contains('*') || name.contains('?') || name.starts_with('!') {
                continue;
            }
            if !hosts.iter().any(|h| h == name) {
                hosts.push(name.to_string());
            }
        }
    }
    hosts
}

fn host_row(host: &str, mouse_state: MouseStateHandle, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font_family = appearance.ui_font_family();
    let text_color = theme.main_text_color(theme.surface_1()).into_solid();
    let icon_fill = theme.nonactive_ui_text_color();
    let hover_bg = theme.surface_2().into_solid();
    let host_owned = host.to_string();
    let host_for_click = host.to_string();

    Hoverable::new(mouse_state, move |state| {
        // Fixed icon size: an unconstrained icon fills the scroll's infinite
        // measurement height and makes the centered row paint at infinite Y.
        let icon = ConstrainedBox::new(Icon::Link.to_warpui_icon(icon_fill).finish())
            .with_width(16.)
            .with_height(16.)
            .finish();
        let label = Text::new_inline(host_owned.clone(), ui_font_family, 12.)
            .with_color(text_color)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(8.)
            .with_child(Container::new(icon).with_margin_left(6.).finish())
            .with_child(label)
            .finish();
        let mut container = Container::new(row).with_vertical_padding(6.);
        if state.is_hovered() {
            container = container.with_background_color(hover_bg);
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::ConnectSsh {
            host: host_for_click.clone(),
        })
    })
    .finish()
}

/// A clickable "Add host…" row that opens `~/.ssh/config` in an editor tab.
fn add_host_row(mouse_state: MouseStateHandle, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font_family = appearance.ui_font_family();
    let text_color = theme.nonactive_ui_text_color().into_solid();
    let icon_fill = theme.nonactive_ui_text_color();
    let hover_bg = theme.surface_2().into_solid();

    Hoverable::new(mouse_state, move |state| {
        let icon = ConstrainedBox::new(Icon::Plus.to_warpui_icon(icon_fill).finish())
            .with_width(16.)
            .with_height(16.)
            .finish();
        let label = Text::new_inline("Add host…".to_string(), ui_font_family, 12.)
            .with_color(text_color)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(8.)
            .with_child(Container::new(icon).with_margin_left(6.).finish())
            .with_child(label)
            .finish();
        let mut container = Container::new(row).with_vertical_padding(6.);
        if state.is_hovered() {
            container = container.with_background_color(hover_bg);
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(WorkspaceAction::OpenSshConfig))
    .finish()
}

/// Renders the SSH panel's content: an "Add host…" row followed by a list of
/// hosts (each opens `ssh <host>` in a new terminal on click). `row_states`
/// must be at least as long as `hosts`.
pub fn render_ssh_content(
    hosts: &[String],
    row_states: &[MouseStateHandle],
    add_button_state: &MouseStateHandle,
    scroll_state: &ClippedScrollStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    let mut column = Flex::column().with_main_axis_size(MainAxisSize::Min);
    column.add_child(add_host_row(add_button_state.clone(), appearance));
    if hosts.is_empty() {
        column.add_child(
            Container::new(
                Text::new_inline(
                    "No hosts in ~/.ssh/config yet.".to_string(),
                    appearance.ui_font_family(),
                    12.,
                )
                .with_color(theme.nonactive_ui_text_color().into_solid())
                .finish(),
            )
            .with_uniform_padding(12.)
            .finish(),
        );
    }
    for (i, host) in hosts.iter().enumerate() {
        let state = row_states.get(i).cloned().unwrap_or_default();
        column.add_child(host_row(host, state, appearance));
    }
    let body = Container::new(column.finish()).with_padding_top(4.).finish();

    // Bound the list to the panel height and scroll — an unbounded
    // `Flex::column` here paints at infinite height and panics.
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
