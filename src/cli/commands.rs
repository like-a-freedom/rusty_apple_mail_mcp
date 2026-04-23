//! CLI command implementations.

use crate::config::MailConfig;
use crate::error::MailMcpError;
use crate::server::tools::{
    BodyFormat, GetAttachmentParams, GetMessageParams, ListAccountsParams, SearchMessagesParams,
    list_mailboxes as server_list_mailboxes,
};
use crate::server::tools::{
    get_attachment_content as server_get_attachment, get_message as server_get_message,
    list_accounts as server_list_accounts, search_messages as server_search_messages,
};

/// Execute list_accounts command.
pub fn list_accounts(config: &MailConfig, include_mailboxes: bool) -> Result<(), MailMcpError> {
    let params = ListAccountsParams { include_mailboxes };
    let result = server_list_accounts(config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute list_mailboxes command.
pub fn list_mailboxes(
    config: &MailConfig,
    _account_filter: Option<String>,
) -> Result<(), MailMcpError> {
    // For list_mailboxes, we just call without filtering at the CLI level
    // The config already handles account filtering
    let result = server_list_mailboxes(config)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute search_messages command.
pub fn search_messages(config: &MailConfig, args: super::SearchArgs) -> Result<(), MailMcpError> {
    let params = SearchMessagesParams {
        subject_query: args.subject_query,
        date_from: args.date_from,
        date_to: args.date_to,
        sender: args.sender,
        participant: args.participant,
        account: args.account,
        mailbox: args.mailbox,
        limit: args.limit.clamp(1, 100),
        offset: args.offset,
        include_body_preview: args.include_body_preview,
    };
    let result = server_search_messages(config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}

/// Execute get_message command.
pub fn get_message(config: &MailConfig, args: super::GetMessageArgs) -> Result<(), MailMcpError> {
    let body_format = match args.body_format.to_lowercase().as_str() {
        "html" => BodyFormat::Html,
        "both" => BodyFormat::Both,
        _ => BodyFormat::Text,
    };
    let params = GetMessageParams {
        message_id: args.message_id,
        include_body: args.include_body,
        include_attachments_summary: args.include_attachments_summary,
        body_format,
        include_recipients: args.include_recipients,
    };
    let result = server_get_message(config, params)?;
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
    let result = server_get_attachment(config, params)?;
    serde_json::to_writer_pretty(std::io::stdout(), &result)?;
    Ok(())
}
