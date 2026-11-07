use std::borrow::Cow;

use anyhow::{Result, anyhow};
use instant::Duration;
use warp_graphql::client::{GraphQLError, Operation};
use warpui_core::r#async::BoxFuture;

use crate::auth::AuthEvent;
use crate::base_client::BaseClient;

/// Sends a GraphQL operation through a base client supplied by the application.
///
/// This function is deliberately generic so concrete endpoint operation
/// instantiations occur in server client crates rather than in the app crate.
pub fn send_graphql_request<'a, QF: 'a, O>(
    base_client: &'a BaseClient,
    operation: O,
    timeout: Option<Duration>,
) -> BoxFuture<'a, Result<QF>>
where
    O: Operation<QF> + Send + 'a,
{
    Box::pin(async move {
        let operation_name = operation.operation_name().map(Cow::into_owned);
        let options = base_client.graphql_request_options(timeout).await?;
        let response = match operation
            .send_request(base_client.owned_http_client(), options)
            .await
        {
            Ok(response) => response,
            Err(GraphQLError::StagingAccessBlocked) => {
                let _ = base_client.send_auth_event(AuthEvent::StagingAccessBlocked);
                anyhow::bail!(GraphQLError::StagingAccessBlocked);
            }
            Err(GraphQLError::IapChallengeBlocked) => {
                let _ = base_client.send_auth_event(AuthEvent::IapChallengeReceived);
                anyhow::bail!(GraphQLError::IapChallengeBlocked);
            }
            Err(err) => {
                anyhow::bail!(err);
            }
        };

        if let Some(errors) = response.errors.as_ref() {
            warp_core::safe_error!(
                safe: ("graphql response for {:?} had errors", operation_name),
                full: ("graphql response for {:?} had errors {:?}", operation_name, errors)
            );
        }

        response.data.ok_or_else(|| {
            let operation_label = operation_name
                .as_deref()
                .unwrap_or("unknown GraphQL operation");
            let error_messages = response
                .errors
                .as_ref()
                .map(|errors| {
                    errors
                        .iter()
                        .filter_map(|error| {
                            let message = error.message.trim();
                            (!message.is_empty()).then(|| message.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .filter(|messages| !messages.is_empty());
            match error_messages {
                Some(messages) => {
                    anyhow!("missing response data for {operation_label}: {messages}")
                }
                None => anyhow!("missing response data for {operation_label}"),
            }
        })
    })
}

#[cfg(test)]
#[path = "graphql_helpers_tests.rs"]
mod tests;
