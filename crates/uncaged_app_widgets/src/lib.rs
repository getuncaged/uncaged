//! App-side widget and helper modules that do not depend on the rest of the app crate:
//! generic `util` helpers, `ui_components` (buttons, dialogs, avatars, ...) and the reusable
//! `view_components` views (action buttons, alerts, toasts, ...). The app re-exports these at
//! their old `crate::util` / `crate::ui_components` / `crate::view_components` paths.

pub mod spacing;
pub mod ui_components;
pub mod util;
pub mod view_components;
