use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LogServiceRequest {
    pub level: log::Level,
    pub target: String,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LogServiceResponse {
    pub success: bool,
}

/// A generic service for relaying log messages over IPC.
pub struct LogService {}

impl ipc::Service for LogService {
    type Request = LogServiceRequest;
    type Response = LogServiceResponse;

    /// The ID this service had before the crate rename; kept so it stays stable on the wire.
    fn id() -> &'static str {
        "warp::plugin::service::logging::LogService"
    }
}
