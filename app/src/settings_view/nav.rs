use pathfinder_geometry::vector::vec2f;
use warpui::elements::{
    Container, Element, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, Text,
};
use warpui::fonts::Weight;
use warpui::ui_components::button::{ButtonVariant, TextAndIcon, TextAndIconAlignment};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};

use super::settings_page::{MatchData, NAV_ITEM_LEFT_MARGIN};
use super::SettingsSection;
use crate::appearance::Appearance;
use crate::ui_components::icons::Icon;

/// The font size for subpage items inside an umbrella.
const SUBPAGE_FONT_SIZE: f32 = 10.;

/// Left margin for subpage items inside an umbrella (top-level margin + indent).
const SUBPAGE_LEFT_MARGIN: f32 = NAV_ITEM_LEFT_MARGIN + 12.;

/// The font size of a group's label in the sidebar.
const GROUP_LABEL_FONT_SIZE: f32 = 10.;
/// The space above a group's label, separating it from the group before.
const GROUP_LABEL_MARGIN_TOP: f32 = 14.;
/// The inner padding of every sidebar item; group labels are inset by the same amount so they
/// line up with the item text.
const NAV_ITEM_PADDING: f32 = 8.;

/// Uncaged: the style of a sidebar item. The selected item gets a quiet raised fill with the
/// main text colour, rather than a full accent-coloured button.
pub fn nav_item_styles(
    appearance: &Appearance,
    is_active: bool,
    left_margin: f32,
) -> UiComponentStyles {
    let theme = appearance.theme();
    let styles = UiComponentStyles::default()
        .set_border_width(0.)
        .set_margin(Coords::default().left(left_margin))
        .set_padding(Coords::uniform(NAV_ITEM_PADDING));
    if is_active {
        styles
            .set_background(theme.surface_2().into())
            .set_font_color(theme.active_ui_text_color().into_solid())
            .set_font_weight(Weight::Semibold)
    } else {
        styles
    }
}

/// A collapsible group of settings subpages in the sidebar.
pub struct SettingsUmbrella {
    pub label: &'static str,
    pub subpages: Vec<SettingsSection>,
    pub expanded: bool,
    /// Whether the header can be clicked to collapse the group. Uncaged's sidebar uses
    /// always-open groups under a plain label instead (see [`SettingsUmbrella::group`]).
    pub collapsible: bool,
    /// Saved expanded state from before search began, restored when search is cleared.
    pub pre_search_expanded: Option<bool>,
    pub button_state_handle: MouseStateHandle,
    pub subpage_button_states: Vec<MouseStateHandle>,
}

impl SettingsUmbrella {
    pub fn new(label: &'static str, subpages: Vec<SettingsSection>) -> Self {
        let subpage_count = subpages.len();
        Self {
            label,
            subpages,
            expanded: false,
            collapsible: true,
            pre_search_expanded: None,
            button_state_handle: MouseStateHandle::default(),
            subpage_button_states: (0..subpage_count)
                .map(|_| MouseStateHandle::default())
                .collect(),
        }
    }

    /// A group that is always open, shown under a plain label rather than a collapsible header.
    pub fn group(label: &'static str, subpages: Vec<SettingsSection>) -> Self {
        Self {
            expanded: true,
            collapsible: false,
            ..Self::new(label, subpages)
        }
    }

    pub fn toggle(&mut self) {
        if self.collapsible {
            self.expanded = !self.expanded;
        }
    }

    /// Whether the group's items are listed in the sidebar.
    pub fn is_open(&self) -> bool {
        self.expanded || !self.collapsible
    }

    /// Render the label above an always-open group's items.
    pub fn render_group_label(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Text::new_inline(
                self.label.to_uppercase(),
                appearance.ui_font_family(),
                GROUP_LABEL_FONT_SIZE,
            )
            .with_color(theme.sub_text_color(theme.background()).into_solid())
            .with_style(warpui::fonts::Properties::default().weight(Weight::Semibold))
            .finish(),
        )
        .with_margin_top(GROUP_LABEL_MARGIN_TOP)
        .with_margin_bottom(2.)
        .with_padding_left(NAV_ITEM_LEFT_MARGIN + NAV_ITEM_PADDING)
        .finish()
    }

    /// Returns true if `section` is one of this umbrella's subpages.
    pub fn contains(&self, section: SettingsSection) -> bool {
        self.subpages.contains(&section)
    }

    /// Render the umbrella header row (label + chevron).
    /// Returns a `Hoverable` so the entire row shares a single hover/click
    /// target — i.e. the hover styling and pointing-hand cursor apply to the
    /// whole clickable area rather than just the text.
    pub fn render_umbrella_row(&self, appearance: &Appearance) -> Hoverable {
        let chevron_icon = if self.expanded {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        };

        // Initial chevron color is overridden by the button's font_color when
        // rendered, so this just seeds a sensible default.
        let text_color = appearance.theme().nonactive_ui_text_color();

        // Use a single full-width text button with a text+icon label so the
        // text label aligns with other top-level settings items and the
        // chevron sits flush-right — while the whole button area receives the
        // hover styling and pointing-hand cursor.
        appearance
            .ui_builder()
            .button(ButtonVariant::Text, self.button_state_handle.clone())
            .with_text_and_icon_label(TextAndIcon::new(
                TextAndIconAlignment::TextFirst,
                self.label.to_string(),
                chevron_icon.to_warpui_icon(text_color),
                MainAxisSize::Max,
                MainAxisAlignment::SpaceBetween,
                vec2f(16., 16.),
            ))
            .with_style(
                UiComponentStyles::default()
                    .set_border_width(0.)
                    .set_margin(Coords::default().left(NAV_ITEM_LEFT_MARGIN))
                    .set_padding(Coords::uniform(8.)),
            )
            .build()
    }

    /// Render a single subpage button within this umbrella.
    pub fn render_subpage_button(
        &self,
        index: usize,
        appearance: &Appearance,
        match_data: MatchData,
        is_active: bool,
    ) -> Option<Hoverable> {
        let section = self.subpages.get(index)?;
        let mouse_state = self.subpage_button_states.get(index)?.clone();

        let label = section.to_string() + &match_data.to_string();

        // A group's items read like top-level pages; a collapsible umbrella's are indented and
        // smaller under its header.
        let styles = if self.collapsible {
            nav_item_styles(appearance, is_active, SUBPAGE_LEFT_MARGIN)
                .set_font_size(SUBPAGE_FONT_SIZE)
        } else {
            nav_item_styles(appearance, is_active, NAV_ITEM_LEFT_MARGIN)
        };

        let hoverable = appearance
            .ui_builder()
            .button(ButtonVariant::Text, mouse_state)
            .with_text_label(label)
            .with_style(styles)
            .build();

        Some(hoverable)
    }
}

/// A sidebar navigation item: either a direct page link or a collapsible umbrella.
pub enum SettingsNavItem {
    /// A top-level page that is rendered directly in the sidebar. Uncaged's sidebar currently
    /// puts every page in a group, so only tests build this today.
    #[cfg_attr(not(test), allow(dead_code))]
    Page(SettingsSection),
    /// A collapsible group header whose children are subpage sections.
    Umbrella(SettingsUmbrella),
}
