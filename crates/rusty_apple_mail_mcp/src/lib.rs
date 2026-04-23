//! Apple Mail read-only MCP server library.
pub mod accounts;
pub mod cli;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod mail;
pub mod server;

pub use config::MailConfig;
pub use error::MailMcpError;
