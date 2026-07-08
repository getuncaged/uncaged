use ai::LLMId;
use warp_core::features::FeatureFlag;
use warp_core::telemetry::testing::MockTelemetryContextProvider;
use warpui_core::{App, ModelHandle};

use crate::model::{AiSetupChoice, OnboardingAuthState, OnboardingStateModel, OnboardingStep};
use crate::OnboardingIntention;

fn add_test_model(app: &mut App) -> ModelHandle<OnboardingStateModel> {
    app.update(MockTelemetryContextProvider::register);
    app.add_model(|_| {
        OnboardingStateModel::new(
            Vec::new(),
            LLMId::from("auto"),
            false,
            true,
            OnboardingAuthState::FreeUser,
        )
    })
}

fn step(app: &App, model: &ModelHandle<OnboardingStateModel>) -> OnboardingStep {
    model.read(app, |model, _| model.step())
}

/// The live Uncaged flow is a fixed four-step sequence:
/// Intro → AiAccess → Customize → ThemePicker, with ThemePicker terminal.
#[test]
fn next_walks_the_four_step_flow() {
    let _flag = FeatureFlag::OpenWarpNewSettingsModes.override_enabled(true);
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        // Starts on the intro slide.
        assert_eq!(step(&app, &model), OnboardingStep::Intro);

        for expected in [
            OnboardingStep::AiAccess,
            OnboardingStep::Customize,
            OnboardingStep::ThemePicker,
        ] {
            model.update(&mut app, |model, ctx| model.next(ctx));
            assert_eq!(step(&app, &model), expected);
        }

        // ThemePicker is the last step: `next` is a no-op.
        model.update(&mut app, |model, ctx| model.next(ctx));
        assert_eq!(step(&app, &model), OnboardingStep::ThemePicker);
    });
}

/// Back navigation is the exact reverse of the forward flow and stops on Intro.
#[test]
fn back_reverses_the_four_step_flow() {
    let _flag = FeatureFlag::OpenWarpNewSettingsModes.override_enabled(true);
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        // Walk to the last step.
        model.update(&mut app, |model, ctx| {
            model.next(ctx); // Intro → AiAccess
            model.next(ctx); // AiAccess → Customize
            model.next(ctx); // Customize → ThemePicker
        });
        assert_eq!(step(&app, &model), OnboardingStep::ThemePicker);

        for expected in [
            OnboardingStep::Customize,
            OnboardingStep::AiAccess,
            OnboardingStep::Intro,
        ] {
            model.update(&mut app, |model, ctx| model.back(ctx));
            assert_eq!(step(&app, &model), expected);
        }

        // Intro is the first step: `back` is a no-op.
        model.update(&mut app, |model, ctx| model.back(ctx));
        assert_eq!(step(&app, &model), OnboardingStep::Intro);
    });
}

/// The progress dots read `(step_index, 4)` for every step in the flow.
#[test]
fn progress_reports_four_dots_with_correct_indices() {
    let _flag = FeatureFlag::OpenWarpNewSettingsModes.override_enabled(true);
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        let cases = [
            (OnboardingStep::Intro, (0, 4)),
            (OnboardingStep::AiAccess, (1, 4)),
            (OnboardingStep::Customize, (2, 4)),
            (OnboardingStep::ThemePicker, (3, 4)),
        ];
        for (target, expected) in cases {
            model.update(&mut app, |model, ctx| model.set_step(target, ctx));
            let progress = model.read(&app, |model, _| model.progress());
            assert_eq!(progress, expected, "unexpected dots for {target:?}");
        }
    });
}

/// The intention defaults to agent-driven development and stays there — there is
/// no longer a terminal fork or an AI-setup choice inside the flow.
#[test]
fn intention_defaults_to_agent_driven_development() {
    let _flag = FeatureFlag::OpenWarpNewSettingsModes.override_enabled(true);
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        model.read(&app, |model, _| {
            assert_eq!(
                *model.intention(),
                OnboardingIntention::AgentDrivenDevelopment
            );
            assert!(model.settings().is_ai_enabled());
        });

        // Walking the whole flow never changes the intention.
        model.update(&mut app, |model, ctx| {
            model.next(ctx);
            model.next(ctx);
            model.next(ctx);
        });
        model.read(&app, |model, _| {
            assert_eq!(
                *model.intention(),
                OnboardingIntention::AgentDrivenDevelopment
            );
        });
    });
}

/// Agent intent keeps AI enabled regardless of the underlying AI-setup choice:
/// choosing to bring third-party CLI agents still means the user wants AI.
#[test]
fn agent_intent_keeps_ai_enabled_for_any_setup_choice() {
    let _flag = FeatureFlag::OpenWarpNewSettingsModes.override_enabled(true);
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        // Default agent intention + built-in agent enables AI.
        model.read(&app, |model, _| assert!(model.settings().is_ai_enabled()));

        // Third-party CLI agents still keep AI enabled.
        model.update(&mut app, |model, ctx| {
            model.set_ai_setup_choice(AiSetupChoice::ThirdParty, ctx)
        });
        model.read(&app, |model, _| assert!(model.settings().is_ai_enabled()));

        // Switching back to the built-in agent also keeps AI enabled.
        model.update(&mut app, |model, ctx| {
            model.set_ai_setup_choice(AiSetupChoice::WarpAgent, ctx)
        });
        model.read(&app, |model, _| assert!(model.settings().is_ai_enabled()));
    });
}
