use anyhow::Result;

fn main() -> Result<()> {
    let target_family = std::env::var("CARGO_CFG_TARGET_FAMILY")?;

    if target_family != "wasm" {
        println!("cargo:rustc-cfg=feature=\"local_fs\"");
    }

    // `ChannelState::app_version()` bakes in GIT_RELEASE_TAG via `option_env!`.
    // Without this, cargo won't recompile warp_core when the tag changes (e.g.
    // across releases on a cached target dir), so the About page shows a stale
    // version. Force a rebuild whenever the release tag changes.
    println!("cargo:rerun-if-env-changed=GIT_RELEASE_TAG");

    Ok(())
}
