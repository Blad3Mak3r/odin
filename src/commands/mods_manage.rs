use anyhow::{Context, Result};
use dialoguer::MultiSelect;

use crate::db::Db;
use crate::instance::state::InstalledMod;
use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let installed = mods::list(paths, db, server_name)?;
    if installed.is_empty() {
        println!("no mods installed on '{server_name}'");
        return Ok(());
    }

    let items: Vec<String> = installed
        .iter()
        .map(|m| format!("{} (v{})", m.mod_id, m.version))
        .collect();
    let defaults: Vec<bool> = installed.iter().map(|m| m.enabled).collect();

    let selection = MultiSelect::new()
        .with_prompt(format!(
            "toggle mods for '{server_name}' (space: toggle, enter: confirm, esc: cancel)"
        ))
        .items(&items)
        .defaults(&defaults)
        .interact_opt()
        .context("failed to read interactive selection (is this running in a real terminal?)")?;

    let Some(selected_indices) = selection else {
        println!("no changes made");
        return Ok(());
    };

    let changes = diff_enabled_states(&installed, &selected_indices);
    if changes.is_empty() {
        println!("no changes made");
        return Ok(());
    }

    for (mod_id, enabled) in changes {
        mods::set_enabled(paths, db, server_name, &mod_id, enabled)?;
        let verb = if enabled { "enabled" } else { "disabled" };
        println!("{verb} '{mod_id}'");
    }

    Ok(())
}

/// Diffs the original installed-mod list against a `MultiSelect`'s checked
/// indices, returning `(mod_id, new_enabled)` only for mods whose state
/// actually changed. Pure / no I/O — unit-testable without a real terminal,
/// mirroring how `thunderstore::relevance_rank` was split out of `search`.
fn diff_enabled_states(
    installed: &[InstalledMod],
    selected_indices: &[usize],
) -> Vec<(String, bool)> {
    installed
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let now_enabled = selected_indices.contains(&i);
            (now_enabled != m.enabled).then(|| (m.mod_id.clone(), now_enabled))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn installed_mod(mod_id: &str, enabled: bool) -> InstalledMod {
        InstalledMod {
            mod_id: mod_id.to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled,
        }
    }

    #[test]
    fn diff_enabled_states_is_empty_when_selection_matches_current_state() {
        let installed = vec![
            installed_mod("owner-a", true),
            installed_mod("owner-b", false),
        ];
        let selected_indices = vec![0]; // only "owner-a" (already enabled) checked
        assert!(diff_enabled_states(&installed, &selected_indices).is_empty());
    }

    #[test]
    fn diff_enabled_states_detects_enable_and_disable() {
        let installed = vec![
            installed_mod("owner-a", true),
            installed_mod("owner-b", false),
        ];
        let selected_indices = vec![1]; // "owner-a" unchecked (disable), "owner-b" checked (enable)
        assert_eq!(
            diff_enabled_states(&installed, &selected_indices),
            vec![
                ("owner-a".to_string(), false),
                ("owner-b".to_string(), true),
            ]
        );
    }

    #[test]
    fn diff_enabled_states_disables_all_when_selection_is_empty() {
        let installed = vec![
            installed_mod("owner-a", true),
            installed_mod("owner-b", true),
        ];
        assert_eq!(
            diff_enabled_states(&installed, &[]),
            vec![
                ("owner-a".to_string(), false),
                ("owner-b".to_string(), false),
            ]
        );
    }
}
