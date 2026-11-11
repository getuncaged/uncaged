use std::ffi::OsStr;

use sysinfo::ProcessesToUpdate;
use uncaged_ui::{Entity, SingletonEntity};

/// On-demand access to process information.
///
/// Nothing is refreshed in the background: an earlier version polled the current process every
/// 5 seconds for heap-usage tracking that nothing consumed, which kept an idle app waking up.
/// Callers refresh what they need right before reading it.
pub struct SystemInfo {
    /// A structure we can use to efficiently query system information.
    system: sysinfo::System,
}

impl SystemInfo {
    pub fn new() -> Self {
        Self {
            system: sysinfo::System::new(),
        }
    }

    /// Returns the [`sysinfo::ProcessRefreshKind`] used when refreshing processes.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn refresh_kind() -> sysinfo::ProcessRefreshKind {
        sysinfo::ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn refresh_all_processes(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true, /* remove_dead_processes */
            Self::refresh_kind(),
        );
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn processes_by_name<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a sysinfo::Process> {
        self.system.processes_by_name(OsStr::new(name))
    }
}

impl Entity for SystemInfo {
    type Event = ();
}

impl SingletonEntity for SystemInfo {}
