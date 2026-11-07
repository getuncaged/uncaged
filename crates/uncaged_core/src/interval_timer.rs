use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use instant::Instant;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uncaged_ui_core::{Entity, SingletonEntity};

/// This represents one interval, i.e. one stage in a multiple-stage timer.
struct TimingInterval {
    /// Assign each interval a unique name.
    name: String,
    /// When this interval ended.
    instant: Instant,
}

impl TimingInterval {
    fn new(name: String, instant: Instant) -> Self {
        Self { name, instant }
    }
}

/// This is a collection of points in time for a multiple-stage process that we want to time, and
/// we want to measure durations between each stage, or "interval".
pub struct IntervalTimer {
    /// The timer starts when this struct is instantiated. The first interval is measured as a
    /// duration from this instant, with each subsequent interval being measured from the end of
    /// the prior interval.
    start_instant: Instant,
    intervals: Vec<TimingInterval>,
}

impl IntervalTimer {
    pub fn new() -> Self {
        Self {
            start_instant: Instant::now(),
            intervals: Vec::new(),
        }
    }

    pub fn mark_interval_end(&mut self, name: impl Into<String>) {
        let name = name.into();
        tracing::info!(name);
        self.intervals
            .push(TimingInterval::new(name, Instant::now()))
    }

    pub fn compute_duration_for_interval(&self, name: &str) -> Option<Duration> {
        self.intervals
            .iter()
            .enumerate()
            .find_map(|(idx, interval)| {
                if interval.name == name {
                    let since = if idx == 0 {
                        self.start_instant
                    } else {
                        self.intervals[idx - 1].instant
                    };

                    let marginal = interval.instant.duration_since(since);
                    Some(marginal)
                } else {
                    None
                }
            })
    }

    /// Once you are done with all the intervals in your process, we compute a cumulative sum of the
    /// time at each interval, as well as an individual time between each interval.
    pub fn compute_stats(&self) -> Vec<TimingDataPoint> {
        let mut cumulative_duration_ms = 0;
        self.intervals
            .iter()
            .enumerate()
            .map(|(i, interval)| {
                let since = if i == 0 {
                    self.start_instant
                } else {
                    self.intervals[i - 1].instant
                };
                // Converting a duration to an int in ms returns a u128 which is excessively large
                // for our purposes. It's also inconvenient as it isn't serializable by default.
                let marginal_duration_ms =
                    interval.instant.duration_since(since).as_millis() as u64;
                cumulative_duration_ms += marginal_duration_ms;
                TimingDataPoint::new(
                    marginal_duration_ms,
                    cumulative_duration_ms,
                    interval.name.clone(),
                )
            })
            .collect()
    }

    /// One line with every interval so far, e.g. `SQLITE_INITIALIZED +12ms (=40ms), ...`.
    pub fn summary(&self) -> String {
        self.summary_with_sub_intervals(&[])
    }

    /// Like [`Self::summary`], but also stops collecting startup sub-intervals (see
    /// [`mark_startup_sub_interval`]) and lists the ones collected so far after the interval they
    /// fall in, e.g. `SQLITE_INITIALIZED +12ms (=40ms) [open +1ms, migrations +3ms], ...`.
    pub fn startup_summary(&self) -> String {
        STARTUP_SUB_INTERVALS_DONE.store(true, Ordering::Relaxed);
        let sub_intervals = std::mem::take(&mut *STARTUP_SUB_INTERVALS.lock());
        self.summary_with_sub_intervals(&sub_intervals)
    }

    fn summary_with_sub_intervals(&self, sub_intervals: &[(&'static str, Instant)]) -> String {
        let mut sub_intervals = sub_intervals.iter().peekable();
        self.compute_stats()
            .iter()
            .zip(&self.intervals)
            .enumerate()
            .map(|(idx, (point, interval))| {
                let mut entry = format!(
                    "{} +{}ms (={}ms)",
                    point.name, point.marginal_duration_ms, point.cumulative_duration_ms
                );
                let mut since = if idx == 0 {
                    self.start_instant
                } else {
                    self.intervals[idx - 1].instant
                };
                let mut parts = Vec::new();
                while let Some((name, instant)) =
                    sub_intervals.next_if(|(_, instant)| *instant <= interval.instant)
                {
                    let marginal_ms = instant.saturating_duration_since(since).as_millis();
                    parts.push(format!("{name} +{marginal_ms}ms"));
                    since = *instant;
                }
                if !parts.is_empty() {
                    entry.push_str(&format!(" [{}]", parts.join(", ")));
                }
                entry
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Sub-intervals marked inside the startup stages, oldest first.
static STARTUP_SUB_INTERVALS: Mutex<Vec<(&'static str, Instant)>> = Mutex::new(Vec::new());

/// Set once the startup summary has been logged; later sub-intervals are dropped.
static STARTUP_SUB_INTERVALS_DONE: AtomicBool = AtomicBool::new(false);

/// Upper bound on collected sub-intervals, so code that also runs outside startup (e.g. in tests,
/// which never log a startup summary) can't grow the list without bound.
const MAX_STARTUP_SUB_INTERVALS: usize = 128;

/// Marks the end of a step inside the current startup stage. The startup timing line lists each
/// step's duration (since the previous step or stage) after the stage it falls in. Costs one
/// clock read and a push until the startup summary is logged, and nothing after.
pub fn mark_startup_sub_interval(name: &'static str) {
    if STARTUP_SUB_INTERVALS_DONE.load(Ordering::Relaxed) {
        return;
    }
    let mut sub_intervals = STARTUP_SUB_INTERVALS.lock();
    if sub_intervals.len() < MAX_STARTUP_SUB_INTERVALS {
        sub_intervals.push((name, Instant::now()));
    }
}

impl Default for IntervalTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for IntervalTimer {
    type Event = ();
}

impl SingletonEntity for IntervalTimer {}

/// Used for reporting the timing results after timing is complete.
#[derive(Clone, Deserialize, Serialize)]
pub struct TimingDataPoint {
    name: String,
    marginal_duration_ms: u64,
    cumulative_duration_ms: u64,
}

impl TimingDataPoint {
    fn new(marginal_duration_ms: u64, cumulative_duration_ms: u64, name: String) -> Self {
        Self {
            marginal_duration_ms,
            cumulative_duration_ms,
            name,
        }
    }
}

#[cfg(test)]
#[path = "interval_timer_tests.rs"]
mod tests;
