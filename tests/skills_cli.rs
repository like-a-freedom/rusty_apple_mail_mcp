//! Process-level tests for the `skills` command.

use std::process::Command;

/// Build a command that runs the built binary.
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rusty_apple_mail_mcp"))
}

#[test]
fn skills_subcommand_is_listed_in_help() {
    let output = cli().arg("--help").output().expect("run cli");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("skills"));
    assert!(stdout.contains("List bundled agent skills"));
}

#[test]
fn skills_get_prints_the_skill_document_and_exits_zero() {
    let output = cli()
        .args(["skills", "get", "apple-mail-cli"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("---"));
    assert!(stdout.contains("# Apple Mail CLI SOP"));
}

#[test]
fn skills_list_without_an_action_lists_the_catalog() {
    let output = cli().arg("skills").output().expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("apple-mail-cli"));
}

#[test]
fn skills_get_unknown_skill_exits_two_with_a_json_error() {
    let output = cli()
        .args(["skills", "get", "nope"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().expect("error line");
    let envelope: serde_json::Value = serde_json::from_str(last).expect("json error");
    assert_eq!(envelope["error_kind"], "usage_error");
    assert!(
        envelope["message"]
            .as_str()
            .expect("message")
            .contains("apple-mail-cli")
    );
}

#[test]
fn skills_get_json_returns_the_document_envelope() {
    let output = cli()
        .args(["skills", "get", "apple-mail-cli", "--json"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    assert_eq!(value["name"], "apple-mail-cli");
    assert_eq!(value["documents"][0]["path"], "SKILL.md");
    assert!(
        value["documents"][0]["content"]
            .as_str()
            .expect("content")
            .starts_with("---")
    );
}

#[test]
fn skills_get_json_accepts_pretty_after_the_action() {
    let output = cli()
        .args(["skills", "get", "apple-mail-cli", "--json", "--pretty"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\n  \"name\""),
        "expected indented json: {stdout}"
    );
}

#[test]
fn skills_get_ref_prints_only_that_reference() {
    let output = cli()
        .args([
            "skills",
            "get",
            "apple-mail-cli",
            "--ref",
            "process-contract",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<!-- reference: references/process-contract.md -->"));
    assert!(!stdout.contains("references/cli-recipes.md"));
    assert!(!stdout.starts_with("---"));
}

#[test]
fn skills_get_help_lists_the_flags() {
    let output = cli()
        .args(["skills", "get", "--help"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--full"));
    assert!(stdout.contains("--ref"));
}

#[test]
fn skills_get_unknown_reference_exits_two_with_a_json_error() {
    let output = cli()
        .args(["skills", "get", "apple-mail-cli", "--ref", "nope"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().expect("error line");
    let envelope: serde_json::Value = serde_json::from_str(last).expect("json error");
    assert_eq!(envelope["error_kind"], "usage_error");
    let message = envelope["message"].as_str().expect("message");
    assert!(message.contains("references/cli-recipes.md"));
    let guidance = envelope["guidance"].as_str().expect("guidance");
    assert!(
        !guidance.contains("bug"),
        "a wrong reference must not be reported as a bug: {guidance}"
    );
}

#[test]
fn skills_works_without_a_configured_mail_directory() {
    let output = cli()
        .args([
            "--mail-directory",
            "/nonexistent/mail/dir",
            "skills",
            "list",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    assert!(!output.stdout.is_empty());
}

#[test]
fn skills_get_with_full_and_reference_is_rejected() {
    let output = cli()
        .args([
            "skills",
            "get",
            "apple-mail-cli",
            "--full",
            "--ref",
            "cli-recipes",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--full") && stderr.contains("--ref"),
        "expected a conflict error, got: {stderr}"
    );
}

#[test]
fn skills_json_flag_is_accepted_before_and_after_the_action() {
    for args in [
        vec!["skills", "--json", "list"],
        vec!["skills", "list", "--json"],
    ] {
        let output = cli().args(&args).output().expect("run cli");
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
        assert_eq!(value["skills"][0]["name"], "apple-mail-cli");
    }
}
