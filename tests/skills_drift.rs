//! Guards that keep the served skill content aligned with the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use clap::error::ErrorKind;
use rusty_apple_mail_mcp::cli::Cli;

/// Name of the binary the skill documents show.
const BINARY: &str = "rusty_apple_mail_mcp";

/// Directory holding the skill documents in the repository.
fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/apple-mail-cli")
}

/// Every skill document in the repository.
fn skill_files() -> Vec<PathBuf> {
    vec![
        skill_dir().join("SKILL.md"),
        skill_dir().join("references/cli-recipes.md"),
        skill_dir().join("references/process-contract.md"),
    ]
}

#[test]
fn served_document_matches_the_repository_file_byte_for_byte() {
    let output = Command::new(env!("CARGO_BIN_EXE_rusty_apple_mail_mcp"))
        .args(["skills", "get", "apple-mail-cli"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let on_disk = std::fs::read(skill_dir().join("SKILL.md")).expect("read skill file");
    assert_eq!(output.stdout, on_disk);
}

#[test]
fn every_documented_command_parses_through_clap() {
    for file in skill_files() {
        let text = std::fs::read_to_string(&file).expect("read skill file");
        for command in documented_commands(&text) {
            assert_parses(&command, &file);
        }
    }
}

/// Assert that a documented command is accepted by the CLI parser.
///
/// A `--help` command is accepted too: clap reports it by printing the help
/// text, which is a successful outcome rather than a parse failure.
fn assert_parses(command: &[String], file: &Path) {
    match Cli::try_parse_from(command) {
        Ok(_) => {}
        Err(err) => assert!(
            matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ),
            "{file:?} documents an unparsable command: {command:?} ({err})"
        ),
    }
}

#[test]
fn skill_documents_stay_within_the_byte_budget() {
    let skill = skill_dir().join("SKILL.md");
    assert!(std::fs::metadata(&skill).expect("stat").len() <= 8192);
    for reference in [
        "references/cli-recipes.md",
        "references/process-contract.md",
    ] {
        let path = skill_dir().join(reference);
        assert!(std::fs::metadata(&path).expect("stat").len() <= 4096);
    }
}

/// Extract one command per line of every fenced code block.
fn documented_commands(text: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut inside_block = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            inside_block = !inside_block;
            continue;
        }
        if !inside_block || trimmed.is_empty() {
            continue;
        }
        if let Some(command) = command_arguments(trimmed) {
            commands.push(command);
        }
    }
    commands
}

/// Turn a shell line into the arguments of a CLI invocation.
fn command_arguments(line: &str) -> Option<Vec<String>> {
    let mut tokens: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    while tokens
        .first()
        .is_some_and(|token| is_environment_assignment(token))
    {
        tokens.remove(0);
    }
    let index = tokens.iter().position(|token| token == BINARY)?;
    let mut arguments = vec![BINARY.to_string()];
    arguments.extend(tokens[index + 1..].iter().cloned());
    Some(arguments)
}

/// Tell whether a token assigns an environment variable.
fn is_environment_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}
