//! The Skills panel content — lists the agent skills installed in the user's
//! home provider directories (`~/.agents/skills`, `~/.claude/skills`,
//! `~/.codex/skills`, …).
//!
//! Rendered inline by the left tool-panel (see `left_panel.rs`). It scans the
//! provider directories directly (rather than going through the async
//! `SkillManager` index) so the list is populated deterministically the moment
//! the panel is shown. Each row shows the skill's name and (when present) the
//! one-line `description:` from its `SKILL.md` frontmatter; clicking a row
//! opens that `SKILL.md` in an editor tab so you can read the full skill.
//! Skills are managed by dropping skill folders into the provider directories.
//! Fully local — no cloud, no account.

use std::path::{Path, PathBuf};

use ai::skills::SKILL_PROVIDER_DEFINITIONS;
use warpui::elements::{
    ClippedScrollStateHandle, ClippedScrollable, Container, CrossAxisAlignment, Element, Fill, Flex,
    Hoverable, MainAxisSize, MouseStateHandle, ParentElement, ScrollbarWidth, Text,
};
use warpui::platform::Cursor;

use crate::appearance::Appearance;
use crate::WorkspaceAction;

/// A skill discovered on disk: its display name, the one-line description parsed
/// from the `SKILL.md` frontmatter (if any), and the path to that `SKILL.md`.
pub struct SkillEntry {
    pub name: String,
    pub description: Option<String>,
    pub path: PathBuf,
}

/// Scans the home provider directories and returns the installed skills. A skill
/// is any subdirectory of a provider's `skills` folder that contains a
/// `SKILL.md`. De-duplicated by name and sorted, since the same skill is
/// commonly symlinked across several providers.
pub fn read_installed_skills() -> Vec<SkillEntry> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };

    let mut skills: Vec<SkillEntry> = Vec::new();
    for definition in SKILL_PROVIDER_DEFINITIONS.iter() {
        let provider_dir = home.join(&definition.skills_path);
        let Ok(entries) = std::fs::read_dir(&provider_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // A skill lives in `<provider>/<name>/SKILL.md`.
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if skills.iter().any(|s| s.name == name) {
                continue;
            }
            let description = parse_skill_description(&skill_md);
            skills.push(SkillEntry {
                name,
                description,
                path: skill_md,
            });
        }
    }

    skills.sort_by_key(|s| s.name.to_lowercase());
    skills
}

/// Extracts the `description:` value from a `SKILL.md` YAML frontmatter block
/// (the leading `--- … ---` section). Returns `None` when there's no
/// frontmatter or no description. Kept deliberately tiny — we don't want a YAML
/// dependency just to read one line for a subtitle.
fn parse_skill_description(skill_md: &Path) -> Option<String> {
    let content = std::fs::read_to_string(skill_md).ok()?;
    let mut lines = content.lines();

    // Frontmatter must be the very first thing in the file.
    if lines.next()?.trim() != "---" {
        return None;
    }

    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("description:") {
            let desc = rest.trim();
            // A block-scalar indicator (`>`, `|`, optionally with a chomping/indent modifier like
            // `>-`) means the value spans following lines; we don't parse those, so treat it as no
            // inline description rather than rendering a bare `>`/`|` as the subtitle.
            let is_block_scalar = desc
                .strip_prefix(['>', '|'])
                .is_some_and(|after| after.chars().all(|c| c == '-' || c == '+' || c.is_ascii_digit()));
            if is_block_scalar {
                return None;
            }
            let desc = desc.trim_matches('"').trim_matches('\'').trim();
            if !desc.is_empty() {
                return Some(desc.to_string());
            }
        }
    }
    None
}

/// Truncates `s` to at most `max` characters, appending an ellipsis when cut, so
/// long descriptions don't overflow the narrow panel.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{}…", truncated.trim_end())
}

/// A clickable skill row: name plus optional description subtitle. Clicking
/// opens the skill's `SKILL.md` in an editor tab.
fn skill_row(
    entry: &SkillEntry,
    mouse_state: MouseStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font_family = appearance.ui_font_family();
    let name_color = theme.main_text_color(theme.surface_1()).into_solid();
    let desc_color = theme.nonactive_ui_text_color().into_solid();
    let hover_bg = theme.surface_2().into_solid();

    let name = entry.name.clone();
    let description = entry.description.clone();
    let path = entry.path.clone();

    Hoverable::new(mouse_state, move |state| {
        let name_el = Text::new_inline(name.clone(), ui_font_family, 12.)
            .with_color(name_color)
            .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_main_axis_size(MainAxisSize::Min);
        column.add_child(name_el);
        if let Some(desc) = &description {
            let desc_el = Text::new_inline(truncate(desc, 88), ui_font_family, 11.)
                .with_color(desc_color)
                .finish();
            column.add_child(Container::new(desc_el).with_padding_top(1.).finish());
        }

        let mut container = Container::new(column.finish())
            .with_padding_left(12.)
            .with_vertical_padding(5.);
        if state.is_hovered() {
            container = container.with_background_color(hover_bg);
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::OpenConfigFile { path: path.clone() })
    })
    .finish()
}

/// Renders the Skills panel's content: a scrollable list of installed skills
/// (each opens its `SKILL.md` on click), or an empty hint when none are found.
/// `row_states` must be at least as long as `skills`.
pub fn render_skills_content(
    skills: &[SkillEntry],
    row_states: &[MouseStateHandle],
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
    for (i, entry) in skills.iter().enumerate() {
        let state = row_states.get(i).cloned().unwrap_or_default();
        column.add_child(skill_row(entry, state, appearance));
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
