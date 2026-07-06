//! The Skills panel content — lists the agent skills installed in the user's
//! home provider directories (`~/.agents/skills`, `~/.claude/skills`,
//! `~/.codex/skills`, …).
//!
//! Rendered inline by the left tool-panel (see `left_panel.rs`). It scans the
//! provider directories directly (rather than going through the async
//! `SkillManager` index) so the list is populated deterministically the moment
//! the panel is shown. Read-only: skills are managed by dropping skill folders
//! into the provider directories. Fully local — no cloud, no account.

use std::path::PathBuf;

use ai::skills::SKILL_PROVIDER_DEFINITIONS;
use warpui::elements::{
    ClippedScrollStateHandle, ClippedScrollable, Container, Element, Fill, Flex, MainAxisSize,
    ParentElement, ScrollbarWidth, Text,
};

use crate::appearance::Appearance;

/// Scans the home provider directories and returns the names of installed
/// skills. A skill is any subdirectory of a provider's `skills` folder that
/// contains a `SKILL.md`. De-duplicated and sorted, since the same skill is
/// commonly symlinked across several providers.
pub fn read_installed_skills() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };

    let mut names: Vec<String> = Vec::new();
    for definition in SKILL_PROVIDER_DEFINITIONS.iter() {
        let provider_dir = home.join(&definition.skills_path);
        let Ok(entries) = std::fs::read_dir(&provider_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // A skill lives in `<provider>/<name>/SKILL.md`.
            if !entry.path().join("SKILL.md").is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !names.iter().any(|n| *n == name) {
                names.push(name);
            }
        }
    }

    names.sort_by_key(|n| n.to_lowercase());
    names
}

fn skill_row(name: &str, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let name_color = theme.main_text_color(theme.surface_1()).into_solid();

    // Just the skill name — no per-row icon.
    let label = Text::new_inline(name.to_string(), appearance.ui_font_family(), 12.)
        .with_color(name_color)
        .finish();

    Container::new(label)
        .with_padding_left(12.)
        .with_vertical_padding(5.)
        .finish()
}

/// Renders the Skills panel's content: a scrollable list of installed skill
/// names, or an empty hint when none are found.
pub fn render_skills_content(
    skills: &[String],
    scroll_state: &ClippedScrollStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    if skills.is_empty() {
        return Container::new(
            Text::new_inline(
                "No skills installed yet.".to_string(),
                appearance.ui_font_family(),
                12.,
            )
            .with_color(theme.nonactive_ui_text_color().into_solid())
            .finish(),
        )
        .with_uniform_padding(12.)
        .finish();
    }

    let mut column = Flex::column().with_main_axis_size(MainAxisSize::Min);
    for name in skills {
        column.add_child(skill_row(name, appearance));
    }
    let body = Container::new(column.finish()).with_padding_top(4.).finish();

    // The list can be arbitrarily long, so bound it to the panel height and
    // scroll — an unbounded `Flex::column` here paints at infinite height.
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
