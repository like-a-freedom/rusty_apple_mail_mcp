//! Database access layer for Apple Mail's Envelope Index `SQLite` database.

mod accounts;
mod connection;
mod epoch;
mod mailboxes;
mod messages;
mod repository;
mod sqlite_repo;

pub use accounts::{AccountRow, list_accounts, mailbox_account_id};
pub use connection::open_readonly;
pub use epoch::COREDATA_EPOCH_OFFSET;
pub use epoch::detect_epoch_offset_seconds;
pub use mailboxes::{count_messages_in_mailbox, list_mailboxes};
pub use messages::{
    MessageRow, address_exists, get_message_by_id, get_recipients, search_messages, tokenize,
};
#[cfg(test)]
pub use repository::FakeMailRepository;
pub use repository::{MailRepository, MessageMetadata, SearchParams};
pub use sqlite_repo::SqliteMailRepository;
