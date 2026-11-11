//! UI components that only need the theme/appearance layer: buttons, dialogs, avatars, ...
pub mod avatar;
pub mod blended_colors;
pub mod breadcrumb;
pub mod buttons;
pub mod color_dot;
pub mod dialog;
pub mod icon_size;
pub mod item_highlight;
pub mod menu_button;
pub mod red_notification_dot;

pub use uncaged_core::ui::icons;

const BORDER_RADIUS: f32 = uncaged_core::ui::radius::SMALL;
