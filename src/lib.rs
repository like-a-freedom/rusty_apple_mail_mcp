//! Apple Mail read-only MCP server library.

pub mod accounts;
pub mod cli;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod mail;
pub mod runner;
pub mod server;

// Core types
pub use config::MailConfig;
pub use error::MailMcpError;
pub use runner::run;

// Domain types
pub use domain::{
    AttachmentContent, AttachmentMeta, ContentFormat, MessageFull, MessageMeta,
    extract_mailbox_name, timestamp_to_iso,
};

// Account types
pub use accounts::AccountMetadata;

// Convenience type alias
pub type Result<T> = std::result::Result<T, MailMcpError>;
