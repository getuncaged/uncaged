use rquickjs::{Ctx, Function, Object};

use super::plugin::PluginHandle;

/// Returns a JS object representing the Warp Plugin API exposed to external JavaScript plugins.
///
/// Currently, the API contains a single "completions" namespace for registering command
/// signatures.
pub fn warp(
    #[allow(unused_variables)] plugin: PluginHandle,
    ctx: Ctx<'_>,
) -> rquickjs::Result<Object<'_>> {
    let api = Object::new(ctx)?;
    Ok(api)
}

/// Returns a JS object to be used as a the `console` global, implementing `console.log()` and
/// `console.err()`.
pub fn console(ctx: Ctx<'_>) -> rquickjs::Result<Object<'_>> {
    let console = Object::new(ctx)?;
    console.set(
        "log",
        Function::new(ctx, |message: String| {
            log::info!("{message}");
        }),
    )?;
    console.set(
        "err",
        Function::new(ctx, |message: String| {
            log::error!("{message}");
        }),
    )?;
    Ok(console)
}
