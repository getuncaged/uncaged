use warp_core::settings::Setting;
use warpui::{AppContext, SingletonEntity};

use crate::report_if_error;
use crate::settings::AISettings;

/// Uncaged: input mode is a toggle, not a guess -- turn autodetection off once.
///
/// Upstream defaulted `ai_autodetection_enabled_internal` to true, and the first-run onboarding
/// callout offered an "Enable Natural Language Detection" checkbox that set
/// `nld_in_terminal_enabled_internal`. Both write an explicit value, and defaults only apply to
/// keys that are absent -- so changing the defaults reaches new installs only, while every install
/// that has ever run keeps classifying.
///
/// That classification is not cheap in a shipped build: `script/*/bundle` compiles in
/// `nld_classifier_v3`, so it is a BERT-tiny ONNX forward pass per keystroke, concentrated on the
/// first characters of a line (once the completer can describe the first token, the classifier
/// short-circuits). It can also move the input between Shell and AI after the user has already
/// chosen a mode.
///
/// So: clear both, exactly once, and record that we did. Anyone who wants the classifier back can
/// turn it on in Settings -> AI and it stays on -- the marker is already set, so this never runs
/// again and never overrides that choice.
///
/// Upstream ran its settings migrations only after a successful server user fetch, which never
/// happens in Uncaged (no accounts), so they are gone. This is called from `settings::init`
/// instead, which runs on every launch.
pub fn apply_uncaged_autodetection_reset(ctx: &mut AppContext) {
    AISettings::handle(ctx).update(ctx, |ai_settings, ctx| {
        if *ai_settings.uncaged_autodetection_reset_applied {
            return;
        }

        if *ai_settings.ai_autodetection_enabled_internal {
            report_if_error!(ai_settings
                .ai_autodetection_enabled_internal
                .set_value(false, ctx));
        }
        if *ai_settings.nld_in_terminal_enabled_internal {
            report_if_error!(ai_settings
                .nld_in_terminal_enabled_internal
                .set_value(false, ctx));
        }

        report_if_error!(ai_settings
            .uncaged_autodetection_reset_applied
            .set_value(true, ctx));
    });
}
