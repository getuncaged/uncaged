//! Where a piece of UI was opened from, when that changes how it behaves.

/// The surface that opened the command palette.
#[derive(Clone, Copy, Debug)]
pub enum PaletteSource {
    Keybinding,
    CtrlTab { shift_pressed_initially: bool },
    WarpDrive,
    QuitModal,
    LogOutModal,
    IntegrationTest,
    ConversationManager,
    ContextChip,
    AgentTip,
    TitleBarSearchBar,
}

/// What opened the command X-ray description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandXRayTrigger {
    Keystroke,
    Hover,
}
