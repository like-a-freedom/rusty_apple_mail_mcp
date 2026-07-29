//! MCP server handler and tool routing.

pub mod content_delivery;
mod handler;
pub mod tools;

pub use handler::MailMcpServer;
