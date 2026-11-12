//! Bundled agent skill documents served by the CLI.
//!
//! The documents in `skills/` are the single source of truth. They are embedded
//! at compile time, so what an agent reads always matches the version of the
//! binary it is calling.

use std::sync::LazyLock;

use crate::error::MailMcpError;

/// The skill document, served as the entry point of the skill.
const SKILL_DOCUMENT: &str = include_str!("../skills/apple-mail-cli/SKILL.md");

/// Reference with exact CLI command forms and shell examples.
const CLI_RECIPES: &str = include_str!("../skills/apple-mail-cli/references/cli-recipes.md");

/// Reference with stream, exit-code, and error-handling rules.
const PROCESS_CONTRACT: &str =
    include_str!("../skills/apple-mail-cli/references/process-contract.md");

/// Path of the skill document inside the skill.
pub const SKILL_DOCUMENT_PATH: &str = "SKILL.md";

/// Path of the CLI recipes reference inside the skill.
const CLI_RECIPES_PATH: &str = "references/cli-recipes.md";

/// Path of the process contract reference inside the skill.
const PROCESS_CONTRACT_PATH: &str = "references/process-contract.md";

/// One document belonging to a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDocument {
    /// Path of the document inside the skill, for example `references/cli-recipes.md`.
    pub path: String,
    /// Full document content, byte-identical to the file in the repository.
    pub content: String,
}

/// A bundled agent skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Skill name, taken from the document frontmatter.
    pub name: String,
    /// One-line description, taken from the document frontmatter.
    pub description: String,
    /// The skill document itself.
    pub document: SkillDocument,
    /// Additional documents, loaded on demand.
    pub references: Vec<SkillDocument>,
}

/// Metadata parsed from a skill document's YAML frontmatter.
#[derive(Debug, serde::Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
}

/// The bundled skill catalog.
///
/// The defect message is kept as a string so the static stays `Send + Sync`;
/// [`catalog`] turns it back into a domain error.
static CATALOG: LazyLock<Result<Vec<Skill>, String>> = LazyLock::new(build_catalog);

/// Every bundled skill.
///
/// Fails when a bundled document carries no parsable frontmatter, which is a
/// packaging defect in the binary rather than a request the caller can fix.
pub fn catalog() -> Result<&'static [Skill], MailMcpError> {
    CATALOG
        .as_ref()
        .map_err(|detail| packaging_defect(detail))
        .map(Vec::as_slice)
}

/// Find a skill by its name.
pub fn find(name: &str) -> Result<Option<&'static Skill>, MailMcpError> {
    Ok(catalog()?.iter().find(|skill| skill.name == name))
}

/// Resolve a reference of a skill by path, file name, or stem.
///
/// Matching order is exact path, then file name, then stem. A query that
/// matches more than one reference at the same tier is ambiguous and is
/// reported with its candidates rather than resolved arbitrarily.
pub fn resolve_reference<'a>(
    skill: &'a Skill,
    query: &str,
) -> Result<&'a SkillDocument, MailMcpError> {
    let mut candidates = matching(skill, |document| document.path == query);
    if candidates.is_empty() {
        candidates = matching(skill, |document| file_name(&document.path) == query);
    }
    if candidates.is_empty() {
        candidates = matching(skill, |document| file_stem(&document.path) == query);
    }

    match candidates.len() {
        0 => Err(unknown_reference(skill, query)),
        1 => Ok(candidates[0]),
        _ => Err(ambiguous_reference(skill, query, &candidates)),
    }
}

/// Collect the references of a skill that satisfy a matcher.
fn matching(skill: &Skill, matcher: impl Fn(&SkillDocument) -> bool) -> Vec<&SkillDocument> {
    skill
        .references
        .iter()
        .filter(|document| matcher(document))
        .collect()
}

/// Build the catalog from the embedded documents.
fn build_catalog() -> Result<Vec<Skill>, String> {
    let frontmatter = parse_frontmatter(SKILL_DOCUMENT).map_err(|err| err.to_string())?;
    Ok(vec![Skill {
        name: frontmatter.name,
        description: frontmatter.description,
        document: SkillDocument {
            path: SKILL_DOCUMENT_PATH.to_string(),
            content: SKILL_DOCUMENT.to_string(),
        },
        references: vec![
            SkillDocument {
                path: CLI_RECIPES_PATH.to_string(),
                content: CLI_RECIPES.to_string(),
            },
            SkillDocument {
                path: PROCESS_CONTRACT_PATH.to_string(),
                content: PROCESS_CONTRACT.to_string(),
            },
        ],
    }])
}

