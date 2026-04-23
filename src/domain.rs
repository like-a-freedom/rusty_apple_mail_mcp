//! Domain types for email messages and attachments.

mod attachment;
mod message;

pub use attachment::{AttachmentContent, AttachmentMeta, ContentFormat};
pub use message::{MessageFull, MessageMeta, extract_mailbox_name, timestamp_to_iso};
