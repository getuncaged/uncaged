use chrono::Utc;

use super::*;
use crate::persistence::model::AgentConversationRecord;

fn persisted_conversation(conversation_id: String) -> AgentConversation {
    AgentConversation {
        conversation: AgentConversationRecord {
            id: 0,
            conversation_id,
            conversation_data: "{}".to_string(),
            last_modified_at: Utc::now().naive_utc(),
        },
        tasks: vec![],
    }
}

#[test]
fn conversations_are_converted_when_first_read() {
    let first = AIConversationId::new();
    let second = AIConversationId::new();
    let mut store = RestoredAgentConversations::new(vec![
        persisted_conversation(first.to_string()),
        persisted_conversation(second.to_string()),
        persisted_conversation("not-a-conversation-id".to_string()),
    ]);
    assert_eq!(store.conversations.len(), 2);
    assert!(store
        .conversations
        .values()
        .all(|conversation| conversation.converted.get().is_none()));

    assert_eq!(store.get_conversation(&first).map(|c| c.id()), Some(first));
    assert!(store.conversations[&first].converted.get().is_some());
    assert!(store.conversations[&second].converted.get().is_none());

    // Taking returns the conversation whether or not it was read before.
    assert_eq!(store.take_conversation(&first).map(|c| c.id()), Some(first));
    assert_eq!(
        store.take_conversation(&second).map(|c| c.id()),
        Some(second)
    );
    assert!(store.take_conversation(&first).is_none());
    assert!(store.get_conversation(&second).is_none());
}

#[test]
fn take_conversations_skips_missing_ids() {
    let restored = AIConversationId::new();
    let mut store =
        RestoredAgentConversations::new(vec![persisted_conversation(restored.to_string())]);
    let taken = store.take_conversations(&[AIConversationId::new(), restored]);
    assert_eq!(
        taken.iter().map(|c| c.id()).collect::<Vec<_>>(),
        vec![restored]
    );
    assert!(store.conversations.is_empty());
}
