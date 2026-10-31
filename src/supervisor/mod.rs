//! The RPC between `odin serve` and the `odin run` runtime that owns one game
//! server process inside each container.
//!
//! `client` is the `odin serve` side; the runtime side lives in
//! `podman::runtime`. `protocol` defines the newline-delimited JSON messages
//! exchanged between them over the runtime's control socket.

pub mod client;
pub mod protocol;

use std::path::PathBuf;

use crate::game::GameId;
use crate::paths::Paths;

/// Path to an instance's control socket (request/response: `Ping`, `Stop`).
/// Rooted at `Paths::runtime_dir`, not the instance's own directory — see
/// that method's doc comment for why (Unix socket path length limits).
pub fn control_sock_path(paths: &Paths, game: GameId, instance_name: &str) -> PathBuf {
    paths
        .runtime_dir()
        .join(format!("{game}-{instance_name}.control.sock"))
}
