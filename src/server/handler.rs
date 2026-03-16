//! MCP Server handler implementation with manual tool routing.

use crate::config::MailConfig;
use crate::error::MailMcpError;
use crate::server::tools::{
    GetAttachmentParams, GetMessageParams, SearchMessagesParams,
    get_attachment_content as tool_get_attachment, get_message as tool_get_message,
    list_accounts as tool_list_accounts, list_mailboxes as tool_list_mailboxes,
    search_messages as tool_search_messages,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    model::*,
    model::{ServerInfo, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// MailMcpServer - MCP server for Apple Mail read-only access.
#[derive(Clone)]
pub struct MailMcpServer {
    config: Arc<MailConfig>,
}

impl MailMcpServer {
    /// Create a new MailMcpServer with the given configuration.
    pub fn new(config: MailConfig) -> Result<Self, MailMcpError> {
        config.validate()?;
        Ok(Self {
            config: Arc::new(config),
        })
    }

    /// Convert a JSON Value to Arc<Map<String, Value>> for Tool schema.
    fn value_to_schema(value: Value) -> Arc<Map<String, Value>> {
        match value {
            Value::Object(map) => Arc::new(map),
            _ => Arc::new(Map::new()),
        }
    }

    /// Format an elapsed duration as fractional seconds with millisecond precision.
    fn format_elapsed_seconds(elapsed: Duration) -> String {
        format!("{:.3}", elapsed.as_secs_f64())
    }

    /// Emit a warn-level completion log for a tool invocation.
    fn log_tool_completion(name: &str, elapsed: Duration, outcome: &str) {
        tracing::warn!(
            "tool completed: name={}, outcome={}, elapsed_s={}",
            name,
            outcome,
            Self::format_elapsed_seconds(elapsed)
        );
    }

    /// List all available tools.
    pub fn tool_definitions() -> Vec<Tool> {
        vec![
            Tool::new(
                "search_messages",
                "Find emails in Apple Mail by subject, date range, sender, participant, account, or mailbox — or any combination.\n\n\
                 Use this tool when: the agent needs to find one or more emails matching known criteria.\n\
                 Do NOT use this tool when: the agent already has a message_id — use get_message instead.\n\n\
                 At least one filter argument must be provided: subject_query, date_from, date_to, sender, participant, account, or mailbox.",
                Self::value_to_schema(json!({
                    "type": "object",
                    "properties": {
                        "subject_query": {
                            "type": "string",
                            "description": "Text to search in subject (partial match, case-insensitive)"
                        },
                        "date_from": {
                            "type": "string",
                            "description": "Start of date range (YYYY-MM-DD, inclusive)"
                        },
                        "date_to": {
                            "type": "string",
                            "description": "End of date range (YYYY-MM-DD, inclusive)"
                        },
                        "sender": {
                            "type": "string",
                            "description": "Sender email address (exact match)"
                        },
                        "participant": {
                            "type": "string",
                            "description": "Recipient email address (To/CC, exact match)"
                        },
                        "account": {
                            "type": "string",
                            "description": "Account identifier returned by list_accounts (for example, ews://account-id)"
                        },
                        "mailbox": {
                            "type": "string",
                            "description": "Mailbox name or fragment"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default 20, max 100)",
                            "default": 20
                        },
                        "include_body_preview": {
                            "type": "boolean",
                            "description": "Include ~200 character body preview",
                            "default": false
                        }
                    }
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "list_accounts",
                "List all mail accounts derived from Apple Mail mailbox URLs.\n\n\
                 Use this tool when: the agent needs to choose a single account before calling search_messages.\n\
                 Do NOT use this tool when: searching across all accounts is acceptable.",
                Self::value_to_schema(json!({
                    "type": "object",
                    "properties": {}
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "get_message",
                "Retrieve the full content of an email by its ID: metadata, body text, recipients, and attachment summary.\n\n\
                 Use this tool when: the agent has a message_id (from search results) and needs to read the email.\n\
                 Do NOT use this tool when: the agent needs to find emails — use search_messages first.",
                Self::value_to_schema(json!({
                    "type": "object",
                    "properties": {
                        "message_id": {
                            "type": "string",
                            "description": "Stable message identifier (from search results)"
                        },
                        "include_body": {
                            "type": "boolean",
                            "description": "Include message body (default true)",
                            "default": true
                        },
                        "include_attachments_summary": {
                            "type": "boolean",
                            "description": "Include attachment list (default true)",
                            "default": true
                        },
                        "body_format": {
                            "type": "string",
                            "enum": ["text", "html", "both"],
                            "description": "Body format (default: text)",
                            "default": "text"
                        }
                    },
                    "required": ["message_id"]
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "get_attachment_content",
                "Extract and return the readable content of an email attachment by its ID.\n\n\
                 Use this tool when: the agent needs to read, summarise, or analyse a specific attachment.\n\
                 Do NOT use this tool when: the agent only needs the attachment list — use get_message instead.",
                Self::value_to_schema(json!({
                    "type": "object",
                    "properties": {
                        "attachment_id": {
                            "type": "string",
                            "description": "Attachment identifier (format: \"{message_id}:{attachment_index}\")"
                        },
                        "message_id": {
                            "type": "string",
                            "description": "Parent message identifier (needed to locate the attachment file)"
                        }
                    },
                    "required": ["attachment_id", "message_id"]
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "list_mailboxes",
                "List all mailboxes in Apple Mail with their message counts.\n\n\
                 Use this tool when: the agent needs to discover available mailboxes or verify mailbox names.\n\
                 Do NOT use this tool when: the agent needs to search for specific emails — use search_messages instead.",
                Self::value_to_schema(json!({
                    "type": "object",
                    "properties": {}
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
        ]
    }

    /// Call a tool by name with the given arguments.
    async fn call_tool_by_name(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        match name {
            "search_messages" => {
                let params: SearchMessagesParams = serde_json::from_value(Value::Object(arguments))
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                let response = tool_search_messages(&self.config, params)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::json(response)?]))
            }
            "get_message" => {
                let params: GetMessageParams = serde_json::from_value(Value::Object(arguments))
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                let response = tool_get_message(&self.config, params)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::json(response)?]))
            }
            "get_attachment_content" => {
                let params: GetAttachmentParams = serde_json::from_value(Value::Object(arguments))
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                let response = tool_get_attachment(&self.config, params)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::json(response)?]))
            }
            "list_accounts" => {
                let response = tool_list_accounts(&self.config)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::json(response)?]))
            }
            "list_mailboxes" => {
                let response = tool_list_mailboxes(&self.config)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::json(response)?]))
            }
            _ => Err(McpError::invalid_request("Unknown tool method", None)),
        }
    }
}

impl ServerHandler for MailMcpServer {
    fn get_info(&self) -> ServerInfo {
        let json = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "apple-mail-mcp",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Read-only access to Apple Mail. Use search_messages to find emails, get_message to read one, get_attachment_content to read an attachment, list_mailboxes to see available mailboxes."
        });
        serde_json::from_value(json).expect("valid ServerInfo")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = Self::tool_definitions();
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let name = request.name;
        let args = request.arguments.unwrap_or_default();
        let started = Instant::now();
        let result = self.call_tool_by_name(&name, args).await;
        let outcome = if result.is_ok() { "success" } else { "error" };

        Self::log_tool_completion(&name, started.elapsed(), outcome);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io;
    use std::io::Write;
    use std::sync::Mutex;
    use std::time::Duration;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct SharedWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    struct SharedWriterGuard {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = SharedWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriterGuard {
                buffer: Arc::clone(&self.buffer),
            }
        }
    }

    impl Write for SharedWriterGuard {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer
                .lock()
                .expect("buffer lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn format_elapsed_seconds_formats_fractional_seconds() {
        assert_eq!(
            MailMcpServer::format_elapsed_seconds(Duration::from_millis(1250)),
            "1.250"
        );
    }

    #[test]
    fn log_tool_completion_emits_warn_log_with_elapsed_seconds() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatch = tracing::Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatch, || {
            MailMcpServer::log_tool_completion(
                "search_messages",
                Duration::from_millis(1250),
                "success",
            );
        });

        let output = String::from_utf8(writer.buffer.lock().expect("buffer lock").clone())
            .expect("utf8 log output");
        assert!(
            output
                .contains("tool completed: name=search_messages, outcome=success, elapsed_s=1.250"),
            "unexpected log output: {output}"
        );
    }

    #[test]
    fn value_to_schema_with_object() {
        let value = json!({"type": "object", "properties": {}});
        let result = MailMcpServer::value_to_schema(value);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn value_to_schema_with_non_object() {
        let value = json!("not an object");
        let result = MailMcpServer::value_to_schema(value);
        assert_eq!(result.len(), 0);
    }

    fn create_temp_config() -> (tempfile::TempDir, MailConfig) {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let db_dir = mail_directory.join(&mail_version).join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");

        let config = MailConfig::from_parts_with_accounts(
            mail_directory,
            mail_version,
            None,
            HashMap::new(),
        )
        .expect("config");
        (temp_dir, config)
    }

    #[test]
    fn get_info_returns_server_info() {
        let (_temp_dir, config) = create_temp_config();
        let server = MailMcpServer::new(config).expect("server creation");
        let info = server.get_info();
        // Just check that info is created, protocol_version type is ProtocolVersion enum
        assert!(info.server_info.name.contains("apple-mail"));
    }

    #[test]
    fn list_tools_returns_tool_definitions() {
        let tools = MailMcpServer::tool_definitions();
        assert!(!tools.is_empty());
        // Should have at least search_messages, get_message, get_attachment_content, list_accounts, list_mailboxes
        assert!(tools.len() >= 5);
    }

    #[test]
    fn call_tool_by_name_unknown_tool() {
        let (_temp_dir, config) = create_temp_config();
        let server = MailMcpServer::new(config).expect("server creation");

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { server.call_tool_by_name("unknown_tool", Map::new()).await });

        assert!(result.is_err());
    }

    #[test]
    fn call_tool_by_name_list_accounts() {
        let (_temp_dir, config) = create_temp_config();

        // Create test database with mailboxes using proper SQLite API
        let db_path = config.envelope_db_path();
        // Remove the placeholder file first
        let _ = std::fs::remove_file(&db_path);
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (ROWID INTEGER PRIMARY KEY, mailbox INTEGER, date_sent INTEGER, date_received INTEGER, message_id TEXT, global_message_id INTEGER, subject INTEGER, sender INTEGER);
            INSERT INTO mailboxes VALUES (1, 'imap://test/INBOX');
            INSERT INTO messages VALUES (1, 1, 0, 0, 'msg1', NULL, NULL, NULL);
            "#,
        ).expect("seed db");
        drop(conn);

        let server = MailMcpServer::new(config).expect("server creation");

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { server.call_tool_by_name("list_accounts", Map::new()).await });

        assert!(result.is_ok());
        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
    }

    #[test]
    fn call_tool_by_name_list_mailboxes() {
        let (_temp_dir, config) = create_temp_config();

        // Create test database with mailboxes using proper SQLite API
        let db_path = config.envelope_db_path();
        // Remove the placeholder file first
        let _ = std::fs::remove_file(&db_path);
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (ROWID INTEGER PRIMARY KEY, mailbox INTEGER, date_sent INTEGER, date_received INTEGER, message_id TEXT, global_message_id INTEGER, subject INTEGER, sender INTEGER);
            INSERT INTO mailboxes VALUES (1, 'imap://test/INBOX');
            INSERT INTO messages VALUES (1, 1, 0, 0, 'msg1', NULL, NULL, NULL);
            "#,
        ).expect("seed db");
        drop(conn);

        let server = MailMcpServer::new(config).expect("server creation");

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { server.call_tool_by_name("list_mailboxes", Map::new()).await });

        assert!(result.is_ok());
    }

    #[test]
    fn call_tool_by_name_search_messages_requires_filter() {
        let (_temp_dir, config) = create_temp_config();
        let server = MailMcpServer::new(config).expect("server creation");

        let mut args = Map::new();
        args.insert("limit".to_string(), json!(20));

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { server.call_tool_by_name("search_messages", args).await });

        assert!(result.is_ok());
        let call_result = result.unwrap();
        // Should return error response since no filter provided
        assert!(!call_result.content.is_empty());
    }

    #[test]
    fn tool_definitions_all_read_only() {
        let tools = MailMcpServer::tool_definitions();
        assert_eq!(tools.len(), 5);

        for tool in &tools {
            assert!(
                tool.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true),
                "Tool {} should be read-only",
                tool.name
            );
        }
    }

    #[test]
    fn call_tool_with_invalid_params() {
        let (_temp_dir, config) = create_temp_config();
        let server = MailMcpServer::new(config).expect("server creation");

        // Pass invalid JSON for search_messages params
        let mut args = Map::new();
        args.insert("limit".to_string(), json!("not_a_number"));

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { server.call_tool_by_name("search_messages", args).await });

        assert!(result.is_err());
    }
}
