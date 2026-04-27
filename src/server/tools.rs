//! MCP tool implementations and schemas.

mod get_attachment;
mod get_message;
mod list_accounts;
mod list_mailboxes;
mod search_messages;

use schemars::JsonSchema;
use serde::Serialize;

/// Response status enum. Wrapped in Option — None means success (skipped in JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Error,
    NotFound,
    Partial,
}

pub use get_attachment::{
    GetAttachmentParams, GetAttachmentResponse, get_attachment_content,
    get_attachment_content_with_conn,
};
pub use get_message::{
    BodyFormat, GetMessageParams, GetMessageResponse, get_message, get_message_with_conn,
};
pub use list_accounts::{
    ListAccountsParams, ListAccountsResponse, list_accounts, list_accounts_with_conn,
};
pub use list_mailboxes::{ListMailboxesResponse, list_mailboxes, list_mailboxes_with_conn};
pub use search_messages::{
    SearchMessagesParams, SearchMessagesResponse, search_messages, search_messages_with_conn,
};
