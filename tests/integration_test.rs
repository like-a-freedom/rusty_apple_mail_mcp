//! Integration tests for the Apple Mail MCP server library.

mod support;

use rusty_apple_mail_mcp::server::{MailMcpServer, tools::*};
use support::{make_test_config, make_test_db, seed_emlx_in_account, seed_emlx_in_nested_mailbox};

#[test]
fn tool_definitions_are_all_read_only() {
    let tools = MailMcpServer::tool_definitions();
    assert_eq!(tools.len(), 5);
    assert!(tools.iter().all(|tool| {
        tool.annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            == Some(true)
    }));
}

#[test]
fn list_accounts_returns_distinct_accounts() {
    let conn = make_test_db();
    let response = list_accounts_with_conn(&conn).unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, Some(2));
    assert_eq!(response.accounts[0].account_id, "ews://account-b");
    assert_eq!(response.accounts[1].account_id, "imap://account-a");
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
            account: None,
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
fn search_by_account_returns_only_matching_messages() {
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
            account: Some("ews://account-b".to_string()),
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, 1);
    assert_eq!(response.messages[0].mailbox, "Inbox");
    assert_eq!(response.messages[0].subject, "Budget Planning");
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
            account: None,
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
    assert_eq!(response.total_count, Some(2));
    assert_eq!(response.mailboxes[0].name, "Inbox");
}

#[test]
fn get_message_reads_body_from_nested_mailbox_uuid_data_layout() {
    let conn = make_test_db();
    conn.execute("UPDATE mailboxes SET url = ?1 WHERE ROWID = 2", ["ews://account-b/Inbox/Internal%20services/Confluence"])
        .unwrap();
    let (_temp_dir, config) = make_test_config();
    seed_emlx_in_nested_mailbox(
        &config,
        "account-b",
        &["Inbox", "Internal services", "Confluence"],
        "79665",
        concat!(
            "From: notifications@example.com\n",
            "To: bob@example.com\n",
            "Message-ID: <msg2@mail>\n",
            "Subject: Budget Planning\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Nested mailbox body\n"
        ),
    );

    let response = get_message_with_conn(
        &config,
        &conn,
        GetMessageParams {
            message_id: "2".to_string(),
            include_body: true,
            include_attachments_summary: true,
            body_format: BodyFormat::Text,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    let message = response.message.expect("message payload");
    assert_eq!(message.mailbox, "Confluence");
    assert_eq!(message.body.as_deref(), Some("Nested mailbox body\n"));
}
