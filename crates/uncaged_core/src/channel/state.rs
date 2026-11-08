use std::borrow::Cow;
use std::collections::HashSet;

use lazy_static::lazy_static;
use parking_lot::Mutex;
use url::{Origin, Url};

use super::Channel;
use crate::channel::config::{ChannelConfig, McpOAuthProviderConfig, WarpServerConfig};
use crate::features::FeatureFlag;
use crate::AppId;

lazy_static! {
    static ref CHANNEL_STATE: Mutex<ChannelState> = Mutex::new(ChannelState::init());
}

#[cfg(feature = "test-util")]
lazy_static! {
    static ref MOCK_SERVER: Mutex<mockito::ServerGuard> = Mutex::new(mockito::Server::new());
    static ref MOCK_SERVER_URL: String = MOCK_SERVER.lock().url();
    static ref APP_VERSION: Mutex<Option<&'static str>> = Mutex::new(None);
}

#[derive(Debug)]
pub struct ChannelState {
    channel: Channel,

    /// The set of additional features to enable (on top of default-enabled ones).
    additional_features: HashSet<FeatureFlag>,

    config: ChannelConfig,

    /// The app ID persisted data is namespaced under, when it differs from
    /// `config.app_id` for this run (see [`Self::set_data_app_id`]).
    data_app_id: Option<AppId>,
}

impl ChannelState {
    pub fn init() -> Self {
        let channel = Channel::Oss;
        let app_id = AppId::new("dev", "uncaged", "WarpOss");
        Self {
            channel,
            additional_features: Default::default(),
            config: ChannelConfig {
                app_id,
                logfile_name: "".into(),
                // Uncaged: fail closed. Every Warp endpoint points at an
                // unroutable sentinel so nothing can reach app.warp.dev even via
                // a code path we missed — defense-in-depth behind the per-site
                // gates. autoupdate is already None.
                server_config: WarpServerConfig::uncaged_sentinel(),
                autoupdate_config: None,
                mcp_static_config: None,
            },
            data_app_id: None,
        }
    }

