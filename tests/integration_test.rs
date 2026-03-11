//! Integration tests for the Apple Mail MCP server library.

mod support;

use rusty_apple_mail_mcp::server::{MailMcpServer, tools::*};
use support::{make_test_config, make_test_db, seed_emlx};

#[test]
fn tool_definitions_are_all_read_only() {
    let tools = MailMcpServer::tool_definitions();
    assert_eq!(tools.len(), 4);
    assert!(tools.iter().all(|tool| {
        tool.annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            == Some(true)
    }));
}

#[test]
fn search_by_subject_returns_matching_messages() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();

    let response = search_messages_with_conn(
        &config,
        &conn,
        SearchMessagesParams {
            subject_query: Some("Q3".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, 1);
    assert_eq!(response.messages[0].subject, "Q3 Review");
}

#[test]
fn search_with_no_filters_returns_validation_error() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();

    let response = search_messages_with_conn(
        &config,
        &conn,
        SearchMessagesParams {
            subject_query: None,
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        },
    )
    .unwrap();

    assert_eq!(response.status, "error");
    assert!(response.guidance.unwrap().contains("At least one filter"));
}

#[test]
fn get_message_returns_body_and_attachment_summary() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();
    seed_emlx(
        &config,
        "INBOX",
        1,
        concat!(
            "From: alice@example.com\n",
            "To: bob@example.com\n",
            "Subject: Q3 Review\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from emlx body\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"notes.txt\"\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\n",
            "\n",
            "Attached text\n",
            "--boundary--\n"
        ),
    );

    let response = get_message_with_conn(
        &config,
        &conn,
        GetMessageParams {
            message_id: "1".to_string(),
            include_body: true,
            include_attachments_summary: true,
            body_format: BodyFormat::Text,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    let message = response.message.expect("message payload");
    assert_eq!(message.subject, "Q3 Review");
    assert!(message.body.expect("body").contains("Hello from emlx body"));
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].filename, "notes.txt");
}

#[test]
fn get_attachment_content_returns_text_for_text_attachment() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();
    seed_emlx(
        &config,
        "INBOX",
        1,
        concat!(
            "From: alice@example.com\n",
            "To: bob@example.com\n",
            "Subject: Q3 Review\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from emlx body\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"notes.txt\"\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\n",
            "\n",
            "Attached text\n",
            "--boundary--\n"
        ),
    );

    let response = get_attachment_content_with_conn(
        &config,
        &conn,
        GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    let attachment = response.attachment.expect("attachment payload");
    assert_eq!(attachment.mime_type, "text/plain");
    assert_eq!(attachment.content.expect("content"), "Attached text");
}

#[test]
fn list_mailboxes_returns_all_mailboxes() {
    let conn = make_test_db();
    let response = list_mailboxes_with_conn(&conn).unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, Some(1));
    assert_eq!(response.mailboxes[0].name, "INBOX");
}
