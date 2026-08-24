use std::io::{self, Write as _};

use anyhow::{Result, bail};

use crate::instance::state::InstalledMod;
use crate::mods::thunderstore::ThunderstorePackage;
use crate::mods::{self, thunderstore};
use crate::paths::Paths;

const MAX_RESULTS: usize = 30;

pub fn run(paths: &Paths, server_name: &str, query: &str, list_only: bool) -> Result<()> {
    let index = thunderstore::fetch_index(paths)?;
    let results = thunderstore::search(&index, query);
    if results.is_empty() {
        println!("no mods found matching '{query}'");
        return Ok(());
    }
    let shown: Vec<&ThunderstorePackage> = results.into_iter().take(MAX_RESULTS).collect();

    let installed = mods::list(paths, server_name)?;

    print_results(&shown, &installed);

    if !list_only {
        prompt_and_install(paths, server_name, &shown)?;
    }

    Ok(())
}

fn print_results(shown: &[&ThunderstorePackage], installed: &[InstalledMod]) {
    println!(
        "{:<4} {:<40} {:<12} {:<10} {:<16} DESCRIPTION",
        "#", "MOD", "VERSION", "DOWNLOADS", "INSTALLED"
    );

    for (i, package) in shown.iter().enumerate() {
        let Some(latest) = package.versions.first() else {
            continue;
        };
        let mod_id = format!("{}-{}", package.owner, package.name);
        let description: String = latest.description.chars().take(50).collect();
        let index = i + 1;

        let status = installed.iter().find(|m| m.mod_id == mod_id).map_or_else(
            || "-".to_string(),
            |m| {
                if m.enabled {
                    format!("yes (v{})", m.version)
                } else {
                    format!("disabled (v{})", m.version)
                }
            },
        );
        println!(
            "{index:<4} {mod_id:<40} {:<12} {:<10} {status:<16} {description}",
            latest.version_number, latest.downloads
        );
    }
}

fn prompt_and_install(
    paths: &Paths,
    server_name: &str,
    shown: &[&ThunderstorePackage],
) -> Result<()> {
    print!("install which # (Enter to skip)? ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(());
    }

    let Ok(choice) = input.parse::<usize>() else {
        bail!("'{input}' is not a number");
    };
    let Some(package) = choice.checked_sub(1).and_then(|i| shown.get(i)) else {
        bail!("no result #{input}; pick a number from the list above");
    };

    let mod_id = format!("{}-{}", package.owner, package.name);
    mods::add(paths, server_name, &mod_id)?;
    println!("installed mod '{mod_id}' for '{server_name}'");
    Ok(())
}
