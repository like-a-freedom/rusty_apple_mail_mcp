//! MCP tool implementations and schemas.

mod get_attachment;
mod get_message;
mod list_accounts;
mod message_lookup;
mod search_messages;

use schemars::JsonSchema;
use serde::Serialize;

/// Explicit completed outcome for every non-error response.
/// Errors are native rmcp errors, not successful response bodies.
/// See ADR-0008.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseOutcome {
    /// Requested data is complete for this call.
    Success,
    /// Data is valid but continuation or a declared extraction limitation remains.
    Partial,
    /// A search/discovery request was processed but its corpus was empty.
    NotFound,
}

pub use get_attachment::{
    GetAttachmentParams, GetAttachmentResponse, get_attachment_content,
    get_attachment_content_with_conn,
};
pub use get_message::{GetMessageParams, GetMessageResponse, get_message, get_message_with_conn};
pub use list_accounts::{
    ListAccountsParams, ListAccountsResponse, list_accounts, list_accounts_with_conn,
};
pub use search_messages::{
    SearchMessagesParams, SearchMessagesResponse, search_messages, search_messages_async,
};
