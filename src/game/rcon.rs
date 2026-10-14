//! Remote-console clients the game drivers use to talk to a server they
//! launched. Each protocol is its own module with analogous functions; there
//! is no common trait until something needs to be generic over the transport.

pub mod source;
pub mod web;
