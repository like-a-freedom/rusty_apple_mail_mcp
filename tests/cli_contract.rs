//! Process-level tests for the CLI contract outside the skills command.
//!
//! These cover the data commands, so a change to the dispatch order in
//! `runner.rs` cannot silently break them.

mod support;

use std::process::Command;

use support::{make_test_config, seed_emlx_in_account};
use tempfile::TempDir;

/// Build a command that runs the built binary.
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rusty_apple_mail_mcp"))
}

/// Seed a message so the search command has something to find.
///
/// The temporary directory stays alive as long as the returned value does.
fn seed_mail() -> (TempDir, String, String) {
    let (temp_dir, config) = make_test_config();
    seed_emlx_in_account(
        &config,
        "account-a",
        "INBOX",
        1,
        concat!(
            "From: alice@example.com\n",
            "To: bob@example.com\n",
            "Subject: Q3 Review\n",
            "MIME-Version: 1.0\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from the CLI contract test\n",
        ),
    );
    let mail_directory = config.mail_directory.display().to_string();
    let mail_version = config.mail_version.clone();
    (temp_dir, mail_directory, mail_version)
}

#[test]
fn cli_reports_the_package_version() {
    let output = cli().arg("--version").output().expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "expected the package version in: {stdout}"
    );
}

#[test]
fn list_accounts_prints_json_on_stdout_and_exits_zero() {
    let (_temp_dir, mail_directory, mail_version) = seed_mail();
    let output = cli()
        .args([
            "--mail-directory",
            &mail_directory,
            "--mail-version",
            &mail_version,
            "list-accounts",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    assert_eq!(value["outcome"], "success");
    assert!(
        value["accounts"].as_array().expect("accounts").len() >= 2,
        "expected the seeded accounts: {stdout}"
    );
}

#[test]
fn search_prints_json_on_stdout_and_exits_zero() {
    let (_temp_dir, mail_directory, mail_version) = seed_mail();
    let output = cli()
        .args([
            "--mail-directory",
            &mail_directory,
            "--mail-version",
            &mail_version,
            "search",
            "--subject-query",
            "Q3",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    assert_eq!(value["outcome"], "success");
    assert_eq!(value["messages"].as_array().expect("messages").len(), 1);
}

#[test]
fn search_without_matches_reports_not_found_with_guidance() {
    let (_temp_dir, mail_directory, mail_version) = seed_mail();
    let output = cli()
        .args([
            "--mail-directory",
            &mail_directory,
            "--mail-version",
            &mail_version,
            "search",
            "--subject-query",
            "no such subject",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    assert_eq!(value["outcome"], "not_found");
    let guidance = value["guidance"].as_str().expect("guidance");
    assert!(
        !guidance.is_empty(),
        "a not-found outcome must guide the caller"
    );
}

#[test]
fn list_accounts_without_a_mail_directory_exits_five() {
    let output = cli()
        .args(["--mail-directory", "/nonexistent/mail/dir", "list-accounts"])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().expect("error line");
    let envelope: serde_json::Value = serde_json::from_str(last).expect("json error");
    assert_eq!(envelope["error_kind"], "database_not_found");
}

#[test]
fn search_without_a_filter_exits_two_with_the_error_envelope() {
    let (_temp_dir, mail_directory, mail_version) = seed_mail();
    let output = cli()
        .args([
            "--mail-directory",
            &mail_directory,
            "--mail-version",
            &mail_version,
            "search",
        ])
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().expect("error line");
    let envelope: serde_json::Value = serde_json::from_str(last).expect("json error");
    assert_eq!(envelope["error_kind"], "missing_filter");
}
