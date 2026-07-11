pub fn run() -> anyhow::Result<()> {
    gecko_core::diagnostics::init();
    tracing::info!(tracy = cfg!(feature = "tracy"), "initializing...");

    Ok(())
}
