//! CLI adapter for the bundled agent skills.
//!
//! The adapter owns the clap arguments and the rendering. It selects documents
//! from [`crate::skills`] and turns them into markdown or JSON, without
//! knowing where the documents came from.

use clap::Parser;
use serde::Serialize;

use crate::error::MailMcpError;
use crate::skills::{SKILL_DOCUMENT_PATH, Skill, SkillDocument, catalog, find, resolve_reference};

/// Output format of the skills command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// Human- and agent-readable markdown.
    Text,
    /// Machine-readable JSON envelope.
    Json,
}

/// Which documents a `skills get` invocation selects.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Selection {
    /// The skill document only.
    Primary,
    /// The skill document and every reference.
    All,
    /// Only the named references.
    References(Vec<String>),
}

/// Arguments of the `skills` command.
#[derive(Debug, Parser)]
pub struct SkillsArgs {
    /// Print a JSON envelope instead of markdown
    #[arg(long, global = true)]
    pub json: bool,

    /// Action to perform
    #[command(subcommand)]
    pub action: Option<SkillsAction>,
}

/// Actions of the `skills` command.
#[derive(Debug, Parser)]
pub enum SkillsAction {
    /// List the bundled skills
    List(ListSkillsArgs),
    /// Print a skill document
    Get(GetSkillArgs),
}

/// Arguments of `skills list`.
#[derive(Debug, Parser)]
pub struct ListSkillsArgs {}
/// Arguments of `skills get`.
#[derive(Debug, Parser)]
pub struct GetSkillArgs {
    /// Name of the skill
    pub name: String,

    /// Print the skill document and every reference
    #[arg(long, conflicts_with = "reference")]
    pub full: bool,

    /// Print only the named reference; repeatable
    #[arg(long = "ref", value_name = "REF")]
    pub reference: Vec<String>,
}

/// Resolve a selection into the documents to render.
fn select<'a>(
    skill: &'a Skill,
    selection: &Selection,
) -> Result<Vec<&'a SkillDocument>, MailMcpError> {
    match selection {
        Selection::Primary => Ok(vec![&skill.document]),
        Selection::All => {
            let mut documents = vec![&skill.document];
            documents.extend(skill.references.iter());
            Ok(documents)
        }
        Selection::References(queries) => queries
            .iter()
            .map(|query| resolve_reference(skill, query))
            .collect(),
    }
}

/// Render selected documents as markdown or as a JSON envelope.
fn render_documents(
    skill: &Skill,
    documents: &[&SkillDocument],
    format: Format,
    pretty: bool,
) -> Result<String, MailMcpError> {
    match format {
        Format::Text => Ok(render_text(documents)),
        Format::Json => {
            let payload = SkillPayload {
                name: &skill.name,
                description: &skill.description,
                documents: documents
                    .iter()
                    .map(|document| DocumentPayload {
                        path: &document.path,
                        content: &document.content,
                    })
                    .collect(),
            };
            serialize(&payload, pretty)
        }
    }
}

