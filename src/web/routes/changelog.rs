use axum::Json;
use serde::Serialize;

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

#[derive(Debug, PartialEq, Serialize)]
pub struct ChangelogSection {
    pub title: String,
    pub changes: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ChangelogRelease {
    pub version: String,
    pub date: Option<String>,
    pub sections: Vec<ChangelogSection>,
}

pub async fn get_changelog() -> Json<Vec<ChangelogRelease>> {
    Json(parse_changelog(CHANGELOG))
}

fn parse_changelog(input: &str) -> Vec<ChangelogRelease> {
    let mut releases = Vec::new();
    let mut release: Option<ChangelogRelease> = None;
    let mut section: Option<ChangelogSection> = None;

    for line in input.lines() {
        if let Some(header) = line.strip_prefix("## [") {
            push_section(&mut release, section.take());
            push_release(&mut releases, release.take());

            let Some((version, suffix)) = header.split_once(']') else {
                continue;
            };
            release = Some(ChangelogRelease {
                version: version.to_owned(),
                date: suffix.strip_prefix(" - ").map(str::to_owned),
                sections: Vec::new(),
            });
            continue;
        }

        if let Some(title) = line.strip_prefix("### ") {
            push_section(&mut release, section.take());
            section = Some(ChangelogSection {
                title: title.to_owned(),
                changes: Vec::new(),
            });
            continue;
        }

        if let Some(change) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))
            && release.is_some()
        {
            section
                .get_or_insert_with(|| ChangelogSection {
                    title: "Changes".to_owned(),
                    changes: Vec::new(),
                })
                .changes
                .push(change.to_owned());
        }
    }

    push_section(&mut release, section);
    push_release(&mut releases, release);
    releases
}

fn push_section(release: &mut Option<ChangelogRelease>, section: Option<ChangelogSection>) {
    if let (Some(release), Some(section)) = (release, section)
        && !section.changes.is_empty()
    {
        release.sections.push(section);
    }
}

fn push_release(releases: &mut Vec<ChangelogRelease>, release: Option<ChangelogRelease>) {
    if let Some(release) = release
        && !release.sections.is_empty()
    {
        releases.push(release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_sections_and_skips_empty_unreleased_section() {
        let releases = parse_changelog(
            "# Changelog\n\n## [Unreleased]\n\n## [1.2.0] - 2026-08-30\n\n### Added\n\n- First change\n* Second change\n\n## [1.1.0] - 2026-08-20\n\n- Older change\n",
        );

        assert_eq!(
            releases,
            vec![
                ChangelogRelease {
                    version: "1.2.0".to_owned(),
                    date: Some("2026-08-30".to_owned()),
                    sections: vec![ChangelogSection {
                        title: "Added".to_owned(),
                        changes: vec!["First change".to_owned(), "Second change".to_owned()],
                    }],
                },
                ChangelogRelease {
                    version: "1.1.0".to_owned(),
                    date: Some("2026-08-20".to_owned()),
                    sections: vec![ChangelogSection {
                        title: "Changes".to_owned(),
                        changes: vec!["Older change".to_owned()],
                    }],
                },
            ]
        );
    }
}
