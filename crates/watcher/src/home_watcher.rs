use std::path::PathBuf;
use std::time::Duration;

use notify_debouncer_full::notify::{RecursiveMode, WatchFilter};
use warpui_core::{Entity, ModelContext, ModelHandle, SingletonEntity};

use crate::{BulkFilesystemWatcher, BulkFilesystemWatcherEvent};

/// Duration between filesystem watch events for the home directory watcher, in milliseconds.
// Uncaged: notify-debouncer-full runs a tick thread that polls at this interval
// whether or not events are pending, so this value is an idle wakeup rate, not just a
// coalescing window. Longer is strictly better for a background index: it coalesces
// more, wakes less, and the only cost is how stale the index can be, which nothing
// user-visible depends on.
// Was 500ms. This one watches $HOME recursively, so it is also the widest.
const HOME_WATCHER_DEBOUNCE_MILLI_SECS: u64 = 2000;

pub enum HomeDirectoryWatcherEvent {
    /// Files directly under the home directory were changed.
    HomeFilesChanged(BulkFilesystemWatcherEvent),
}

/// Watches the user's home directory non-recursively for file changes.
pub struct HomeDirectoryWatcher {
    _watcher: ModelHandle<BulkFilesystemWatcher>,
}

impl HomeDirectoryWatcher {
    pub fn new(home_dir: PathBuf, ctx: &mut ModelContext<Self>) -> Self {
        let watcher = ctx.add_model(|ctx| {
            BulkFilesystemWatcher::new(Duration::from_millis(HOME_WATCHER_DEBOUNCE_MILLI_SECS), ctx)
        });
        ctx.subscribe_to_model(&watcher, Self::handle_fs_event);

        // Register the home directory as a non-recursive watch.
        watcher.update(ctx, |watcher, _ctx| {
            std::mem::drop(watcher.register_path(
                &home_dir,
                WatchFilter::accept_all(),
                RecursiveMode::NonRecursive,
            ));
        });

        Self { _watcher: watcher }
    }

    /// Test-only constructor that uses a stub filesystem watcher with no background thread,
    pub fn new_for_test(ctx: &mut ModelContext<Self>) -> Self {
        let watcher = ctx.add_model(|_| BulkFilesystemWatcher::new_for_test());
        Self { _watcher: watcher }
    }

    /// Forwards filesystem events to home directory watcher subscribers.
    fn handle_fs_event(
        &mut self,
        _: ModelHandle<BulkFilesystemWatcher>,
        event: &BulkFilesystemWatcherEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        ctx.emit(HomeDirectoryWatcherEvent::HomeFilesChanged(event.clone()));
    }
}

impl Entity for HomeDirectoryWatcher {
    type Event = HomeDirectoryWatcherEvent;
}

impl SingletonEntity for HomeDirectoryWatcher {}
