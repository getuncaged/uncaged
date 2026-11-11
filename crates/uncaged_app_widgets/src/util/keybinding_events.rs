use uncaged_ui::keymap::Keystroke;
use uncaged_ui::{Entity, SingletonEntity};

/// Notifier for custom keybinding changed. Views could subscribe to this for
/// KeybindingChangedEvent.
#[derive(Default)]
pub struct KeybindingChangedNotifier {}

impl KeybindingChangedNotifier {
    pub fn new() -> Self {
        Default::default()
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn mock() -> Self {
        Self::new()
    }
}

pub enum KeybindingChangedEvent {
    BindingChanged {
        /// Name of the keybinding that is being changed.
        binding_name: String,
        new_trigger: Option<Keystroke>,
    },
}

impl Entity for KeybindingChangedNotifier {
    type Event = KeybindingChangedEvent;
}

impl SingletonEntity for KeybindingChangedNotifier {}
