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
        let args = request.arguments.unwrap_or_default();
        self.call_tool_by_name(&request.name, args).await
    }
}
