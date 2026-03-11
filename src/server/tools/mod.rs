//! MCP tool implementations and schemas.

mod get_attachment;
mod get_message;
mod list_accounts;
mod list_mailboxes;
mod search_messages;

pub use get_attachment::{
    GetAttachmentParams, GetAttachmentResponse, get_attachment_content,
    get_attachment_content_with_conn,
};
pub use get_message::{
    BodyFormat, GetMessageParams, GetMessageResponse, get_message, get_message_with_conn,
};
pub use list_accounts::{ListAccountsResponse, list_accounts, list_accounts_with_conn};
pub use list_mailboxes::{ListMailboxesResponse, list_mailboxes, list_mailboxes_with_conn};
pub use search_messages::{
    SearchMessagesParams, SearchMessagesResponse, search_messages, search_messages_with_conn,
};
