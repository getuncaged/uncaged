use chrono::{DateTime, Utc};
use cloud_objects::time::ServerTimestamp;
use serde::{Deserialize, Serialize};
use uncaged_ui::{Entity, SingletonEntity};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum RequestLimitRefreshDuration {
    Weekly,
    Monthly,
    EveryTwoWeeks,
}

/// The current rate limit info for the user.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RequestLimitInfo {
    pub limit: usize,
    pub num_requests_used_since_refresh: usize,
    pub next_refresh_time: ServerTimestamp,
    pub is_unlimited: bool,
    pub request_limit_refresh_duration: RequestLimitRefreshDuration,
    pub is_unlimited_voice: bool,
    #[serde(default)]
    pub voice_request_limit: usize,
    #[serde(default)]
    pub voice_requests_used_since_last_refresh: usize,
    #[serde(default)]
    pub is_unlimited_codebase_indices: bool,
    #[serde(default)]
    pub max_codebase_indices: usize,
    #[serde(default)]
    pub max_files_per_repo: usize,
    #[serde(default)]
    pub embedding_generation_batch_size: usize,
}

fn default_voice_requests_limit() -> usize {
    10000
}

impl Default for RequestLimitInfo {
    /// This is the default rate limit for the free tier imposed by the server as of 02/10/25.
    fn default() -> Self {
        Self {
            limit: 150,
            num_requests_used_since_refresh: 0,
            next_refresh_time: ServerTimestamp::new(Utc::now() + chrono::Duration::days(30)),
            is_unlimited: false,
            request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
            is_unlimited_voice: false,
            voice_request_limit: default_voice_requests_limit(),
            voice_requests_used_since_last_refresh: 0,
            is_unlimited_codebase_indices: false,
            max_codebase_indices: 3,
            max_files_per_repo: 5000,
            embedding_generation_batch_size: 100,
        }
    }
}

#[cfg(test)]
impl RequestLimitInfo {
    pub fn new_for_test(limit: usize, num_requests_used_since_refresh: usize) -> Self {
        Self {
            limit,
            num_requests_used_since_refresh,
            ..Self::default()
        }
    }
}

/// Largest repository Uncaged's local index will take on.
const UNCAGED_MAX_FILES_PER_REPO: usize = 30_000;
/// Chunks indexed per step; keyword indexing is cheap, so steps can be big.
const UNCAGED_INDEX_BATCH_SIZE: usize = 500;

pub struct CodebaseContextUsageLimit {
    pub max_files_per_repo: usize,
    pub max_indices_allowed: Option<usize>,
    pub embedding_generation_batch_size: usize,
}

/// Uncaged has no Warp plan: the request limit is Warp's free-tier default and
/// is never refreshed from a server.
pub struct AIRequestUsageModel {
    request_limit_info: RequestLimitInfo,
}

impl Entity for AIRequestUsageModel {
    type Event = ();
}

impl AIRequestUsageModel {
    pub fn new() -> Self {
        Self {
            request_limit_info: RequestLimitInfo::default(),
        }
    }

    /// Returns the number of remaining requests the user has based on their latest rate limit info.
    /// If the current time is past the next refresh time, then the number of remaining reqs is the limit.
    fn requests_remaining(&self) -> usize {
        if self.next_refresh_time() <= Utc::now() || self.is_unlimited() {
            self.request_limit_info.limit
        } else {
            self.request_limit_info
                .limit
                .saturating_sub(self.request_limit_info.num_requests_used_since_refresh)
        }
    }

    /// Returns `true` if the user has at least one request remaining before hitting the AI request
    /// limit.
    ///
    /// WARNING: This method doesn't account for add-on credits. Consider if you want
    /// [`Self::has_any_ai_remaining`] instead.
    pub fn has_requests_remaining(&self) -> bool {
        self.requests_remaining() > 0
    }

    /// Returns `true` if the user has plan requests remaining or a local engine
    /// powers AI. Use this method as the starting point for AI availability checking.
    pub fn has_any_ai_remaining(&self) -> bool {
        self.has_requests_remaining()
            // Uncaged: a local engine means unlimited local AI, no Warp credits.
            || crate::uncaged::engine_active()
    }

    pub fn request_limit(&self) -> usize {
        self.request_limit_info.limit
    }

    /// Returns the number of indices the user's tier allows them to create and the number of files
    /// the user's tier allows them to index. If the user is allowed unlimited indices, then the
    /// max_indices_allowed is None.
    /// Uncaged indexes on this machine, so Warp's plan limits (3 folders,
    /// 5,000 files) don't apply. The file cap only keeps memory in check.
    pub fn codebase_context_limits(&self) -> CodebaseContextUsageLimit {
        CodebaseContextUsageLimit {
            max_files_per_repo: UNCAGED_MAX_FILES_PER_REPO,
            max_indices_allowed: None,
            embedding_generation_batch_size: UNCAGED_INDEX_BATCH_SIZE,
        }
    }

    pub fn next_refresh_time(&self) -> DateTime<Utc> {
        self.request_limit_info.next_refresh_time.utc()
    }

    pub fn is_unlimited(&self) -> bool {
        self.request_limit_info.is_unlimited
    }
}

impl SingletonEntity for AIRequestUsageModel {}

#[cfg(test)]
#[path = "request_usage_model_tests.rs"]
mod tests;
