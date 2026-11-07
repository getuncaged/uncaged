/// Returns the full memory footprint of the current process, in bytes.
///
/// Unlike RSS (resident set size), this includes memory that has been swapped
/// out or compressed by the OS.  On macOS, this returns `phys_footprint` from
/// `task_info(TASK_VM_INFO)`, which is the same value displayed by Activity
/// Monitor.
pub fn memory_footprint_bytes() -> u64 {
    platform::memory_footprint_bytes()
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod platform {
    use std::mem;

    use mach2::kern_return::KERN_SUCCESS;
    use mach2::task::task_info;
    use mach2::task_info::{task_vm_info, TASK_VM_INFO};
    use mach2::traps::mach_task_self;

    /// Calls `task_info(TASK_VM_INFO)` and returns the populated struct on
    /// success, or `None` if the call fails.
    fn query_task_vm_info() -> Option<task_vm_info> {
        // SAFETY: We zero-initialise the struct and pass its exact size to the
        // kernel.  `task_info` writes into the struct up to `count` natural
        // ints and returns `KERN_SUCCESS` on success.
        unsafe {
            let mut info: task_vm_info = mem::zeroed();
            let mut count = (mem::size_of::<task_vm_info>() / mem::size_of::<i32>()) as u32;
            let kr = task_info(
                mach_task_self(),
                TASK_VM_INFO,
                &mut info as *mut _ as *mut i32,
                &mut count,
            );
            if kr == KERN_SUCCESS {
                Some(info)
            } else {
                None
            }
        }
    }

    pub fn memory_footprint_bytes() -> u64 {
        query_task_vm_info()
            .map(|info| info.phys_footprint)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod platform {
    /// Reads `/proc/self/status` and sums `VmRSS` + `VmSwap` to approximate
    /// the full memory footprint (resident + swapped).
    pub fn memory_footprint_bytes() -> u64 {
        read_proc_self_status().unwrap_or(0)
    }

    fn read_proc_self_status() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let mut rss_kb: u64 = 0;
        let mut swap_kb: u64 = 0;
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                rss_kb = parse_kb(value);
            } else if let Some(value) = line.strip_prefix("VmSwap:") {
                swap_kb = parse_kb(value);
            }
        }
        Some((rss_kb + swap_kb) * 1024)
    }

    fn parse_kb(s: &str) -> u64 {
        // Lines look like "VmRSS:    12345 kB"
        s.split_whitespace()
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// FreeBSD
// ---------------------------------------------------------------------------

#[cfg(target_os = "freebsd")]
mod platform {
    /// FreeBSD has no `/proc/self/status` by default (linprocfs is optional and
    /// rarely mounted), so we use `getrusage(RUSAGE_SELF)`. `ru_maxrss` is
    /// reported in kilobytes and represents the maximum resident set size, not
    /// the current value, but it's the closest portable signal we have without
    /// pulling in `kvm`/`sysctl(KERN_PROC_PID)` plumbing.
    pub fn memory_footprint_bytes() -> u64 {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
            return 0;
        }
        (usage.ru_maxrss as u64).saturating_mul(1024)
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use std::mem;

    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;

    #[repr(C)]
    struct ProcessMemoryCountersEx {
        base: PROCESS_MEMORY_COUNTERS,
        private_usage: usize,
    }

    /// Uses `GetProcessMemoryInfo` to read `PrivateUsage` from
    /// `PROCESS_MEMORY_COUNTERS_EX`, which accounts for private committed
    /// memory (resident + paged out).
    ///
    /// The `windows` crate doesn't expose `PROCESS_MEMORY_COUNTERS_EX`
    /// directly, but it is layout-compatible with `PROCESS_MEMORY_COUNTERS`
    /// plus one trailing `usize` field (`PrivateUsage`).  We define a minimal
    /// wrapper to read that field.
    pub fn memory_footprint_bytes() -> u64 {
        query_counters()
            .map(|c| c.private_usage as u64)
            .unwrap_or(0)
    }

    fn query_counters() -> Option<ProcessMemoryCountersEx> {
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that does not
        // need to be closed.  `K32GetProcessMemoryInfo` writes into the
        // provided struct up to `cb` bytes.
        unsafe {
            let handle = GetCurrentProcess();
            let mut counters: ProcessMemoryCountersEx = mem::zeroed();
            counters.base.cb = mem::size_of::<ProcessMemoryCountersEx>() as u32;
            if K32GetProcessMemoryInfo(handle, &mut counters.base, counters.base.cb).as_bool() {
                Some(counters)
            } else {
                None
            }
        }
    }
}
