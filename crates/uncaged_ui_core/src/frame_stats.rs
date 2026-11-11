//! Opt-in frame timing, for measuring how much work each redraw costs.
//!
//! Set `UNCAGED_FRAME_STATS=1` to log, every two seconds in which something was drawn, how many
//! frames were drawn and how long building their scenes (layout and paint) and encoding them for
//! the GPU took. With the variable unset this is a single cached check per frame.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::Duration;

use instant::Instant;

const REPORT_INTERVAL: Duration = Duration::from_secs(2);

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("UNCAGED_FRAME_STATS").is_some_and(|v| v != "0"))
}

#[derive(Default)]
struct Stats {
    since: Option<Instant>,
    scene: Vec<Duration>,
    render: Vec<Duration>,
}

thread_local! {
    static STATS: RefCell<Stats> = RefCell::default();
}

/// Records the time spent building one scene.
pub fn record_scene(elapsed: Duration) {
    STATS.with_borrow_mut(|stats| stats.scene.push(elapsed));
}

/// Records the time spent encoding one frame and logs a summary when one is due.
pub fn record_render(elapsed: Duration) {
    STATS.with_borrow_mut(|stats| {
        stats.render.push(elapsed);
        let since = *stats.since.get_or_insert_with(Instant::now);
        if since.elapsed() < REPORT_INTERVAL {
            return;
        }
        log::info!(
            "Frame stats: {} frames in {:.1}s; scene {}; render {}",
            stats.render.len(),
            since.elapsed().as_secs_f32(),
            summarize(&mut stats.scene),
            summarize(&mut stats.render),
        );
        *stats = Stats::default();
    });
}

fn summarize(samples: &mut [Duration]) -> String {
    if samples.is_empty() {
        return "n/a".to_owned();
    }
    samples.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1000.;
    let mean = samples.iter().sum::<Duration>() / samples.len() as u32;
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    let max = samples[samples.len() - 1];
    format!(
        "mean {:.2}ms p95 {:.2}ms max {:.2}ms",
        ms(mean),
        ms(p95),
        ms(max)
    )
}
