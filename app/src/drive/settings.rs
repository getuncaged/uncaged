use settings::macros::define_settings_group;
use settings::SupportedPlatforms;

use super::DriveSortOrder;

define_settings_group!(WarpDriveSettings, settings: [
    sorting_choice: WarpDriveSortingChoice {
        type: DriveSortOrder,
        default: DriveSortOrder::ByObjectType,
        supported_platforms: SupportedPlatforms::ALL,
        private: false,
        toml_path: "warp_drive.sorting_choice",
        description: "The sort order for items in Warp Drive.",
    },
    // Controls whether Warp Drive appears in the tools panel, command palette, and command search.
    enable_warp_drive: EnableWarpDrive {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        private: false,
        toml_path: "warp_drive.enabled",
        description: "Whether workspace is enabled.",
    },
]);

impl WarpDriveSettings {
    /// Returns whether Warp Drive should be considered enabled.
    /// Uncaged runs fully local (no login), so this is driven purely by the
    /// user setting — we never gate on auth/login state.
    pub fn is_warp_drive_enabled(app: &uncaged_ui::AppContext) -> bool {
        use uncaged_ui::SingletonEntity as _;
        *Self::as_ref(app).enable_warp_drive
    }
}
