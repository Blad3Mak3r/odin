//! Dispatches mod-related behavior by where a `mod_id` came from. The source
//! is encoded as a prefix on the id itself rather than a separate DB column:
//! Thunderstore ids stay bare `<namespace>-<name>` (so existing installs and
//! on-disk `mods_dir()/<mod_id>` roots keep working unchanged), while
//! Nexus and user-uploaded mods get a `nexus:`/`local:` prefix. Colons are
//! valid in Linux filenames, so the prefixed string works directly as a
//! directory name and symlink name with no further encoding.

use uuid::Uuid;

pub const NEXUS_PREFIX: &str = "nexus:";
pub const LOCAL_PREFIX: &str = "local:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSource {
    Thunderstore,
    Nexus,
    Local,
}

/// Determines a mod's source from its `mod_id`. Anything without a
/// recognized prefix is assumed to be a (pre-existing) Thunderstore id.
pub fn mod_source(mod_id: &str) -> ModSource {
    if mod_id.starts_with(NEXUS_PREFIX) {
        ModSource::Nexus
    } else if mod_id.starts_with(LOCAL_PREFIX) {
        ModSource::Local
    } else {
        ModSource::Thunderstore
    }
}

/// Lowercases `name`, replaces runs of non-alphanumeric characters with a
/// single `-`, and trims leading/trailing `-`. Falls back to `"mod"` if
/// nothing alphanumeric survives, so a slug is never empty.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "mod".to_string()
    } else {
        slug
    }
}

/// Builds a `local:` mod id for a freshly uploaded mod: a slug of the
/// user-supplied name plus a short random suffix, so two uploads with the
/// same name don't collide in the global store.
pub fn make_local_mod_id(name: &str) -> String {
    let short_id = Uuid::new_v4().simple().to_string()[..8].to_string();
    format!("{LOCAL_PREFIX}{}-{short_id}", slugify(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thunderstore_ids_have_no_prefix() {
        assert_eq!(
            mod_source("denikson-BepInExPack_Valheim"),
            ModSource::Thunderstore
        );
    }

    #[test]
    fn nexus_prefix_is_detected() {
        assert_eq!(mod_source("nexus:1234"), ModSource::Nexus);
    }

    #[test]
    fn local_prefix_is_detected() {
        assert_eq!(mod_source("local:my-mod-ab12cd34"), ModSource::Local);
    }

    #[test]
    fn slugify_collapses_punctuation_and_lowercases() {
        assert_eq!(slugify("My Cool Mod!!"), "my-cool-mod");
        assert_eq!(slugify("  leading/trailing  "), "leading-trailing");
    }

    #[test]
    fn slugify_falls_back_when_nothing_alphanumeric_survives() {
        assert_eq!(slugify("!!!"), "mod");
        assert_eq!(slugify(""), "mod");
    }

    #[test]
    fn make_local_mod_id_is_unique_across_calls() {
        let a = make_local_mod_id("My Mod");
        let b = make_local_mod_id("My Mod");
        assert_ne!(a, b);
        assert!(a.starts_with("local:my-mod-"));
        assert_eq!(mod_source(&a), ModSource::Local);
    }
}
