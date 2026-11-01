//! Uncaged: the corner radii every surface draws from.
//!
//! Warp hardcoded a 4px radius at each call site and left the big surfaces -- panels, the
//! settings sidebar, menu rows -- square. Three sizes, used consistently, give the UI the
//! softer, rounded look current macOS, Windows and VS Code share: small for controls you click
//! or type into, medium for floating chrome, large for the surfaces that hold everything else.

use warpui_core::elements::{CornerRadius, Radius};

/// Buttons, text inputs, list and menu rows, tooltips, chips.
pub const SMALL: f32 = 6.;
/// Menus, dropdowns, toasts, tabs, find bars.
pub const MEDIUM: f32 = 8.;
/// Panels, dialogs, the command palette, settings cards.
pub const LARGE: f32 = 12.;

pub fn small() -> CornerRadius {
    CornerRadius::with_all(Radius::Pixels(SMALL))
}

pub fn medium() -> CornerRadius {
    CornerRadius::with_all(Radius::Pixels(MEDIUM))
}

pub fn large() -> CornerRadius {
    CornerRadius::with_all(Radius::Pixels(LARGE))
}
