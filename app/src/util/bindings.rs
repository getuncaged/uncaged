//! Keybinding helpers. The read-only lookups live in `uncaged_app_widgets`; the functions that
//! persist user keybindings to the config file stay here because they need `crate::keyboard`.

pub use uncaged_app_widgets::util::bindings::*;
use uncaged_ui::keymap::{Keystroke, Trigger};
use uncaged_ui::{AppContext, SingletonEntity};

use crate::keyboard::{remove_custom_keybinding, write_custom_keybinding, UserDefinedKeybinding};
use crate::settings_view::keybindings::{KeybindingChangedEvent, KeybindingChangedNotifier};

/// Sets a custom keybinding for an editable binding using the given keystroke. Will
/// persist the keybinding to the user's config file and emit a KeybindingChangedEvent.
pub fn set_custom_keybinding(binding_name: &str, keystroke: &Keystroke, ctx: &mut AppContext) {
    ctx.set_custom_trigger(
        binding_name.into(),
        Trigger::Keystrokes(vec![keystroke.clone()]),
    );
    write_custom_keybinding(
        binding_name.into(),
        UserDefinedKeybinding::keystroke(keystroke.clone()),
    );
    KeybindingChangedNotifier::handle(ctx).update(ctx, |_, ctx| {
        ctx.emit(KeybindingChangedEvent::BindingChanged {
            binding_name: binding_name.into(),
            new_trigger: Some(keystroke.clone()),
        })
    });
}

/// Reset an editable binding back to its default trigger. Will persist this change to
/// the user's config file and emit a KeybindingChangedEvent with the default trigger.
/// Returns the default keystroke for the binding.
pub fn reset_keybinding_to_default(binding_name: &str, ctx: &mut AppContext) -> Option<Keystroke> {
    ctx.remove_custom_trigger(binding_name);
    remove_custom_keybinding(binding_name);

    let default_keystroke = ctx
        .editable_bindings()
        .find(|binding| binding.name == binding_name)
        .and_then(|binding| trigger_to_keystroke(binding.trigger));

    KeybindingChangedNotifier::handle(ctx).update(ctx, |_, ctx| {
        ctx.emit(KeybindingChangedEvent::BindingChanged {
            binding_name: binding_name.into(),
            new_trigger: default_keystroke.clone(),
        })
    });

    default_keystroke
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;
