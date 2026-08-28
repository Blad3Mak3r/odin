use std::sync::LazyLock;

/// Shared blocking HTTP client — one connection pool / TLS session cache
/// reused by every outbound request instead of each call site paying for a
/// fresh TCP+TLS handshake. Per-call needs (timeouts, User-Agent) go on the
/// request via `.timeout()`/`.header()`, not this client, since call sites
/// need different values.
pub static CLIENT: LazyLock<reqwest::blocking::Client> =
    LazyLock::new(reqwest::blocking::Client::new);
