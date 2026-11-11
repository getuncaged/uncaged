pub use uncaged_app_widgets::ui_components::icon_size::icon_size;
use uncaged_core::ui::appearance::Appearance;
use uncaged_core::ui::theme::AnsiColorIdentifier;

use crate::ui_components::blended_colors;
use crate::ui_components::icons::Icon;

pub fn green_check_icon(appearance: &Appearance) -> uncaged_ui::elements::Icon {
    uncaged_ui::elements::Icon::new(
        Icon::Check.into(),
        AnsiColorIdentifier::Green.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

pub fn red_x_icon(appearance: &Appearance) -> uncaged_ui::elements::Icon {
    uncaged_ui::elements::Icon::new(
        Icon::X.into(),
        AnsiColorIdentifier::Red.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

/// Yellow warning triangle for terminal states that only partially succeeded
/// (e.g. some child agents launched and others failed).
pub fn warning_icon(appearance: &Appearance) -> uncaged_ui::elements::Icon {
    uncaged_ui::elements::Icon::new(
        Icon::Triangle.into(),
        AnsiColorIdentifier::Yellow.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

pub fn cancelled_icon(appearance: &Appearance) -> uncaged_ui::elements::Icon {
    uncaged_ui::elements::Icon::new(
        Icon::Cancelled.into(),
        blended_colors::neutral_6(appearance.theme()),
    )
}

pub fn reverted_icon(appearance: &Appearance) -> uncaged_ui::elements::Icon {
    uncaged_ui::elements::Icon::new(
        Icon::ReverseLeft.into(),
        blended_colors::neutral_6(appearance.theme()),
    )
}
