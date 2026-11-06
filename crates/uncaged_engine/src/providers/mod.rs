//! Backend providers: the thing that actually talks to a model.
//!
//! Every provider lowers a neutral `Conversation` into its own wire format,
//! streams the response, and raises it back into `ProviderEvent`s. The engine
//! is provider-agnostic above this line.

mod acp;
mod anthropic;
mod openai;
mod sse;

use futures::stream::BoxStream;
use serde_json::Value;

use crate::config::{ProviderConfig, UncagedConfig};
use crate::model::Conversation;

/// A normalized streaming event from any provider.
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    /// A chunk of assistant prose.
    TextDelta(String),
    /// A fully-assembled tool call the model wants to make.
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    /// The turn finished normally.
    Done,
    /// The provider failed; carries a user-facing message.
    Error(String),
}

/// A backend that can stream a turn. Implementations spawn their own work and
/// return a stream of events, so the engine just consumes.
pub trait Provider: Send + Sync {
    fn stream(&self, conversation: Conversation) -> BoxStream<'static, ProviderEvent>;
}

/// Construct the provider selected by config. `conversation_id` keys the CLI/ACP
/// session pool so a tab reuses one agent session across turns (ignored by the
/// stateless HTTP providers, which rebuild full history from each request).
/// `working_directory` is where a CLI agent's session works (also ignored by HTTP providers).
pub fn build(
    config: &UncagedConfig,
    conversation_id: &str,
    working_directory: Option<&str>,
) -> Box<dyn Provider> {
    match &config.provider {
        ProviderConfig::Anthropic {
            api_key,
            model,
            base_url,
            max_tokens,
            effort,
        } => Box::new(anthropic::AnthropicProvider {
            api_key: api_key.clone(),
            model: model.clone(),
            base_url: base_url.clone(),
            max_tokens: *max_tokens,
            effort: *effort,
        }),
        ProviderConfig::OpenAiCompatible {
            base_url,
            api_key,
            model,
            max_tokens,
            effort,
            ..
        } => Box::new(openai::OpenAiProvider {
            base_url: base_url.clone(),
            api_key: api_key.clone(),
            model: model.clone(),
            max_tokens: *max_tokens,
            effort: *effort,
        }),
        ProviderConfig::Acp {
            command,
            model,
            effort,
        } => Box::new(acp::AcpProvider {
            command: command.clone(),
            model: model.clone(),
            effort: *effort,
            conversation_id: conversation_id.to_string(),
            working_directory: working_directory.map(str::to_string),
        }),
    }
}
