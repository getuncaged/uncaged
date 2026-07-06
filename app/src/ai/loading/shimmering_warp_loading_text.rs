//! Shimmering loading text - renders the Uncaged caret with shimmering text for loading states.

use warp_core::ui::appearance::Appearance;
use warpui::elements::shimmering_text::{
    ShimmerConfig, ShimmeringTextElement, ShimmeringTextStateHandle,
};
use warpui::elements::Element;
use warpui::{AppContext, SingletonEntity};

/// Uncaged caret glyph (`›`, U+203A) — replaces Warp's private-use logo glyph
/// (U+E500) that used to render Warp's trademarked mark in loading text. U+203A is
/// present in the bundled Roboto (unlike the dingbat `❯` U+276F), so it renders
/// without falling back to a system font.
const UNCAGED_CARET: &str = "\u{203A}";

/// Creates a shimmering text element with the Warp glyph.
pub fn shimmering_warp_loading_text(
    text: impl Into<String>,
    font_size: f32,
    shimmer_handle: ShimmeringTextStateHandle,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();

    // Use same colors as common.rs for consistency
    let base_color = theme.disabled_text_color(theme.surface_1()).into_solid();
    let shimmer_color = theme.main_text_color(theme.surface_1()).into_solid();

    // Hardcoded shimmer config for consistent animation
    let config = ShimmerConfig::default();

    // Create a single shimmering element with glyph and text
    ShimmeringTextElement::new(
        format!("{} {}", UNCAGED_CARET, text.into()),
        appearance.ui_font_family(),
        font_size,
        base_color,
        shimmer_color,
        config,
        shimmer_handle,
    )
    .finish()
}
