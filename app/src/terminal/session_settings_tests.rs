use super::{AgentToolbarChipSelection, CLIAgentToolbarChipSelection};
use crate::ai::blocklist::agent_view::toolbar_item::AgentToolbarItemKind;

#[test]
fn custom_agent_toolbar_drops_removed_share_session_item() {
    // `ShareSession` was a toolbar item for session sharing, which no longer exists. A stored
    // custom layout that still lists it should load with the remaining items intact.
    let value = serde_json::json!({
        "Custom": {
            "left": ["NLDToggle", "ShareSession"],
            "right": ["ShareSession", "ModelSelector"],
        }
    });

    let selection: AgentToolbarChipSelection =
        serde_json::from_value(value).expect("legacy toolbar layout should deserialize");

    assert_eq!(
        selection,
        AgentToolbarChipSelection::Custom {
            left: vec![AgentToolbarItemKind::NLDToggle],
            right: vec![AgentToolbarItemKind::ModelSelector],
        }
    );
}

#[test]
fn custom_cli_agent_toolbar_drops_removed_share_session_item() {
    let value = serde_json::json!({
        "Custom": {
            "left": ["FileAttach", "ShareSession", "FileExplorer"],
            "right": ["Settings"],
        }
    });

    let selection: CLIAgentToolbarChipSelection =
        serde_json::from_value(value).expect("legacy toolbar layout should deserialize");

    assert_eq!(
        selection,
        CLIAgentToolbarChipSelection::Custom {
            left: vec![
                AgentToolbarItemKind::FileAttach,
                AgentToolbarItemKind::FileExplorer,
            ],
            right: vec![AgentToolbarItemKind::Settings],
        }
    );
}
