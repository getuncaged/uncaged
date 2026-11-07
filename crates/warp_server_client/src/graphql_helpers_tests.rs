use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cynic::GraphQlResponse;
use futures::executor::block_on;
use warp_graphql::client::{GraphQLError, RequestOptions};

use super::send_graphql_request;
use crate::auth::AuthEvent;
use crate::base_client::{AuthenticatedGraphqlConfig, BaseClient, GraphqlRoutingConfig};

fn base_client() -> (BaseClient, async_channel::Receiver<AuthEvent>) {
    let (event_sender, event_receiver) = async_channel::unbounded();
    (
        BaseClient::new(
            Arc::new(http_client::Client::new()),
            event_sender,
            None,
            GraphqlRoutingConfig::default(),
            AuthenticatedGraphqlConfig::default(),
            None,
        ),
        event_receiver,
    )
}

fn assert_no_events(event_receiver: &async_channel::Receiver<AuthEvent>) {
    assert!(event_receiver.try_recv().is_err());
}

struct FakeGraphqlOperation {
    expected_auth_token: Option<String>,
    send_count: Arc<AtomicUsize>,
    result: FakeGraphqlResult,
}

enum FakeGraphqlResult {
    Success,
}

impl FakeGraphqlOperation {
    fn successful(expected_auth_token: Option<&str>, send_count: Arc<AtomicUsize>) -> Self {
        Self {
            expected_auth_token: expected_auth_token.map(ToOwned::to_owned),
            send_count,
            result: FakeGraphqlResult::Success,
        }
    }
}

impl warp_graphql::client::Operation<()> for FakeGraphqlOperation {
    fn operation_name(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed("FakeGraphqlOperation"))
    }

    fn send_request(
        self,
        _client: Arc<http_client::Client>,
        options: RequestOptions,
    ) -> Pin<
        Box<
            dyn Future<Output = std::result::Result<GraphQlResponse<()>, GraphQLError>>
                + Send
                + 'static,
        >,
    >
    where
        Self: Sized,
    {
        Box::pin(async move {
            assert_eq!(options.auth_token, self.expected_auth_token);
            self.send_count.fetch_add(1, Ordering::SeqCst);
            match self.result {
                FakeGraphqlResult::Success => Ok(GraphQlResponse {
                    data: Some(()),
                    errors: None,
                }),
            }
        })
    }
}

fn has_error_message(error: &anyhow::Error, expected: &str) -> bool {
    error.chain().any(|cause| cause.to_string() == expected)
}

#[test]
fn missing_request_credentials_returns_before_sending() {
    let (base_client, event_receiver) = base_client();
    let send_count = Arc::new(AtomicUsize::new(0));

    let error = block_on(send_graphql_request(
        &base_client,
        FakeGraphqlOperation::successful(Some("unused-token"), send_count.clone()),
        None,
    ))
    .unwrap_err();

    assert!(has_error_message(
        &error,
        "missing authentication credentials"
    ));
    assert_eq!(send_count.load(Ordering::SeqCst), 0);
    assert_no_events(&event_receiver);
}
