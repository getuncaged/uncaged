//! Rust bindings for the `warp.multi_agent.v1` protobuf package: the agent
//! protocol Agent Mode streams from `uncaged_engine` and stores in SQLite.
//!
//! The bindings in `generated/` are produced by `script/regen_agent_api` from
//! the `.proto` sources in `proto/`. Do not edit them by hand.

use std::sync::LazyLock;

use prost_reflect::DescriptorPool;

#[allow(clippy::all)]
#[rustfmt::skip]
#[path = "generated/warp.multi_agent.v1.rs"]
mod generated;

pub use generated::*;

static FILE_DESCRIPTOR_SET: &[u8] = include_bytes!("generated/file_descriptor_set.bin");

static DESCRIPTOR_POOL: LazyLock<DescriptorPool> = LazyLock::new(|| {
    DescriptorPool::decode(FILE_DESCRIPTOR_SET).expect("Failed to load file descriptor set")
});

pub static MESSAGE_DESCRIPTOR: LazyLock<prost_reflect::MessageDescriptor> = LazyLock::new(|| {
    get_descriptor_pool()
        .get_message_by_name("warp.multi_agent.v1.Message")
        .expect("Proto definition exists.")
});

/// Returns the descriptor pool for the generated types.
///
/// This enables clients to use reflection APIs on the types defined
/// in this crate.
pub fn get_descriptor_pool() -> &'static DescriptorPool {
    &DESCRIPTOR_POOL
}
