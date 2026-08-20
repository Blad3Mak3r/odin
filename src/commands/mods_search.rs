use anyhow::Result;

use crate::mods::thunderstore;
use crate::paths::Paths;

pub fn run(paths: &Paths, query: &str) -> Result<()> {
    let index = thunderstore::fetch_index(paths)?;
    let results = thunderstore::search(&index, query);

    if results.is_empty() {
        println!("no mods found matching '{query}'");
        return Ok(());
    }

    println!(
        "{:<40} {:<12} {:<10} DESCRIPTION",
        "MOD", "VERSION", "DOWNLOADS"
    );
    for package in results.iter().take(30) {
        let Some(latest) = package.versions.first() else {
            continue;
        };
        let mod_id = format!("{}-{}", package.owner, package.name);
        let description: String = latest.description.chars().take(50).collect();
        println!(
            "{:<40} {:<12} {:<10} {}",
            mod_id, latest.version_number, latest.downloads, description
        );
    }
    Ok(())
}
