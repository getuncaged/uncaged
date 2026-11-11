//! Warp UI Components module contains functions and structs that implement our internal components
//! used for the apps design (our buttons with styling, headers and panels etc.) as well definition
//! of colors (aka blended colors from the figma designs derived from Warp theme) and icons used
//! within the app. The theme-only components live in `uncaged_app_widgets`.
pub(crate) mod agent_icon;
pub(crate) mod icon_with_status;
pub(crate) mod render_file_search_row;
pub(crate) mod window_focus_dimming;

pub use uncaged_app_widgets::ui_components::{
    avatar, blended_colors, breadcrumb, buttons, color_dot, dialog, icons, item_highlight,
    menu_button, red_notification_dot,
};
