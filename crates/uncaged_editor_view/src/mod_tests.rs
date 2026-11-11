//! Helper utilities for testing the editor.

use rand::Rng;
use settings::{PrivatePreferences, PublicPreferences, SettingsManager};
use uncaged_core::semantic_selection::SemanticSelection;
use uncaged_ui::{AddSingletonModel as _, App};
use uncaged_ui_extras::user_preferences::in_memory::InMemoryPreferences;

use crate::settings::{AppEditorSettings, SelectionSettings};

/// Registers the settings the editor view reads, backed by in-memory preferences.
pub fn initialize_settings_for_tests(app: &mut App) {
    app.update(|ctx| {
        ctx.add_singleton_model(|_| PublicPreferences::new(Box::<InMemoryPreferences>::default()));
        ctx.add_singleton_model(|_| PrivatePreferences::new(Box::<InMemoryPreferences>::default()));
    });
    app.add_singleton_model(|_ctx| SettingsManager::default());
    AppEditorSettings::register(app);
    SelectionSettings::register(app);
    SemanticSelection::register(app);
}

pub fn sample_text(rows: usize, cols: usize) -> String {
    let mut text = String::new();
    for row in 0..rows {
        let c: char = ('a' as u32 + row as u32) as u8 as char;
        let mut line = c.to_string().repeat(cols);
        if row < rows - 1 {
            line.push('\n');
        }
        text += &line;
    }
    text
}

pub struct RandomCharIter<T: Rng>(T);

impl<T: Rng> RandomCharIter<T> {
    pub fn new(rng: T) -> Self {
        Self(rng)
    }
}

impl<T: Rng> Iterator for RandomCharIter<T> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.gen_bool(1.0 / 5.0) {
            Some('\n')
        } else {
            Some(self.0.gen_range(b'a'..b'z' + 1).into())
        }
    }
}
