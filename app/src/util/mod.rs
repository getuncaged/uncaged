pub mod bindings;
#[cfg(feature = "local_fs")]
pub mod file;
pub(crate) mod link_detection;
pub mod openable_file_type;
pub mod tooltips;
pub(crate) mod traffic_lights;
#[cfg(windows)]
pub mod windows;

pub use uncaged_app_widgets::util::{
    clipboard, color, dedupe_from_last, extensions, git, image, keybinding_events, links,
    merge_ranges, parse_ascii_u32, path, repo_detection, time_format, truncation, vm_detection,
    AsciiDebug,
};
