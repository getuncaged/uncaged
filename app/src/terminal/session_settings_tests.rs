use super::{AgentToolbarChipSelection, CLIAgentToolbarChipSelection};
use crate::ai::blocklist::agent_view::toolbar_item::AgentToolbarItemKind;

#[test]
fn custom_agent_toolbar_drops_removed_items() {
    // `ShareSession` (session sharing) and `VoiceInput` (voice dictation through Warp's server)
    // were toolbar items that no longer exist. A stored custom layout that still lists them
    // should load with the remaining items intact.
    let value = serde_json::json!({
        "Custom": {
            "left": ["NLDToggle", "ShareSession", "VoiceInput"],
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
fn custom_cli_agent_toolbar_drops_removed_items() {
    let value = serde_json::json!({
        "Custom": {
            "left": ["FileAttach", "VoiceInput", "ShareSession", "FileExplorer"],
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
