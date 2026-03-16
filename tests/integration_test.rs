//! Integration tests for the Apple Mail MCP server library.

mod support;

use rusty_apple_mail_mcp::server::{MailMcpServer, tools::*};
use support::{
    make_restricted_test_config, make_test_config, make_test_db, seed_emlx_in_account,
    seed_emlx_in_nested_mailbox,
};

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
    let (_temp_dir, config) = make_test_config();
    let response = list_accounts_with_conn(&config, &conn).unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, Some(2));
    assert_eq!(response.accounts[0].account_id, "ews://account-b");
    assert_eq!(response.accounts[1].account_id, "imap://account-a");
    assert_eq!(
        response.accounts[0].account_name.as_deref(),
        Some("Kaspersky")
    );
    assert_eq!(
        response.accounts[0].email.as_deref(),
        Some("anton.solovey@kaspersky.com")
    );
}

#[test]
fn list_accounts_hides_disallowed_accounts() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_restricted_test_config("ews://account-b");

    let response = list_accounts_with_conn(&config, &conn).unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, Some(1));
    assert_eq!(response.accounts[0].account_id, "ews://account-b");
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
fn search_messages_defaults_to_allowed_accounts_only() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_restricted_test_config("ews://account-b");

    let response = search_messages_with_conn(
        &config,
        &conn,
        SearchMessagesParams {
            subject_query: Some("Budget".to_string()),
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
    assert_eq!(response.messages[0].id, "2");
}

#[test]
fn search_messages_rejects_disallowed_explicit_account_filter() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_restricted_test_config("ews://account-b");

    let response = search_messages_with_conn(
        &config,
        &conn,
        SearchMessagesParams {
            subject_query: Some("Q3".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: Some("imap://account-a".to_string()),
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        },
    )
    .unwrap();

    assert_eq!(response.status, "error");
    assert!(
        response
            .guidance
            .expect("guidance")
            .contains("excluded by APPLE_MAIL_ACCOUNT")
    );
}

#[test]
fn search_messages_reports_attachment_count_without_body_preview() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();
    conn.execute(
        "INSERT INTO attachments (ROWID, message, attachment_id, name) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![1_i64, 1_i64, "att-1", "notes.txt"],
    )
    .unwrap();
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
    assert_eq!(response.messages[0].attachment_count, 1);
    assert_eq!(response.messages[0].body_preview, None);
}

#[test]
fn search_messages_prefers_database_summary_and_attachment_metadata() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();
    conn.execute_batch(
        r#"
        INSERT INTO attachments (ROWID, message, attachment_id, name) VALUES
            (1, 1, 'att-1', 'notes.txt'),
            (2, 1, 'att-2', 'agenda.txt');
        "#,
    )
    .unwrap();

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
            include_body_preview: true,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, 1);
    assert_eq!(response.messages[0].attachment_count, 2);
    assert_eq!(
        response.messages[0].body_preview.as_deref(),
        Some("DB-backed preview for Q3 review")
    );
}

#[test]
fn search_messages_falls_back_to_emlx_preview_when_database_summary_is_missing() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();
    seed_emlx_in_account(
        &config,
        "account-b",
        "Inbox",
        2,
        concat!(
            "From: notifications@example.com\n",
            "To: bob@example.com\n",
            "Subject: Budget Planning\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Fallback preview from emlx body\n"
        ),
    );

    let response = search_messages_with_conn(
        &config,
        &conn,
        SearchMessagesParams {
            subject_query: Some("Budget".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            include_body_preview: true,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, 1);
    assert_eq!(
        response.messages[0].body_preview.as_deref(),
        Some("Fallback preview from emlx body")
    );
}

#[test]
fn search_messages_counts_attachments_from_database_for_nested_mailbox_results() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_test_config();
    conn.execute(
        "INSERT INTO attachments (ROWID, message, attachment_id, name) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![1_i64, 2_i64, "agenda-1", "agenda.txt"],
    )
    .unwrap();
    seed_emlx_in_nested_mailbox(
        &config,
        "account-b",
        &["Inbox"],
        "79665",
        concat!(
            "From: notifications@example.com\n",
            "To: bob@example.com\n",
            "Message-ID: <msg2@mail>\n",
            "Subject: Budget Planning\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Nested mailbox body\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"agenda.txt\"\n",
            "Content-Disposition: attachment; filename=\"agenda.txt\"\n",
            "\n",
            "Agenda attachment\n",
            "--boundary--\n"
        ),
    );

    let response = search_messages_with_conn(
        &config,
        &conn,
        SearchMessagesParams {
            subject_query: Some("Budget".to_string()),
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
    assert_eq!(response.messages[0].attachment_count, 1);
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
    let (_temp_dir, config) = make_test_config();
    let response = list_mailboxes_with_conn(&config, &conn).unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, Some(2));
    assert_eq!(response.mailboxes[0].name, "Inbox");
}

#[test]
fn list_mailboxes_hides_disallowed_accounts() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_restricted_test_config("ews://account-b");

    let response = list_mailboxes_with_conn(&config, &conn).unwrap();

    assert_eq!(response.status, "success");
    assert_eq!(response.total_count, Some(1));
    assert_eq!(
        response.mailboxes[0].account_id.as_deref(),
        Some("ews://account-b")
    );
}

