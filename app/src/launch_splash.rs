//! Uncaged: the short splash the first window shows when the app starts.
//!
//! It is the welcome screen's mark and shimmering title, drawn over the workspace while
//! the workspace starts underneath it, then faded away. It never holds the app back: the
//! terminal behind it is live from the first frame, so typing during the splash lands in
//! the shell as usual.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use settings::Setting as _;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warpui::elements::shimmering_text::{
    ShimmerConfig, ShimmeringTextElement, ShimmeringTextStateHandle,
};
use warpui::elements::{
    Align, ConstrainedBox, Container, CrossAxisAlignment, Flex, MainAxisAlignment, MainAxisSize,
    ParentElement,
};
use warpui::{AppContext, Element, SingletonEntity as _};

use crate::appearance::Appearance;
use crate::channel::{Channel, ChannelState};
use crate::window_settings::WindowSettings;

/// The mark and title fade in over this long.
const ENTER: Duration = Duration::from_millis(180);
/// How long the splash stays fully visible after fading in.
const HOLD: Duration = Duration::from_millis(520);
/// The whole splash fades out over this long.
const EXIT: Duration = Duration::from_millis(320);
/// How often the splash redraws while it animates (about 60 frames a second).
pub const FRAME: Duration = Duration::from_millis(16);
/// The Uncaged ember, the same on every theme (matches the welcome screen).
const EMBER: u32 = 0xFF7A18FF;

/// Only the app's first window gets the splash; later windows open straight away.
static SHOWN: AtomicBool = AtomicBool::new(false);

pub struct LaunchSplash {
    started: Instant,
    shimmer: ShimmeringTextStateHandle,
}

impl LaunchSplash {
    /// A splash for the first window of this run, if the user hasn't turned it off.
    pub fn for_first_window(app: &AppContext) -> Option<Self> {
        if !*WindowSettings::as_ref(app).show_launch_animation.value() {
            return None;
        }
        // Integration tests click and type from their first frame; a splash on top would swallow
        // those clicks.
        if ChannelState::channel() == Channel::Integration {
            return None;
        }
        if SHOWN.swap(true, Ordering::Relaxed) {
            return None;
        }
        Some(Self {
            started: Instant::now(),
            shimmer: ShimmeringTextStateHandle::new(),
        })
    }

    pub fn is_finished(&self) -> bool {
        self.started.elapsed() >= ENTER + HOLD + EXIT
    }

    pub fn render(&self, appearance: &Appearance) -> Box<dyn Element> {
        let (content_opacity, backdrop_opacity) = opacities(self.started.elapsed());
        let theme = appearance.theme();
        let background = theme.background().into_solid();

        let logo = ConstrainedBox::new(
            Icon::Oz
                .to_warpui_icon(ColorU::from_u32(EMBER).into())
                .with_opacity(content_opacity)
                .finish(),
        )
        .with_width(56.)
        .with_height(56.)
        .finish();

        let base: ColorU = internal_colors::text_sub(theme, background).into();
        let ink: ColorU = theme.foreground().into();
        let title = ShimmeringTextElement::new(
            "Uncaged",
            appearance.ui_font_family(),
            30.,
            with_alpha(base, content_opacity),
            with_alpha(ink, content_opacity),
            ShimmerConfig {
                // One sweep across the word while the splash is up.
                period: ENTER + HOLD,
                shimmer_radius: 4,
                padding: 4,
            },
            self.shimmer.clone(),
        )
        .finish();

        let mark = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(logo)
            .with_child(Container::new(title).with_margin_top(14.).finish())
            .finish();

        Container::new(Align::new(mark).finish())
            .with_background_color(with_alpha(background, backdrop_opacity))
            .finish()
    }
}

/// How visible the mark and the backdrop are `elapsed` into the splash, each from 0 to 1.
pub(crate) fn opacities(elapsed: Duration) -> (f32, f32) {
    let t = elapsed.as_secs_f32();
    let enter = ENTER.as_secs_f32();
    let exit_start = (ENTER + HOLD).as_secs_f32();
    let exit = EXIT.as_secs_f32();

    if t < enter {
        (ease_out(t / enter), 1.)
    } else if t < exit_start {
        (1., 1.)
    } else {
        let left = 1. - ease_out(((t - exit_start) / exit).min(1.));
        // The mark leaves a little ahead of the backdrop, so it doesn't hang in mid-air.
        (left * left, left)
    }
}

fn ease_out(x: f32) -> f32 {
    1. - (1. - x).powi(3)
}

fn with_alpha(color: ColorU, opacity: f32) -> ColorU {
    ColorU {
        a: (color.a as f32 * opacity.clamp(0., 1.)).round() as u8,
        ..color
    }
}

#[cfg(test)]
#[path = "launch_splash_tests.rs"]
mod tests;
