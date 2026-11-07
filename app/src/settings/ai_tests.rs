use chrono::Utc;
use settings_value::SettingsValue as _;
use warpui::{App, SingletonEntity};

use super::*;
use crate::test_util::settings::initialize_settings_for_tests;

// FocusedTerminalInfo Tests

#[test]
fn test_update_both_values_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // Update both values to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_additional_value_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Now update to (true, true) - only changing restored blocks
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_no_change() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with same values (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state remains the same
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify no event was emitted
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 0);
    });
}

#[test]
fn test_update_only_remote_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (false, true) - only remote blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(false, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(!model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_only_restored_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (true, false) - only restored blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

// ToolbarCommandMap Tests

#[test]
fn test_toolbar_command_map_deserialize_from_map() {
    let json = serde_json::json!({
        "^claude": "Claude",
        "^gemini": "Gemini",
        "^codex": ""
    });
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^gemini"], "Gemini");
    assert_eq!(map.0["^codex"], "");
}

#[test]
fn test_toolbar_command_map_deserialize_from_legacy_vec() {
    let json = serde_json::json!(["^claude", "^gemini", "^custom"]);
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    // Legacy vec format should assign empty agent values.
    for (_, agent) in map.0.iter() {
        assert_eq!(agent, "");
    }
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^claude", "^gemini", "^custom"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_map_format() {
    use settings_value::SettingsValue;

    let value = serde_json::json!({
        "^claude": "Claude",
        "^amp": "Amp"
    });
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 2);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^amp"], "Amp");
}

#[test]
fn test_toolbar_command_map_from_file_value_legacy_array() {
    use settings_value::SettingsValue;

    // Patterns are intentionally non-alphabetical to verify insertion order is preserved.
    let value = serde_json::json!(["^zebra", "^alpha", "^middle"]);
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^zebra"], "");
    assert_eq!(map.0["^alpha"], "");
    assert_eq!(map.0["^middle"], "");
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^zebra", "^alpha", "^middle"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_invalid() {
    use settings_value::SettingsValue;

    let value = serde_json::json!(42);
    assert!(ToolbarCommandMap::from_file_value(&value).is_none());
}

#[test]
fn test_toolbar_command_map_roundtrip() {
    use settings_value::SettingsValue;

    let mut inner = IndexMap::new();
    inner.insert("^claude".to_string(), "Claude".to_string());
    inner.insert("^custom".to_string(), String::new());
    let original = ToolbarCommandMap::new(inner);

    let file_value = original.to_file_value();
    let restored = ToolbarCommandMap::from_file_value(&file_value).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_toolbar_command_map_matched_agent() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let mut map = IndexMap::new();
        map.insert("^claude".to_string(), "Claude".to_string());
        map.insert("^gemini".to_string(), "Gemini".to_string());
        map.insert("^custom-tool".to_string(), String::new());

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            report_if_error!(settings
                .cli_agent_footer_enabled_commands
                .set_value(ToolbarCommandMap::new(map), ctx));
        });

        app.read(|ctx| {
            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "claude chat");
            assert_eq!(agent, Some(CLIAgent::Claude));

            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "gemini ask");
            assert_eq!(agent, Some(CLIAgent::Gemini));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "custom-tool --flag");
            assert_eq!(agent, Some(CLIAgent::Unknown));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "unmatched-command");
            assert_eq!(agent, None);
        });
    });
}

#[test]
fn orchestration_is_enabled_when_ai_is_enabled() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(settings.is_orchestration_enabled(ctx));
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_empty_history() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // With empty history, banner should not be displayed
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_exceeded_not_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that had quota exceeded and banner not dismissed
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should be displayed when the previous cycle had quota exceeded and banner not dismissed
            assert!(settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_exceeded_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that had quota exceeded but banner was dismissed
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: true },
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when the previous cycle had quota exceeded but banner was dismissed
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_not_exceeded() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that did not have quota exceeded
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when the previous cycle did not have quota exceeded
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_only_one_cycle() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with only one cycle
        let now = Utc::now();
        let current_end_date = now + chrono::Duration::days(15);

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: true, // Even if quota is exceeded
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when there's only one cycle, even if quota is exceeded
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_mark_quota_banner_as_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();

        // Create test cycles: two expired cycles and one future cycle
        let expired_cycle_1 = CycleInfo {
            end_date: now - chrono::Duration::days(30), // 30 days ago
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let expired_cycle_2 = CycleInfo {
            end_date: now - chrono::Duration::days(15), // 15 days ago
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let future_cycle = CycleInfo {
            end_date: now + chrono::Duration::days(15), // 15 days in future
            was_quota_exceeded: false,
            banner_state: BannerState { dismissed: false },
        };

        let cycle_history = vec![expired_cycle_1, expired_cycle_2, future_cycle];

        // Set up initial state
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        // Mark expired cycles as dismissed
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.mark_quota_banner_as_dismissed(ctx);
        });

        // Verify the results
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 3);

            // First cycle (oldest expired) should be dismissed
            assert!(cycle_history[0].banner_state.dismissed);
            // Second cycle (more recent expired) should be dismissed
            assert!(cycle_history[1].banner_state.dismissed);
            // Future cycle should not be dismissed
            assert!(!cycle_history[2].banner_state.dismissed);
        });
    });
}

#[test]
fn removed_cloud_agent_session_mode_falls_back_to_default() {
    // Older builds could persist `cloud_agent`; it no longer parses, so the setting keeps its
    // default (Terminal) instead of failing to load.
    assert_eq!(
        DefaultSessionMode::from_file_value(&serde_json::json!("cloud_agent")),
        None
    );
    assert_eq!(
        DefaultSessionMode::from_file_value(&serde_json::json!("agent")),
        Some(DefaultSessionMode::Agent)
    );
    assert_eq!(DefaultSessionMode::default(), DefaultSessionMode::Terminal);
}
