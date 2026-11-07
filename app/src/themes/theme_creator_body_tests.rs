use settings::Setting as _;
use warpui::platform::WindowStyle;
use warpui::App;

use super::*;
use crate::appearance::AppearanceManager;
use crate::settings::{Settings, ThemeSettings};
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::vim_registers::VimRegisters;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspace::ToastStack;

fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(AppearanceManager::new);
    app.add_singleton_model(|_| ToastStack);
    app.add_singleton_model(|_| SyncedInputState::mock());
    app.add_singleton_model(|_| VimRegisters::new());
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.update(|ctx| {
        ThemeSettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.use_system_theme.set_value(false, ctx).unwrap();
            settings
                .theme_kind
                .set_value(ThemeKind::Adeberry, ctx)
                .unwrap();
        });
        AppearanceManager::handle(ctx).update(ctx, |manager, ctx| {
            manager.refresh_theme_state(ctx);
        });
    });
}

/// The theme the test app has saved as active. The editor must never change what the app shows;
/// only the preview card follows the edits.
fn saved_theme(app: &mut App) -> WarpTheme {
    app.update(|ctx| Settings::theme_for_theme_kind(&ThemeKind::Adeberry, ctx))
}

#[test]
fn initializing_hidden_theme_editor_preserves_saved_theme() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let expected_theme =
            app.update(|ctx| Settings::theme_for_theme_kind(&ThemeKind::Adeberry, ctx));

        // add_window flushes the queued editor events after construction, including those
        // emitted before the color editors' subscriptions were registered.
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.read(&app, |body, ctx| {
            assert_eq!(Appearance::as_ref(ctx).theme(), &expected_theme);
            assert_eq!(
                *ThemeSettings::as_ref(ctx).theme_kind.value(),
                ThemeKind::Adeberry
            );
            assert!(!*ThemeSettings::as_ref(ctx).use_system_theme.value());
            assert_eq!(
                body.color_editors[SLOT_BG].as_ref(ctx).buffer_text(ctx),
                "#15110c"
            );
        });
    });
}

#[test]
fn manual_color_edit_updates_preview_card_without_touching_app_theme() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| {
            body.color_editors[SLOT_ACCENT].update(ctx, |editor, ctx| {
                editor.set_buffer_text("#123456", ctx);
            });
        });

        body.read(&app, |body, ctx| {
            let expected_accent = coloru_from_hex_string("#123456").unwrap();
            assert_eq!(body.manual_colors[SLOT_ACCENT], expected_accent);
            assert_eq!(
                body.preview_theme(ctx).accent(),
                ThemeFill::Solid(expected_accent)
            );
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
            assert_eq!(
                *ThemeSettings::as_ref(ctx).theme_kind.value(),
                ThemeKind::Adeberry
            );
        });
    });
}

#[test]
fn showing_theme_editor_previews_defaults_in_card_only() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| body.on_shown(ctx));

        body.read(&app, |body, ctx| {
            assert_eq!(body.preview_theme(ctx), body.build_manual_theme(ctx, None));
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
            assert_eq!(
                *ThemeSettings::as_ref(ctx).theme_kind.value(),
                ThemeKind::Adeberry
            );
        });
    });
}

#[test]
fn appearance_segments_set_light_and_dark_without_toggling() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::SetLightAppearance(true), ctx);
        });
        body.read(&app, |body, ctx| {
            assert!(body.is_light);
            assert_eq!(body.preview_theme(ctx), body.build_manual_theme(ctx, None));
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });

        // Clicking the segment that is already selected keeps it selected rather than flipping it.
        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::SetLightAppearance(true), ctx);
        });
        body.read(&app, |body, _| assert!(body.is_light));

        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::SetLightAppearance(false), ctx);
        });
        body.read(&app, |body, _| assert!(!body.is_light));
    });
}

#[test]
fn preview_follows_manual_edits_and_gradient_switch() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.read(&app, |body, ctx| {
            assert!(matches!(
                body.preview_theme(ctx).background(),
                ThemeFill::VerticalGradient(_)
            ));
        });

        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::ToggleGradient, ctx);
            body.color_editors[SLOT_BG].update(ctx, |editor, ctx| {
                editor.set_buffer_text("#203040", ctx);
            });
        });

        body.read(&app, |body, ctx| {
            let expected_background = coloru_from_hex_string("#203040").unwrap();
            assert_eq!(
                body.preview_theme(ctx).background(),
                ThemeFill::Solid(expected_background)
            );
            assert_eq!(body.preview_theme(ctx), body.build_manual_theme(ctx, None));
        });
    });
}

#[test]
fn from_image_preview_shows_current_theme_until_an_image_is_picked() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| {
            body.handle_action(
                &ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                ctx,
            );
        });

        body.read(&app, |body, ctx| {
            assert_eq!(body.mode, ThemeCreatorMode::FromImage);
            assert_eq!(&body.preview_theme(ctx), Appearance::as_ref(ctx).theme());
        });
    });
}

fn processed_image_options() -> InMemoryThemeOptions {
    InMemoryThemeOptions::new_for_test(
        "Harbour at dusk".to_string(),
        PathBuf::from("/tmp/harbour.jpg"),
        vec![
            coloru_from_hex_string("#1b2430").unwrap(),
            coloru_from_hex_string("#d9c7a3").unwrap(),
            coloru_from_hex_string("#c0504d").unwrap(),
            coloru_from_hex_string("#4f81bd").unwrap(),
            coloru_from_hex_string("#9bbb59").unwrap(),
        ],
    )
}