#[test]
fn get_message_blocks_disallowed_accounts() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_restricted_test_config("ews://account-b");

    let response = get_message_with_conn(
        &config,
        &conn,
        GetMessageParams {
            message_id: "1".to_string(),
            include_body: false,
            include_attachments_summary: false,
            body_format: BodyFormat::Text,
        },
    )
    .unwrap();

    assert_eq!(response.status, "error");
    assert!(
        response
            .guidance
            .expect("guidance")
            .contains("excluded by APPLE_MAIL_ACCOUNT")
    );
}

#[test]
fn get_attachment_blocks_disallowed_accounts() {
    let conn = make_test_db();
    let (_temp_dir, config) = make_restricted_test_config("ews://account-b");
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

    assert_eq!(response.status, "error");
    assert!(
        response
            .guidance
            .expect("guidance")
            .contains("excluded by APPLE_MAIL_ACCOUNT")
    );
}

#[test]
fn get_message_reads_body_from_nested_mailbox_uuid_data_layout() {
    let conn = make_test_db();
    conn.execute(
        "UPDATE mailboxes SET url = ?1 WHERE ROWID = 2",
        ["ews://account-b/Inbox/Internal%20services/Confluence"],
    )
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

#[test]
fn get_message_prefers_message_id_match_over_wrong_numeric_hint() {
    let conn = make_test_db();
    conn.execute(
        "UPDATE messages SET global_message_id = ?1 WHERE ROWID = 2",
        [99974],
    )
    .unwrap();
    let (_temp_dir, config) = make_test_config();

    let messages_dir = config
        .mail_directory
        .join(&config.mail_version)
        .join("account-b")
        .join("Inbox.mbox")
        .join("Messages");
    std::fs::create_dir_all(&messages_dir).unwrap();

    let wrong_emlx = messages_dir.join("99974.emlx");
    let wrong_raw_email = concat!(
        "Message-ID: <wrong@mail>\n",
        "Subject: Wrong body\n",
        "Content-Type: text/plain; charset=utf-8\n",
        "\n",
        "Wrong numeric hint body\n"
    );
    std::fs::write(
        &wrong_emlx,
        format!("{}\n{}", wrong_raw_email.len(), wrong_raw_email),
    )
    .unwrap();

    let correct_emlx = messages_dir.join("79665.emlx");
    let correct_raw_email = concat!(
        "Message-ID: <msg2@mail>\n",
        "Subject: Budget Planning\n",
        "Content-Type: text/plain; charset=utf-8\n",
        "\n",
        "Correct Message-ID body\n"
    );
    std::fs::write(
        &correct_emlx,
        format!("{}\n{}", correct_raw_email.len(), correct_raw_email),
    )
    .unwrap();

    let response = get_message_with_conn(
        &config,
        &conn,
        GetMessageParams {
            message_id: "2".to_string(),
            include_body: true,
            include_attachments_summary: false,
            body_format: BodyFormat::Text,
        },
    )
    .unwrap();

    assert_eq!(response.status, "success");
    let message = response.message.expect("message payload");
    assert_eq!(message.body.as_deref(), Some("Correct Message-ID body\n"));
}

#[test]
fn get_message_uses_cache_for_repeated_calls() {
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

    let params = GetMessageParams {
        message_id: "1".to_string(),
        include_body: true,
        include_attachments_summary: true,
        body_format: BodyFormat::Text,
    };

    let first = get_message_with_conn(&config, &conn, params.clone()).unwrap();
    assert_eq!(first.status, "success");
    let first_message = first.message.expect("first message");
    assert!(
        first_message
            .body
            .expect("body")
            .contains("Hello from emlx body")
    );

    let second = get_message_with_conn(&config, &conn, params).unwrap();
    assert_eq!(second.status, "success");
    let second_message = second.message.expect("second message");
    assert!(
        second_message
            .body
            .expect("body")
            .contains("Hello from emlx body")
    );
}
