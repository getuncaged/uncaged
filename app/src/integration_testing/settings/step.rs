use settings::Setting;
use warpui::integration::TestStep;
use warpui::windowing::WindowManager;
use warpui::{async_assert, SingletonEntity};

use crate::integration_testing::step::new_step_with_default_assertions;
use crate::settings_view::SettingsAction;
use crate::user_config::WarpConfig;
use crate::window_settings::WindowSettings;
use crate::workspace::{Workspace, WorkspaceAction};

/// Builds a step that will toggle a setting by [`SettingsAction`]. This can
/// only update settings with a corresponding action on the settings view.
pub fn toggle_setting(action: SettingsAction) -> TestStep {
    new_step_with_default_assertions(&format!("Toggle setting: {action:?}")).with_action(
        move |app, _, _| {
            let window_id = app.read(|ctx| {
                WindowManager::as_ref(ctx)
                    .active_window()
                    .expect("no active window")
            });
            let workspace_view_id = app
                .views_of_type::<Workspace>(window_id)
                .and_then(|views| views.first().map(|view| view.id()))
                .expect("no workspace view");
            app.dispatch_typed_action(
                window_id,
                &[workspace_view_id],
                &WorkspaceAction::DispatchToSettingsTab(action.clone()),
            );
        },
    )
}

/// Asserts how many loaded themes are named like `theme_name` — the themes the theme gallery and
/// the Appearance page offer.
pub fn assert_installed_themes_contain(theme_name: &'static str, count: usize) -> TestStep {
    TestStep::new("Assert the installed themes match our expectations").add_named_assertion(
        format!("The installed themes include {count} theme(s) named \"{theme_name}\""),
        move |app, _window_id| {
            let result: usize = app.read(|ctx| {
                WarpConfig::as_ref(ctx)
                    .theme_config()
                    .theme_items()
                    .filter(|(theme, _)| theme.matches(theme_name))
                    .count()
            });
            async_assert!(
                result == count,
                "Should have exactly {count} theme(s) named test theme. Instead had {result}"
            )
        },
    )
}

/// Set a custom size for new windows. This updates:
/// * The boolean setting for whether or not to use the custom size
/// * The setting for the window width in rows
/// * The setting for the window height in columns
pub fn set_window_custom_size(rows: u16, columns: u16) -> TestStep {
    TestStep::new("Set custom size for new windows").with_action(move |app, _, _| {
        WindowSettings::handle(app).update(app, |settings, ctx| {
            settings
                .open_windows_at_custom_size
                .set_value(true, ctx)
                .expect("Could not enable custom window sizes");
            settings
                .new_windows_num_rows
                .set_value(rows, ctx)
                .expect("Could not set window width");
            settings
                .new_windows_num_columns
                .set_value(columns, ctx)
                .expect("Could not set window height");
        })
    })
}
