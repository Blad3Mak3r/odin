use std::net::SocketAddr;

use anyhow::{Context, Result};

use crate::paths::Paths;

/// The only place a tokio runtime is spun up — everything else in Odin
/// stays synchronous. Scoped narrowly to this one subcommand so the rest of
/// the CLI doesn't pay for an async runtime it never uses.
pub fn run(paths: &Paths, bind: &str, port: u16) -> Result<()> {
    let addr: SocketAddr = format!("{bind}:{port}")
        .parse()
        .with_context(|| format!("invalid bind address '{bind}:{port}'"))?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?
        .block_on(crate::web::serve(paths.clone(), addr))
}
