//! A singleton model for storing conversations by ID to enable restoration across terminal views.

use std::cell::{Cell, OnceCell};
use std::collections::HashMap;

use uncaged_ui::{Entity, SingletonEntity};

use crate::ai::agent::conversation::{AIConversation, AIConversationId};
use crate::ai::blocklist::history_model::convert_persisted_conversation_to_ai_conversation_with_metadata;
use crate::persistence::model::AgentConversation;

/// Singleton model that holds restored agent conversations on app startup.
///
/// Loading restored conversations into this model is a means of propagating restored data from
/// sqlite (read at startup) to arbitrary consuming locations in the view/model hierarchy without
/// piping it all the way from the root view to the terminal view(s) that require it.
#[derive(Default)]
pub struct RestoredAgentConversations {
    /// All conversations stored by their ID, available for restoration
    conversations: HashMap<AIConversationId, RestoredConversation>,
}

/// A persisted conversation that is converted to an [`AIConversation`] the first time it is read.
/// Most restored conversations are never opened again, and converting all of them up front is a
/// noticeable part of startup.
struct RestoredConversation {
    persisted: Cell<Option<AgentConversation>>,
    /// Set on first read; `None` inside if the conversion failed.
    converted: OnceCell<Option<AIConversation>>,
}

impl RestoredConversation {
    fn new(persisted: AgentConversation) -> Self {
        Self {
            persisted: Cell::new(Some(persisted)),
            converted: OnceCell::new(),
        }
    }

    fn get(&self) -> Option<&AIConversation> {
        self.converted
            .get_or_init(|| self.persisted.take().and_then(convert_conversation))
            .as_ref()
    }

    fn into_conversation(self) -> Option<AIConversation> {
        match self.converted.into_inner() {
            Some(converted) => converted,
            None => self.persisted.into_inner().and_then(convert_conversation),
        }
    }
}

fn convert_conversation(conversation: AgentConversation) -> Option<AIConversation> {
    let conversation_id = conversation.conversation.conversation_id.clone();
    let converted = convert_persisted_conversation_to_ai_conversation_with_metadata(conversation);
    if converted.is_none() {
        log::warn!("Failed to convert persisted conversation {conversation_id} to AIConversation");
    }
    converted
}

impl RestoredAgentConversations {
    pub fn new(conversations: Vec<AgentConversation>) -> Self {
        let mut conversations_by_id = HashMap::new();
        for conversation in conversations.into_iter() {
            match AIConversationId::try_from(conversation.conversation.conversation_id.clone()) {
                Ok(conversation_id) => {
                    conversations_by_id
                        .insert(conversation_id, RestoredConversation::new(conversation));
                }
                // The conversion fails on an ID that doesn't parse; run it now so it logs why.
                Err(_) => {
                    convert_conversation(conversation);
                }
            }
        }

        Self {
            conversations: conversations_by_id,
        }
    }

    /// Gets a reference to a restored conversation without removing it.
    pub fn get_conversation(&self, id: &AIConversationId) -> Option<&AIConversation> {
        self.conversations.get(id)?.get()
    }

    /// Removes the restored conversation and returns it, if any.
    pub fn take_conversation(&mut self, id: &AIConversationId) -> Option<AIConversation> {
        self.conversations.remove(id)?.into_conversation()
    }

    /// Takes and returns AIConversations for the given IDs, sorted by first exchange start time.
    pub fn take_conversations(
        &mut self,
        conversation_ids: &[AIConversationId],
    ) -> Vec<AIConversation> {
        let mut conversations = Vec::new();
        for &conversation_id in conversation_ids {
            if let Some(conversation) = self.take_conversation(&conversation_id) {
                conversations.push(conversation);
            }
        }

        // Sort by first exchange start time (oldest first)
        conversations.sort_by_key(|conversation| {
            conversation
                .first_exchange()
                .map(|exchange| exchange.start_time)
        });
        conversations
    }
}

impl Entity for RestoredAgentConversations {
    type Event = ();
}

impl SingletonEntity for RestoredAgentConversations {}

#[cfg(test)]
#[path = "restored_conversations_tests.rs"]
mod tests;
