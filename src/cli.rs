//! CLI interface for Apple Mail MCP tools.

pub mod commands;

use clap::Parser;

/// CLI arguments for the Apple Mail tool.
#[derive(Debug, Parser)]
#[command(name = "rusty_apple_mail")]
#[command(about = "Apple Mail read-only tool - MCP server or CLI", long_about = None)]
pub struct Cli {
    /// Mail directory path (default: platform-specific Apple Mail location)
    #[arg(long, env = "APPLE_MAIL_DIR")]
    pub mail_directory: Option<std::path::PathBuf>,

    /// Apple Mail database version (default: V10)
    #[arg(long, env = "APPLE_MAIL_VERSION")]
    pub mail_version: Option<String>,

    /// Account selector(s) - comma-separated account identifiers or email addresses
    #[arg(long, env = "APPLE_MAIL_ACCOUNT")]
    pub account: Option<String>,

    /// Run as MCP server (stdin/stdout protocol)
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// CLI subcommands.
#[derive(Debug, Parser)]
pub enum Command {
    /// List all email accounts
    ListAccounts(ListAccountsArgs),
    /// List all mailboxes
    ListMailboxes(ListMailboxesArgs),
    /// Search messages
    Search(SearchArgs),
    /// Get a specific message by ID
    GetMessage(GetMessageArgs),
    /// Get attachment content
    GetAttachment(GetAttachmentArgs),
}

/// Arguments for list_accounts command.
#[derive(Debug, Parser)]
pub struct ListAccountsArgs {
    /// Include mailboxes grouped by account
    #[arg(long, default_value = "false")]
    pub include_mailboxes: bool,
}

/// Arguments for list_mailboxes command.
#[derive(Debug, Parser)]
pub struct ListMailboxesArgs {
    /// Filter by account identifier
    #[arg(long)]
    pub account: Option<String>,
}

/// Arguments for search_messages command.
#[derive(Debug, Parser)]
pub struct SearchArgs {
    /// Text to search in subject (partial match, case-insensitive)
    #[arg(long)]
    pub subject_query: Option<String>,
    /// Start of date range (YYYY-MM-DD, inclusive)
    #[arg(long)]
    pub date_from: Option<String>,
    /// End of date range (YYYY-MM-DD, inclusive)
    #[arg(long)]
    pub date_to: Option<String>,
    /// Sender email address (exact match)
    #[arg(long)]
    pub sender: Option<String>,
    /// Recipient email address (To/CC exact match)
    #[arg(long)]
    pub participant: Option<String>,
    /// Account identifier
    #[arg(long)]
    pub account: Option<String>,
    /// Mailbox name or fragment
    #[arg(long)]
    pub mailbox: Option<String>,
    /// Maximum number of results (default 20, max 100)
    #[arg(long, default_value = "20")]
    pub limit: u32,
    /// Offset for pagination
    #[arg(long, default_value = "0")]
    pub offset: u32,
    /// Include ~200 character body preview
    #[arg(long, default_value = "false")]
    pub include_body_preview: bool,
}

/// Arguments for get_message command.
#[derive(Debug, Parser)]
pub struct GetMessageArgs {
    /// Message ID (required)
    #[arg(long, required = true)]
    pub message_id: String,
    /// Include message body (default: true)
    #[arg(long, default_value = "true")]
    pub include_body: bool,
    /// Include attachment list (default: true)
    #[arg(long, default_value = "true")]
    pub include_attachments_summary: bool,
    /// Body format: text, html, or both (default: text)
    #[arg(long, default_value = "text")]
    pub body_format: String,
    /// Include To/CC recipients (default: false)
    #[arg(long, default_value = "false")]
    pub include_recipients: bool,
}

/// Arguments for get_attachment command.
#[derive(Debug, Parser)]
pub struct GetAttachmentArgs {
    /// Attachment identifier (format: "{message_id}:{attachment_index}")
    #[arg(long, required = true)]
    pub attachment_id: String,
    /// Parent message ID (needed to locate the attachment file)
    #[arg(long, required = true)]
    pub message_id: String,
}

pub use commands::{get_attachment, get_message, list_accounts, list_mailboxes, search_messages};
