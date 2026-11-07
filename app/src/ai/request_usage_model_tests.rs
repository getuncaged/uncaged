use ai::api_keys::ApiKeyManager;
use chrono::Duration;
use warpui::{App, ModelHandle};

use super::*;
use crate::auth::AuthStateProvider;
use crate::server::server_api::ServerApiProvider;

fn add_request_usage_model(app: &mut App) -> ModelHandle<AIRequestUsageModel> {
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    add_request_usage_model_without_auth(app)
}

fn add_request_usage_model_for_anonymous_users(app: &mut App) -> ModelHandle<AIRequestUsageModel> {
    app.add_singleton_model(|_| AuthStateProvider::new_anonymous_for_test());
    add_request_usage_model_without_auth(app)
}
fn register_user_preferences_for_tests(app: &mut App) {
    if app
        .models_of_type::<settings::PrivatePreferences>()
        .is_empty()
    {
        app.update(crate::settings::init_and_register_user_preferences);
    }
}

fn add_request_usage_model_without_auth(app: &mut App) -> ModelHandle<AIRequestUsageModel> {
    // Uncaged: an active local engine makes `has_any_ai_remaining` always true
    // (see `crate::uncaged::engine_active`). Pin it off so these tests exercise
    // the Warp credit/BYOK logic regardless of the machine's ~/.uncaged config.
    crate::uncaged::set_engine_active_for_test(Some(false));
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    register_user_preferences_for_tests(app);
    app.update(|ctx| {
        warpui_extras::secure_storage::register_noop("test", ctx);
        ctx.add_singleton_model(ApiKeyManager::new);
    });
    app.add_singleton_model(|ctx| {
        AIRequestUsageModel::new_for_test(ServerApiProvider::as_ref(ctx).get_ai_client(), ctx)
    })
}

#[test]
fn test_request_limit_info() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 200,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() + Duration::days(1)),
                is_unlimited: false,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(200, request_usage_model.request_limit());
            assert_eq!(39, request_usage_model.requests_used());
            assert_eq!(161, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_request_limit_info_with_limit() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 999999999,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() + Duration::minutes(1)),
                is_unlimited: false,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(999999999, request_usage_model.request_limit());
            assert_eq!(39, request_usage_model.requests_used());
            assert_eq!(999999960, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_request_limit_info_past_refresh_time() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 200,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() - Duration::seconds(1)),
                is_unlimited: false,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(200, request_usage_model.request_limit());
            assert_eq!(0, request_usage_model.requests_used());
            assert_eq!(200, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_request_limit_info_is_unlimited_true() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 999999999,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() + Duration::minutes(1)),
                is_unlimited: true,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(999999999, request_usage_model.request_limit());
            assert_eq!(39, request_usage_model.requests_used());
            assert_eq!(999999999, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_has_any_ai_remaining_true_with_remaining_requests() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _| {
            // Some requests remaining, no bonus or overages needed.
            model.request_limit_info = RequestLimitInfo::new_for_test(10, 5);
            assert!(model.has_any_ai_remaining());
        });
    });
}

#[test]
fn test_ambient_credits_banner_dismissal_is_persisted() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, ctx| {
            assert!(!model.is_ambient_credits_banner_dismissed());
            model.dismiss_ambient_credits_banner(ctx);
            assert!(model.is_ambient_credits_banner_dismissed());
        });

        app.update(|ctx| {
            let stored_value = ctx
                .private_user_preferences()
                .read_value(AMBIENT_CREDITS_BANNER_DISMISSED_KEY)
                .unwrap();
            assert_eq!(stored_value, Some("true".to_owned()));
        });
    });
}

#[test]
fn test_ambient_credits_banner_dismissal_loads_from_preferences() {
    App::test((), |mut app| async move {
        register_user_preferences_for_tests(&mut app);
        app.update(|ctx| {
            ctx.private_user_preferences()
                .write_value(AMBIENT_CREDITS_BANNER_DISMISSED_KEY, "true".to_owned())
                .unwrap();
        });

        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _ctx| {
            assert!(model.is_ambient_credits_banner_dismissed());
        });
    });
}
#[test]
fn test_has_any_ai_remaining_false_when_no_requests_or_bonus() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _| {
            // At limit, no bonus credits and no overages.
            model.request_limit_info = RequestLimitInfo::new_for_test(10, 10);
            assert!(!model.has_any_ai_remaining());
        });
    });
}

#[test]
fn test_has_any_ai_remaining_true_when_local_engine_active() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        // Uncaged: the local engine means unlimited local AI, no Warp credits needed.
        crate::uncaged::set_engine_active_for_test(Some(true));

        request_usage_model.update(&mut app, |model, _| {
            model.request_limit_info = RequestLimitInfo::new_for_test(10, 10);
            assert!(
                model.has_any_ai_remaining(),
                "expected has_any_ai_remaining to be true when the local engine is active",
            );
        });
    });
}

#[test]
fn test_has_any_ai_remaining_true_with_user_bonus_credits() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _| {
            // No standard requests remaining.
            model.request_limit_info = RequestLimitInfo::new_for_test(10, 10);

            // User-level bonus credits remaining.
            model.bonus_grants = vec![BonusGrant {
                created_at: Utc::now(),
                cost_cents: 0,
                expiration: Some(Utc::now() + chrono::Duration::days(7)),
                grant_type: BonusGrantType::Any,
                reason: "test user bonus".to_string(),
                user_facing_message: None,
                request_credits_granted: 5,
                request_credits_remaining: 5,
                scope: BonusGrantScope::User,
            }];

            assert!(
                model.has_any_ai_remaining(),
                "expected has_any_ai_remaining to be true when user bonus credits exist",
            );
        });
    });
}

#[test]
fn test_has_any_ai_remaining_false_with_only_ambient_bonus_credits() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _| {
            // No standard requests remaining.
            model.request_limit_info = RequestLimitInfo::new_for_test(10, 10);

            // Only ambient-only bonus credits.
            model.bonus_grants = vec![BonusGrant {
                created_at: Utc::now(),
                cost_cents: 0,
                expiration: Some(Utc::now() + chrono::Duration::days(7)),
                grant_type: BonusGrantType::AmbientOnly,
                reason: "ambient trial credits".to_string(),
                user_facing_message: None,
                request_credits_granted: 1000,
                request_credits_remaining: 1000,
                scope: BonusGrantScope::User,
            }];

            assert!(
                !model.has_any_ai_remaining(),
                "expected has_any_ai_remaining to be false when only ambient-only bonus credits exist",
            );
        });
    });
}
