use std::collections::HashMap;
use std::sync::Arc;

use futures::executor::block_on;

use super::{
    AGENT_SOURCE_HEADER, AMBIENT_WORKLOAD_TOKEN_HEADER, AmbientHeaderPolicy,
    AuthenticatedGraphqlConfig, BaseClient, CLOUD_AGENT_ID_HEADER, GraphqlRoutingConfig,
    HeaderOverride,
};

struct StaticIapTokenProvider;

impl http_client::iap::IapTokenProvider for StaticIapTokenProvider {
    fn cached_token(&self) -> Option<String> {
        Some("iap-token".to_string())
    }
}

fn client() -> BaseClient {
    let (event_sender, _) = async_channel::unbounded();
    let mut authenticated_headers = HashMap::new();
    authenticated_headers.insert("X-Test-Authenticated".to_string(), "true".to_string());
    BaseClient::new(
        Arc::new(http_client::Client::new()),
        event_sender,
        Some("cloud_mode".to_string()),
        GraphqlRoutingConfig {
            path_prefix: Some("/routing-only".to_string()),
        },
        AuthenticatedGraphqlConfig {
            headers: authenticated_headers,
        },
        None,
    )
}

#[test]
fn iap_proxy_auth_header_uses_configured_provider() {
    let (event_sender, _) = async_channel::unbounded();
    let client = BaseClient::new(
        Arc::new(http_client::Client::new()),
        event_sender,
        None,
        GraphqlRoutingConfig::default(),
        AuthenticatedGraphqlConfig::default(),
        Some(Arc::new(StaticIapTokenProvider)),
    );

    assert_eq!(
        client.iap_proxy_auth_header(),
        Some((
            http_client::iap::IAP_PROXY_AUTH_HEADER,
            "Bearer iap-token".to_string()
        ))
    );
}

#[test]
fn explicit_token_graphql_options_route_without_authenticated_headers() {
    let client = client();
    client.set_ambient_agent_task_id(Some("ambient-task".to_string()));

    let options = client.graphql_request_options_with_token(Some("token".to_string()));

    assert_eq!(options.path_prefix.as_deref(), Some("/routing-only"));
    assert_eq!(options.auth_token.as_deref(), Some("token"));
    assert!(options.headers.is_empty());
}

#[test]
fn ambient_policy_supports_inherit_override_and_omit() {
    let client = client();
    client.set_ambient_agent_task_id(Some("ambient-task".to_string()));

    let inherited = block_on(client.ambient_headers(AmbientHeaderPolicy {
        workload_token: HeaderOverride::Set("workload".to_string()),
        cloud_agent_id: HeaderOverride::Inherit,
        agent_source: HeaderOverride::Inherit,
    }))
    .unwrap();
    assert!(inherited.contains(&(
        AMBIENT_WORKLOAD_TOKEN_HEADER.to_string(),
        "workload".to_string(),
    )));
    assert!(inherited.contains(&(
        CLOUD_AGENT_ID_HEADER.to_string(),
        "ambient-task".to_string()
    )));
    assert!(inherited.contains(&(AGENT_SOURCE_HEADER.to_string(), "cloud_mode".to_string())));

    let task_scoped = block_on(client.ambient_headers(AmbientHeaderPolicy {
        workload_token: HeaderOverride::Set("workload".to_string()),
        ..AmbientHeaderPolicy::for_task("specific-task")
    }))
    .unwrap();
    assert!(task_scoped.contains(&(
        CLOUD_AGENT_ID_HEADER.to_string(),
        "specific-task".to_string(),
    )));
    assert!(!task_scoped.contains(&(
        CLOUD_AGENT_ID_HEADER.to_string(),
        "ambient-task".to_string()
    )));

    let omitted = block_on(client.ambient_headers(AmbientHeaderPolicy::omit_all())).unwrap();
    assert!(omitted.is_empty());
}