/// Parse the YAML frontmatter of a skill document.
fn parse_frontmatter(content: &str) -> Result<Frontmatter, MailMcpError> {
    let block = frontmatter_block(content)
        .ok_or_else(|| packaging_defect("bundled skill document has no YAML frontmatter"))?;
    serde_yaml::from_str(block).map_err(|err| {
        packaging_defect(&format!("bundled skill frontmatter is not parsable: {err}"))
    })
}

/// Extract the raw frontmatter block of a document.
fn frontmatter_block(content: &str) -> Option<&str> {
    let body = content.strip_prefix("---")?;
    let (block, _) = body.split_once("\n---")?;
    Some(block)
}

/// Build the error for a defect in the bundled content.
///
/// The error is carried as a JSON error because that is the only existing
/// variant that reports the internal category and the guidance to report a
/// bug. The defect is in the binary, not in the caller's request.
fn packaging_defect(detail: &str) -> MailMcpError {
    MailMcpError::Json(serde_json::Error::io(std::io::Error::other(format!(
        "bundled skill content is broken: {detail}"
    ))))
}

/// Build the error for a reference that the skill does not contain.
fn unknown_reference(skill: &Skill, query: &str) -> MailMcpError {
    let available = skill
        .references
        .iter()
        .map(|reference| reference.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    MailMcpError::Validation(format!(
        "Unknown reference '{query}' in skill '{}'. Available references: {available}",
        skill.name
    ))
}

/// Build the error for a reference query that matches more than one document.
fn ambiguous_reference(skill: &Skill, query: &str, matches: &[&SkillDocument]) -> MailMcpError {
    let candidates = matches
        .iter()
        .map(|reference| reference.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    MailMcpError::Validation(format!(
        "Ambiguous reference '{query}' in skill '{}'. Matching references: {candidates}",
        skill.name
    ))
}

/// File name of a document path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Stem of a document path, without its directory and extension.
fn file_stem(path: &str) -> &str {
    file_name(path)
        .rsplit_once('.')
        .map_or(file_name(path), |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill() -> &'static Skill {
        find("apple-mail-cli")
            .expect("catalog is parsable")
            .expect("skill present")
    }

    #[test]
    fn catalog_exposes_the_bundled_skill_with_frontmatter_metadata() {
        let skills = catalog().expect("catalog is parsable");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "apple-mail-cli");
        assert!(skills[0].description.contains("AI agent"));
    }

    #[test]
    fn skill_document_is_byte_identical_to_the_repository_file() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/skills/apple-mail-cli/SKILL.md"
        );
        let on_disk = std::fs::read_to_string(path).expect("read skill file");
        assert_eq!(skill().document.content, on_disk);
    }

    #[test]
    fn find_returns_none_for_an_unknown_name() {
        assert!(find("nope").expect("catalog is parsable").is_none());
    }

    #[test]
    fn resolve_reference_matches_by_path_file_name_and_stem() {
        for query in ["references/cli-recipes.md", "cli-recipes.md", "cli-recipes"] {
            let resolved = resolve_reference(skill(), query).expect("reference resolves");
            assert_eq!(resolved.path, "references/cli-recipes.md");
        }
    }

    #[test]
    fn resolve_reference_rejects_an_unknown_reference() {
        let err = resolve_reference(skill(), "nope").expect_err("unknown reference");
        assert!(matches!(err, MailMcpError::Validation(_)));
        assert!(err.to_string().contains("references/cli-recipes.md"));
    }

    #[test]
    fn resolve_reference_rejects_an_ambiguous_reference() {
        let skill = Skill {
            name: "test".to_string(),
            description: "test".to_string(),
            document: SkillDocument {
                path: "SKILL.md".to_string(),
                content: "document".to_string(),
            },
            references: vec![
                SkillDocument {
                    path: "references/a/notes.md".to_string(),
                    content: "a".to_string(),
                },
                SkillDocument {
                    path: "references/b/notes.md".to_string(),
                    content: "b".to_string(),
                },
            ],
        };
        let err = resolve_reference(&skill, "notes.md").expect_err("ambiguous reference");
        assert!(err.to_string().contains("Ambiguous"));
        assert!(err.to_string().contains("references/a/notes.md"));
        assert!(err.to_string().contains("references/b/notes.md"));
    }

    #[test]
    fn parse_frontmatter_rejects_a_document_without_frontmatter() {
        let err = parse_frontmatter("# no frontmatter\n").expect_err("frontmatter missing");
        assert!(err.to_string().contains("no YAML frontmatter"));
    }

    #[test]
    fn parse_frontmatter_rejects_unparsable_frontmatter() {
        let err = parse_frontmatter("---\nname: [unclosed\n---\n").expect_err("frontmatter broken");
        assert!(err.to_string().contains("not parsable"));
    }
}
