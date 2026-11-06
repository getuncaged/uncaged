use settings::Setting as _;
use warpui::platform::WindowStyle;
use warpui::App;

use super::*;
use crate::auth::AuthStateProvider;
use crate::settings::{Settings, ThemeSettings};
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::vim_registers::VimRegisters;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspace::ToastStack;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(AppearanceManager::new);
    app.add_singleton_model(|_| ToastStack);
    app.add_singleton_model(|_| SyncedInputState::mock());
    app.add_singleton_model(|_| VimRegisters::new());
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(UserWorkspaces::default_mock);
    #[cfg(feature = "voice_input")]
    app.add_singleton_model(voice_input::VoiceInput::new);
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
fn manual_color_edit_updates_preview_without_changing_saved_theme() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
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
                Appearance::as_ref(ctx).theme().accent(),
                ThemeFill::Solid(expected_accent)
            );
            assert_eq!(
                *ThemeSettings::as_ref(ctx).theme_kind.value(),
                ThemeKind::Adeberry
            );
        });
    });
}

#[test]
fn showing_theme_editor_explicitly_starts_default_preview() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| body.on_shown(ctx));

        body.read(&app, |body, ctx| {
            assert_eq!(
                Appearance::as_ref(ctx).theme(),
                &body.build_manual_theme(ctx, None)
            );
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
        let (_, body) = app.add_window(WindowStyle::NotStealFocus, ThemeCreatorBody::new);

        body.update(&mut app, |body, ctx| {
            body.handle_action(&ThemeCreatorBodyAction::SetLightAppearance(true), ctx);
        });
        body.read(&app, |body, ctx| {
            assert!(body.is_light);
            assert_eq!(
                Appearance::as_ref(ctx).theme(),
                &body.build_manual_theme(ctx, None)
            );
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
