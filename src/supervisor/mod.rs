//! Odin's conmon-style Valheim supervisor: `odin run --instance <name>` is a
//! hidden CLI subcommand that owns one Valheim server process for its entire
//! life and exposes a small Unix-socket RPC surface so `odin serve` (or a
//! one-off CLI invocation) can control it without holding a live
//! `tokio::process::Child` or relying solely on OS-level pid fingerprinting.
//!
//! `server` is the `odin run` side (the conmon role); `client` is the `odin
//! serve` side (the podman role). `protocol` defines the newline-delimited
//! JSON messages exchanged between them.
//!
//! Valheim's lifecycle uses this supervisor today. Rust v1 uses the shared
//! process primitives directly because it has no event parser or readiness
//! contract yet.

pub mod client;
pub mod protocol;
pub mod server;

use std::path::PathBuf;

use crate::paths::Paths;

/// Path to an instance's control socket (request/response: `Ping`, `Stop`).
/// Rooted at `Paths::runtime_dir`, not the instance's own directory — see
/// that method's doc comment for why (Unix socket path length limits).
pub fn control_sock_path(paths: &Paths, instance_name: &str) -> PathBuf {
    paths
        .runtime_dir()
        .join(format!("{instance_name}.control.sock"))
}

/// Path to an instance's events socket (push-only: `LogLine`, `Exited`).
pub fn events_sock_path(paths: &Paths, instance_name: &str) -> PathBuf {
    paths
        .runtime_dir()
        .join(format!("{instance_name}.events.sock"))
}

/// Path to the supervisor's own pidfile, written on startup and removed on
/// clean exit.
pub fn pidfile_path(paths: &Paths, instance_name: &str) -> PathBuf {
    paths.runtime_dir().join(format!("{instance_name}.pid"))
}
