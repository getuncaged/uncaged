use tracing::subscriber;

/// Configures the global tracing subscriber to not care about any spans or
/// events.
///
/// This is done so that we prevent the `tracing` crate from writing out log
/// lines for spans and trace events.
pub fn init() -> anyhow::Result<()> {
    subscriber::set_global_default(subscriber::NoSubscriber::new())?;
    Ok(())
}