/// Render the catalog of bundled skills.
fn render_catalog(format: Format, pretty: bool) -> Result<String, MailMcpError> {
    let skills = catalog()?;
    match format {
        Format::Text => Ok(skills
            .iter()
            .map(|skill| format!("{}: {}", skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n")),
        Format::Json => {
            let payload = CatalogPayload {
                skills: skills
                    .iter()
                    .map(|skill| CatalogEntry {
                        name: &skill.name,
                        description: &skill.description,
                        references: skill
                            .references
                            .iter()
                            .map(|reference| reference.path.as_str())
                            .collect(),
                    })
                    .collect(),
            };
            serialize(&payload, pretty)
        }
    }
}

/// Render the output of the skills command.
pub fn run(args: &SkillsArgs, pretty: bool) -> Result<String, MailMcpError> {
    let format = if args.json {
        Format::Json
    } else {
        Format::Text
    };
    match &args.action {
        None | Some(SkillsAction::List(_)) => render_catalog(format, pretty),
        Some(SkillsAction::Get(get)) => render_get(get, format, pretty),
    }
}

/// Render the document of one skill.
fn render_get(args: &GetSkillArgs, format: Format, pretty: bool) -> Result<String, MailMcpError> {
    let skill = find(&args.name)?.ok_or_else(|| unknown_skill(&args.name))?;
    let documents = select(skill, &selection_from(args))?;
    render_documents(skill, &documents, format, pretty)
}

/// Derive the selection from the arguments of `skills get`.
///
/// Clap rejects `--full` together with `--ref`, so the two never conflict here.
fn selection_from(args: &GetSkillArgs) -> Selection {
    if args.full {
        Selection::All
    } else if args.reference.is_empty() {
        Selection::Primary
    } else {
        Selection::References(args.reference.clone())
    }
}

/// Build the error for a skill the catalog does not contain.
fn unknown_skill(name: &str) -> MailMcpError {
    let available = catalog().map(skill_names).unwrap_or_default();
    MailMcpError::Validation(format!(
        "Unknown skill '{name}'. Available skills: {available}"
    ))
}

/// Names of the bundled skills, comma separated.
fn skill_names(skills: &[Skill]) -> String {
    skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render documents as markdown.
///
/// The skill document is printed verbatim so its frontmatter stays valid at the
/// top of the output. References are preceded by a marker naming their path.
fn render_text(documents: &[&SkillDocument]) -> String {
    documents
        .iter()
        .map(|document| {
            if document.path == SKILL_DOCUMENT_PATH {
                document.content.clone()
            } else {
                format!(
                    "<!-- reference: {} -->\n\n{}",
                    document.path, document.content
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Serialize a payload as JSON.
fn serialize<T: Serialize>(value: &T, pretty: bool) -> Result<String, MailMcpError> {
    let json = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    Ok(json)
}

/// JSON envelope of a skill and its selected documents.
#[derive(Debug, Serialize)]
struct SkillPayload<'a> {
    name: &'a str,
    description: &'a str,
    documents: Vec<DocumentPayload<'a>>,
}

/// JSON envelope of one document.
#[derive(Debug, Serialize)]
struct DocumentPayload<'a> {
    path: &'a str,
    content: &'a str,
}

/// JSON envelope of the catalog.
#[derive(Debug, Serialize)]
struct CatalogPayload<'a> {
    skills: Vec<CatalogEntry<'a>>,
}

/// JSON envelope of one catalog entry.
#[derive(Debug, Serialize)]
struct CatalogEntry<'a> {
    name: &'a str,
    description: &'a str,
    references: Vec<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::find;

    fn skill() -> &'static Skill {
        find("apple-mail-cli")
            .expect("catalog is parsable")
            .expect("skill present")
    }

    #[test]
    fn select_returns_the_primary_document_only() {
        let documents = select(skill(), &Selection::Primary).expect("selection");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].path, "SKILL.md");
    }

    #[test]
    fn select_returns_the_primary_document_and_every_reference_for_all() {
        let documents = select(skill(), &Selection::All).expect("selection");
        let paths: Vec<&str> = documents
            .iter()
            .map(|document| document.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "SKILL.md",
                "references/cli-recipes.md",
                "references/process-contract.md"
            ]
        );
    }

    #[test]
    fn select_returns_only_the_named_references() {
        let selection = Selection::References(vec!["process-contract".to_string()]);
        let documents = select(skill(), &selection).expect("selection");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].path, "references/process-contract.md");
    }

    #[test]
    fn select_rejects_an_unknown_reference() {
        let selection = Selection::References(vec!["nope".to_string()]);
        let err = select(skill(), &selection).expect_err("unknown reference");
        assert!(err.to_string().contains("references/cli-recipes.md"));
    }

    #[test]
    fn render_text_prints_the_primary_document_verbatim() {
        let documents = select(skill(), &Selection::Primary).expect("selection");
        let text = render_documents(skill(), &documents, Format::Text, false).expect("render");
        assert_eq!(text, skill().document.content);
    }

    #[test]
    fn render_text_marks_each_reference_with_its_path() {
        let documents = select(skill(), &Selection::All).expect("selection");
        let text = render_documents(skill(), &documents, Format::Text, false).expect("render");
        assert!(text.contains("<!-- reference: references/cli-recipes.md -->"));
        assert!(text.contains("<!-- reference: references/process-contract.md -->"));
        assert!(text.starts_with("---"));
        assert!(
            !text.contains("\n\n\n"),
            "documents must be separated by a single blank line"
        );
    }

    #[test]
    fn render_json_carries_the_same_documents_as_the_text_form() {
        let documents = select(skill(), &Selection::All).expect("selection");
        let json = render_documents(skill(), &documents, Format::Json, false).expect("render");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["name"], "apple-mail-cli");
        assert_eq!(value["documents"].as_array().expect("documents").len(), 3);
        assert_eq!(value["documents"][0]["path"], "SKILL.md");
        assert_eq!(value["documents"][1]["path"], "references/cli-recipes.md");
    }

    #[test]
    fn render_catalog_names_every_skill() {
        let text = render_catalog(Format::Text, false).expect("render");
        assert!(text.contains("apple-mail-cli"));
    }

    #[test]
    fn render_catalog_json_lists_every_skill_with_its_references() {
        let json = render_catalog(Format::Json, false).expect("render");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["skills"][0]["name"], "apple-mail-cli");
        assert_eq!(
            value["skills"][0]["references"][0],
            "references/cli-recipes.md"
        );
    }
}
