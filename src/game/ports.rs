//! Port-block allocation shared by the statically compiled game drivers.

use anyhow::{Context, Result};

use super::{GameId, driver};

/// Returns every consecutive port claimed by an instance starting at
/// `base_port`. Drivers whose ports cease to be consecutive can reserve their
/// explicit ports alongside this block when loading existing configuration.
pub fn block(game: GameId, base_port: u16) -> Result<Vec<u16>> {
    let count = driver(game).port_requirements().count;
    (0..count)
        .map(|offset| {
            base_port
                .checked_add(offset)
                .context("no available game port block remains")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drivers_declare_their_required_port_blocks() {
        assert_eq!(
            block(GameId::Valheim, 2456).unwrap(),
            vec![2456, 2457, 2458]
        );
        assert_eq!(block(GameId::Rust, 28015).unwrap(), vec![28015, 28016]);
    }
}