    /// Returns the server used by test-only URL routing so downstream tests can install mocks.
    #[cfg(feature = "test-util")]
    pub fn mock_server() -> parking_lot::MutexGuard<'static, mockito::ServerGuard> {
        lazy_static::initialize(&MOCK_SERVER_URL);
        MOCK_SERVER.lock()
    }

    pub fn new(channel: Channel, mut config: ChannelConfig) -> Self {
        if let Some(app_id) = app_id_from_bundle() {
            config.app_id = app_id;
        }
        Self {
            channel,
            additional_features: Default::default(),
            config,
            data_app_id: None,
        }
    }

    pub fn with_additional_features(mut self, overrides: &[FeatureFlag]) -> Self {
        self.additional_features.extend(overrides);
        self
    }

    pub fn set(state: ChannelState) {
        *CHANNEL_STATE.lock() = state;
    }

    pub fn is_release_bundle() -> bool {
        cfg!(feature = "release_bundle")
    }

    pub fn enable_debug_features() -> bool {
        cfg!(debug_assertions) || matches!(Self::channel(), Channel::Local | Channel::Dev)
    }

    /// Returns the canonical identifier for the application.
    ///
    /// This should not be used for namespacing persisted data - such use cases
    /// should make use of [`Self::data_domain`] instead.
    pub fn app_id() -> AppId {
        CHANNEL_STATE.lock().config.app_id.clone()
    }

    /// Returns the app ID that persisted data is namespaced under.
    ///
    /// This is [`Self::app_id`] unless this run has to keep using the data of a
    /// previous identity, because moving it to the current one was not safe yet
    /// (see `identity_migration` in the app crate).
    pub fn data_app_id() -> AppId {
        let state = CHANNEL_STATE.lock();
        state
            .data_app_id
            .clone()
            .unwrap_or_else(|| state.config.app_id.clone())
    }

    /// Namespaces persisted data under `app_id` for the rest of this process.
    ///
    /// Must be called before anything computes a data path or domain.
    pub fn set_data_app_id(app_id: AppId) {
        CHANNEL_STATE.lock().data_app_id = Some(app_id);
    }

    /// Returns a profile name for isolating user data. This should be used to
    /// sandbox how user data is stored.
    ///
    /// This is a debugging tool for isolating development instances of Warp, and is not
    /// supported in release builds.
    pub fn data_profile() -> Option<String> {
        if cfg!(debug_assertions) {
            std::env::var("WARP_DATA_PROFILE").ok()
        } else {
            None
        }
    }

    /// Returns a value that should be used for namespacing persisted data.
    ///
    /// In release builds, this is identical to the data app ID (normally the
    /// app ID); in debug builds, it optionally includes a suffix derived from
    /// the `WARP_DATA_PROFILE` environment variable.
    pub fn data_domain() -> String {
        match Self::data_profile() {
            Some(profile) => format!("{}-{profile}", Self::data_app_id()),
            None => Self::data_app_id().to_string(),
        }
    }

    /// Returns the data domain if it differs from the app ID (a data profile is
    /// set, or this run keeps a previous identity's data), otherwise None.
    pub fn data_domain_if_not_default() -> Option<String> {
        let data_domain = Self::data_domain();
        (data_domain != Self::app_id().to_string()).then_some(data_domain)
    }

    pub fn additional_features() -> HashSet<FeatureFlag> {
        CHANNEL_STATE
            .lock()
            .additional_features
            .iter()
            .cloned()
            .collect()
    }

    pub fn debug_str() -> String {
        format!("{:?}", *CHANNEL_STATE.lock())
    }

    pub fn logfile_name() -> Cow<'static, str> {
        CHANNEL_STATE.lock().config.logfile_name.clone()
    }

    pub fn releases_base_url() -> Cow<'static, str> {
        CHANNEL_STATE
            .lock()
            .config
            .autoupdate_config
            .as_ref()
            .map(|ac| ac.releases_base_url.clone())
            .unwrap_or_default()
    }

    pub fn server_root_url() -> Cow<'static, str> {
        cfg_if::cfg_if! {
            if #[cfg(feature = "test-util")] {
                Cow::Owned(MOCK_SERVER_URL.clone())
            } else {
                CHANNEL_STATE.lock().config.server_config.server_root_url.clone()
            }
        }
    }

    /// The audience for workload identity tokens. Uncaged has no Oz config, so this is
    /// always the server root URL.
    pub fn workload_audience_url() -> Cow<'static, str> {
        Self::server_root_url()
    }

    // Returns the origin url, with scheme, domain, and ports (if any)
    pub fn server_root_domain() -> Origin {
        Url::parse(&Self::server_root_url())
            .expect("Server root URL should be valid")
            .origin()
    }

    pub fn channel() -> Channel {
        CHANNEL_STATE.lock().channel
    }

    #[cfg(feature = "test-util")]
    pub fn app_version() -> Option<&'static str> {
        let version = APP_VERSION.lock();

        version.or_else(|| option_env!("GIT_RELEASE_TAG"))
    }

    #[cfg(feature = "test-util")]
    pub fn set_app_version(version: Option<&'static str>) {
        *APP_VERSION.lock() = version;
    }

    #[cfg(not(feature = "test-util"))]
    pub fn app_version() -> Option<&'static str> {
        option_env!("GIT_RELEASE_TAG")
    }

    pub fn show_autoupdate_menu_items() -> bool {
        // Uncaged: always on for the Oss channel.
        //
        // This gates every entry point to an applied update -- the "Update and relaunch"
        // menu item, the update banner, and the resource centre row. It reads
        // `autoupdate_config`, which describes *upstream's* update service and is `None`
        // here, so `unwrap_or_default()` returned false and hid all three.
        //
        // The effect was an update that could be found but not applied: the check ran,
        // the release was fetched, the stage reached `UpdateReady` -- and the only sign
        // of it was a dot on the settings icon leading to a menu with no update item.
        // Uncaged does not use upstream's updater, so the absence of its config says
        // nothing about whether Uncaged has an update to offer.
        if matches!(Self::channel(), Channel::Oss) {
            return true;
        }

        CHANNEL_STATE
            .lock()
            .config
            .autoupdate_config
            .as_ref()
            .map(|ac| ac.show_autoupdate_menu_items)
            .unwrap_or_default()
    }

    /// Returns the MCP OAuth provider config matching the given client ID, if any.
    pub fn mcp_oauth_provider_by_client_id(client_id: &str) -> Option<McpOAuthProviderConfig> {
        CHANNEL_STATE
            .lock()
            .config
            .mcp_static_config
            .as_ref()
            .and_then(|c| c.providers.iter().find(|p| p.client_id == client_id))
            .cloned()
    }

    /// Returns the MCP OAuth provider config matching the given issuer URL, if any.
    pub fn mcp_oauth_provider_by_issuer(issuer: &str) -> Option<McpOAuthProviderConfig> {
        CHANNEL_STATE
            .lock()
            .config
            .mcp_static_config
            .as_ref()
            .and_then(|c| c.providers.iter().find(|p| p.issuer == issuer))
            .cloned()
    }

    pub fn url_scheme() -> &'static str {
        match Self::channel() {
            Channel::Stable => "warp",
            Channel::Preview => "warppreview",
            Channel::Dev => "warpdev",
            // Dummy value--integration tests shouldn't support URL schemes.
            Channel::Integration => "warpintegration",
            Channel::Local => "warplocal",
            Channel::Oss => "uncagedoss",
        }
    }
}

fn app_id_from_bundle() -> Option<AppId> {
    // On macOS, attempt to determine the app ID from the containing bundle,
    // falling back to the channel-keyed "default" ID if we cannot retrieve
    // bundle information.
    //
    // We skip this for tests, as the call to `mainBundle` can take 30+ms,
    // which is a significant portion of the total test runtime.
    #[cfg(all(target_os = "macos", not(feature = "test-util")))]
    {
        use objc2_foundation::NSBundle;

        let bundle = NSBundle::mainBundle();
        if let Some(bundle_identifier) = bundle.bundleIdentifier() {
            let app_id = bundle_identifier.to_string();
            if !app_id.is_empty() {
                return Some(
                    AppId::parse(&app_id)
                        .expect("macOS bundle identifier has an unexpected format"),
                );
            }
        }
    }

    None
}