#[test]
fn switching_modes_never_touches_the_app_theme() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        let saved_theme = saved_theme(&mut app);
        body.update(&mut app, |body, ctx| body.on_shown(ctx));

        body.update(&mut app, |body, ctx| {
            body.handle_action(
                &ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                ctx,
            );
        });

        body.read(&app, |_, ctx| {
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });

        // Not with a processed image either: its theme shows only in the preview card.
        body.update(&mut app, |body, ctx| {
            body.theme_options = Some(processed_image_options());
            body.handle_action(
                &ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::Manual),
                ctx,
            );
            body.handle_action(
                &ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                ctx,
            );
        });
        body.read(&app, |body, ctx| {
            assert!(body.preview_theme(ctx).background_image().is_some());
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });
    });
}

#[test]
fn cancel_starts_over_in_both_modes_and_leaves_the_app_theme_alone() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        for mode in [ThemeCreatorMode::Manual, ThemeCreatorMode::FromImage] {
            body.update(&mut app, |body, ctx| {
                body.on_shown(ctx);
                body.editor.update(ctx, |editor, ctx| {
                    editor.set_buffer_text("Half-made theme", ctx);
                });
                body.theme_options = Some(processed_image_options());
                body.handle_action(&ThemeCreatorBodyAction::SetMode(mode), ctx);
            });

            body.update(&mut app, |body, ctx| {
                body.handle_action(&ThemeCreatorBodyAction::Cancel, ctx);
            });

            // Read after the update so the editor's queued reset events have been delivered:
            // they must not refill anything.
            body.read(&app, |body, ctx| {
                assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
                assert_eq!(body.editor.as_ref(ctx).buffer_text(ctx), "");
                assert!(body.theme_options.is_none());
                assert_eq!(body.mode, mode);
            });
        }
    });
}

#[test]
fn image_background_switch_updates_the_generated_preview() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| {
            body.theme_options = Some(processed_image_options());
            body.handle_action(
                &ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                ctx,
            );
        });
        body.read(&app, |body, ctx| {
            let preview = body.preview_theme(ctx);
            assert!(preview.background_image().is_some());
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });

        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::ToggleImageAsBackground, ctx);
            body.handle_action(&ThemeCreatorBodyAction::SetBackgroundColor(1), ctx);
        });
        body.read(&app, |body, ctx| {
            let preview = body.preview_theme(ctx);
            assert!(preview.background_image().is_none());
            assert_eq!(
                preview.background(),
                ThemeFill::Solid(coloru_from_hex_string("#d9c7a3").unwrap())
            );
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });
    });
}

#[test]
fn customize_colors_moves_the_generated_theme_into_the_custom_editor() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        let options = processed_image_options();
        let generated = options.theme_with_background_image(Some(DEFAULT_BG_IMAGE_OPACITY));
        body.update(&mut app, |body, ctx| {
            body.theme_options = Some(options.clone());
            body.handle_action(
                &ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                ctx,
            );
            body.handle_action(&ThemeCreatorBodyAction::CustomizeGeneratedTheme, ctx);
        });

        body.read(&app, |body, ctx| {
            assert_eq!(body.mode, ThemeCreatorMode::Manual);
            assert_eq!(body.editor.as_ref(ctx).buffer_text(ctx), "Harbour at dusk");
            assert_eq!(
                body.manual_colors[SLOT_BG],
                generated.background().into_solid()
            );
            assert_eq!(
                body.manual_colors[SLOT_FG],
                generated.foreground().into_solid()
            );
            assert_eq!(
                body.manual_colors[SLOT_ACCENT],
                generated.accent().into_solid()
            );
            assert_eq!(
                body.manual_colors[SLOT_BRIGHT_START + 1],
                ColorU::from(generated.terminal_colors().bright.red)
            );
            assert_eq!(
                body.color_editors[SLOT_ACCENT].as_ref(ctx).buffer_text(ctx),
                coloru_to_hex_string(&generated.accent().into_solid())
            );
            assert!(!body.use_gradient);
            assert_eq!(body.bg_image, Some(options.path()));
            // The preview card shows the custom editor's copy, which is now what gets saved or
            // shared; the app keeps its own theme.
            assert_eq!(
                body.preview_theme(ctx),
                body.build_manual_theme(ctx, body.bg_image.as_deref())
            );
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });
    });
}

#[test]
fn removing_the_background_image_brings_back_background_opacity() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let saved_theme = saved_theme(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| {
            body.handle_action(
                &ThemeCreatorBodyAction::HandleImageSelected(PathBuf::from("/tmp/harbour.jpg")),
                ctx,
            );
            body.handle_action(&ThemeCreatorBodyAction::SetBackgroundOpacity(40.), ctx);
            // A flat background, so its alpha can be read straight off the fill.
            body.handle_action(&ThemeCreatorBodyAction::ToggleGradient, ctx);
        });
        body.read(&app, |body, ctx| {
            let preview = body.preview_theme(ctx);
            assert!(preview.background_image().is_some());
            // With a picture, the background colour is an opaque veil over it.
            assert_eq!(preview.background().into_solid().a, 255);
        });

        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::RemoveBackgroundImage, ctx);
        });
        body.read(&app, |body, ctx| {
            let preview = body.preview_theme(ctx);
            assert!(body.bg_image.is_none());
            assert!(preview.background_image().is_none());
            assert_eq!(preview.background().into_solid().a, 102);
            assert_eq!(Appearance::as_ref(ctx).theme(), &saved_theme);
        });
    });
}
