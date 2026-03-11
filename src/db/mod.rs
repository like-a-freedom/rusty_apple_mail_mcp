//! Database access layer for Apple Mail's Envelope Index SQLite database.

mod connection;
mod queries;

pub use connection::open_readonly;
pub use queries::{
    COREDATA_EPOCH_OFFSET, MessageRow, address_exists, count_messages_in_mailbox,
    detect_epoch_offset_seconds, get_message_by_id, get_recipients, list_mailboxes,
    search_messages,
};
