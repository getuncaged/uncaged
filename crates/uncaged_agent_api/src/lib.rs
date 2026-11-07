//! Rust bindings for the `warp.multi_agent.v1` protobuf package: the agent
//! protocol Agent Mode streams from `uncaged_engine` and stores in SQLite.
//!
//! The bindings in `generated/` are produced by `script/regen_agent_api` from
//! the `.proto` sources in `proto/`. Do not edit them by hand.

#[allow(clippy::all)]
#[rustfmt::skip]
#[path = "generated/warp.multi_agent.v1.rs"]
mod generated;

pub use generated::*;

pub mod field_mask;
