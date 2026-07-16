//! CLI command implementations.

use crate::config::MailConfig;
use crate::db::SqliteMailRepository;
use crate::error::MailMcpError;
use crate::mail::FilesystemAttachmentStore;
use crate::server::tools::{
    GetAttachmentParams, GetMessageParams, ListAccountsParams, SearchMessagesParams,
    list_mailboxes as server_list_mailboxes,
};
use crate::server::tools::{
    get_attachment_content as server_get_attachment, get_message as server_get_message,
    list_accounts as server_list_accounts, search_messages as server_search_messages,
};

fn open_repo_store(
    config: &MailConfig,
) -> Result<(SqliteMailRepository, FilesystemAttachmentStore), MailMcpError> {
    let db_path = config.envelope_db_path();
    let repo = SqliteMailRepository::new(&db_path)?;
    let store = FilesystemAttachmentStore::new(&config.mail_directory);
    Ok((repo, store))
}

/// Execute list_accounts command.
pub fn list_accounts(config: &MailConfig, include_mailboxes: bool) -> Result<(), MailMcpError> {
    let params = ListAccountsParams { include_mailboxes };
    let (repo, store) = open_repo_store(config)?;
    let result = server_list_accounts(&repo, &store, config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute list_mailboxes command.
pub fn list_mailboxes(config: &MailConfig) -> Result<(), MailMcpError> {
    let (repo, _store) = open_repo_store(config)?;
    let result = server_list_mailboxes(&repo, config)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute search_messages command.
pub fn search_messages(config: &MailConfig, args: super::SearchArgs) -> Result<(), MailMcpError> {
    let has_any_filter = args.subject_query.is_some()
        || args.date_from.is_some()
        || args.date_to.is_some()
        || args.sender.is_some()
        || args.participant.is_some()
        || args.account.is_some()
        || args.mailbox.is_some();

    if !has_any_filter {
        return Err(MailMcpError::Validation(
            "At least one filter must be provided: --subject-query, --date-from, --date-to, \
             --sender, --participant, --account, or --mailbox."
                .to_string(),
        ));
    }

    if args.limit > 100 {
        return Err(MailMcpError::Validation(format!(
            "limit must be between 1 and 100, got {}",
            args.limit
        )));
    }

    let params = SearchMessagesParams {
        subject_query: args.subject_query,
        date_from: args.date_from,
        date_to: args.date_to,
        sender: args.sender,
        participant: args.participant,
        account: args.account,
        mailbox: args.mailbox,
        limit: args.limit,
        offset: args.offset,
        include_body_preview: args.include_body_preview,
    };
    let result = server_search_messages(config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute get_message command.
pub fn get_message(config: &MailConfig, args: super::GetMessageArgs) -> Result<(), MailMcpError> {
    let params = GetMessageParams {
        message_id: args.message_id,
        include_body: args.include_body,
        include_attachments_summary: args.include_attachments_summary,
        include_recipients: args.include_recipients,
    };
    let (repo, store) = open_repo_store(config)?;
    let result = server_get_message(&repo, &store, config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute get_attachment command.
pub fn get_attachment(
    config: &MailConfig,
    args: super::GetAttachmentArgs,
) -> Result<(), MailMcpError> {
    let params = GetAttachmentParams {
        attachment_id: args.attachment_id,
        message_id: args.message_id,
    };
    let (repo, store) = open_repo_store(config)?;
    let result = server_get_attachment(&repo, &store, config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

#[cfg(test)]
mod cli_validation_tests {
    use super::*;
    use tempfile::TempDir;

    fn dummy_config() -> (TempDir, MailConfig) {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let db_dir = mail_directory.join("V10").join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");
        let config = MailConfig::new(
            mail_directory,
            "V10".to_string(),
            None,
            std::collections::HashMap::new(),
        )
        .expect("valid dummy config");
        (temp_dir, config)
    }

    #[test]
    fn search_rejects_no_filters() {
        let (_temp, config) = dummy_config();
        let args = super::super::SearchArgs {
            subject_query: None,
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            offset: 0,
            include_body_preview: false,
        };
        let err = search_messages(&config, args).unwrap_err();
        assert!(err.to_string().contains("At least one filter"));
    }

    #[test]
    fn search_rejects_limit_over_100() {
        let (_temp, config) = dummy_config();
        let args = super::super::SearchArgs {
            subject_query: Some("test".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 101,
            offset: 0,
            include_body_preview: false,
        };
        let err = search_messages(&config, args).unwrap_err();
        assert!(err.to_string().contains("limit must be between 1 and 100"));
    }
}
