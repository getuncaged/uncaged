//! The app-wide HTTP client.

use std::sync::Arc;

use warp_core::context_flag::ContextFlag;
use warpui::{Entity, ModelContext, SingletonEntity};

use super::network_logging::NetworkLogModel;
use crate::brand;

/// A singleton that owns the HTTP client the app shares for plain HTTP requests (downloads,
/// update fetches, and the like). When the network log console is enabled, its traffic shows up
/// in the network log. Requests carry a generic `Uncaged/<version>` User-Agent and nothing that
/// identifies the install.
pub struct SharedHttpClient {
    client: Arc<http_client::Client>,
}

impl SharedHttpClient {
    /// Builds the shared client. [`NetworkLogModel`] must already be registered.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut client = http_client::Client::with_user_agent(brand::HTTP_USER_AGENT);
        if ContextFlag::NetworkLogConsole.is_enabled() {
            NetworkLogModel::handle(ctx).update(ctx, |model, model_ctx| {
                model.install_on_clients([&mut client], model_ctx);
            });
        }
        Self {
            client: Arc::new(client),
        }
    }

    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self {
            client: Arc::new(http_client::Client::new_for_test()),
        }
    }

    /// Returns the shared client.
    pub fn get(&self) -> Arc<http_client::Client> {
        self.client.clone()
    }
}

impl Entity for SharedHttpClient {
    type Event = ();
}

impl SingletonEntity for SharedHttpClient {}
