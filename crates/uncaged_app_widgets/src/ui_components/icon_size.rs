use uncaged_core::ui::appearance::Appearance;
use uncaged_ui::{AppContext, SingletonEntity};

/// Returns the size for icons in the AI block, scaled to the user's current font size.
pub fn icon_size(app: &AppContext) -> f32 {
    let appearance = Appearance::as_ref(app);
    app.font_cache().line_height(
        appearance.monospace_font_size(),
        appearance.line_height_ratio(),
    )
}
