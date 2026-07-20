pub mod default_themes;
pub mod theme;
pub mod theme_background_image;
pub mod theme_chooser;
pub mod theme_explorer_body;
pub mod theme_gallery;
pub mod theme_creator;
pub mod theme_creator_body;
pub mod theme_creator_modal;
pub mod theme_deletion_body;
pub mod theme_deletion_modal;

use warp_core::ui::theme::WarpTheme;

pub fn onboarding_theme_picker_themes() -> [WarpTheme; 5] {
    [
        default_themes::uncaged(),
        default_themes::midnight(),
        default_themes::dark_theme(),
        default_themes::light_theme(),
        default_themes::adeberry(),
    ]
}
