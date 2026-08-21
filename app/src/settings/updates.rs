//! Uncaged: automatic-update preferences.
//!
//! Two booleans rather than one, because "off" and "not asked yet" must be
//! different states. A single `auto_update = false` cannot tell the difference
//! between a user who declined and a user who has never seen the question, and
//! the whole design rests on making no network request until they answer.
//!
//! * `auto_update_prompt_answered` — has the user been asked?
//! * `auto_update_enabled`         — what did they say?
//!
//! Both default to false, so a fresh install is "not asked, not checking" and
//! stays completely offline until the user chooses. See
//! `app/src/autoupdate/mod.rs` for the gate that reads them.
//!
//! Not synced to the cloud: Uncaged has no cloud. `SyncToCloud::Never` is the
//! only honest value here, and it keeps the setting from being round-tripped by
//! any inherited syncing machinery.

use settings::macros::define_settings_group;
use settings::{SupportedPlatforms, SyncToCloud};

define_settings_group!(UpdateSettings, settings: [
   auto_update_enabled: AutoUpdateEnabled {
       type: bool,
       default: false,
       supported_platforms: SupportedPlatforms::ALL,
       sync_to_cloud: SyncToCloud::Never,
       private: false,
       toml_path: "general.auto_update_enabled",
       description: "Check GitHub for new Uncaged releases and install them. Off until you turn it on; nothing is requested before then.",
   },
   auto_update_prompt_answered: AutoUpdatePromptAnswered {
       type: bool,
       default: false,
       supported_platforms: SupportedPlatforms::ALL,
       sync_to_cloud: SyncToCloud::Never,
       private: false,
       toml_path: "general.auto_update_prompt_answered",
       description: "Whether the one-time 'check for updates?' question has been answered. Internal; set by answering the prompt.",
   },
]);
