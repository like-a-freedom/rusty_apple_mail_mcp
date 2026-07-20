//! Repository trait and parameter types for data access.

use crate::db::accounts::AccountRow;
use crate::db::messages::MessageRow;
use crate::error::MailMcpError;
use std::any::Any;
use std::collections::HashMap;

/// Search parameters for message queries.
#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub subject_query: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub sender: Option<String>,
    pub participant: Option<String>,
    pub account: Option<String>,
    pub allowed_accounts: Option<Vec<String>>,
    pub mailbox: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

/// Summary and attachment count metadata for search results.
#[derive(Debug, Clone, Default)]
pub struct MessageMetadata {
    pub summary: Option<String>,
    pub attachment_count: u32,
}

/// Repository trait for Apple Mail data access.
///
/// This trait defines the seam between tools (MCP handlers) and data storage.
/// Implementations can use SQLite, in-memory fixtures, or other backends.
pub trait MailRepository: Send + Sync {
    /// Search messages with optional filters.
    fn search_messages(&self, params: SearchParams) -> Result<Vec<MessageRow>, MailMcpError>;

    /// Get a single message by row ID.
    fn get_message(&self, id: i64) -> Result<Option<MessageRow>, MailMcpError>;

    /// Get recipients for a message.
    fn get_recipients(&self, message_id: i64) -> Result<Vec<(String, i32)>, MailMcpError>;

    /// List all mailboxes with their row IDs and URLs.
    fn list_mailboxes(&self) -> Result<Vec<(i64, String)>, MailMcpError>;

    /// List accounts aggregated from mailbox URLs.
    fn list_accounts(&self) -> Result<Vec<AccountRow>, MailMcpError>;

    /// Count messages in a specific mailbox.
    fn count_messages_in_mailbox(&self, mailbox_id: i64) -> Result<i64, MailMcpError>;

    /// Check if an email address exists in the address index.
    fn address_exists(&self, address: &str) -> Result<bool, MailMcpError>;

    /// Get summary and attachment count metadata for a batch of message IDs.
    fn get_message_metadata(
        &self,
        message_ids: &[i64],
    ) -> Result<HashMap<i64, MessageMetadata>, MailMcpError>;

    /// Detect the timestamp epoch offset used by the database.
    fn detect_epoch_offset(&self) -> Result<i64, MailMcpError>;

    /// Downcast support for concrete implementations that need internal access.
    fn as_any(&self) -> &dyn Any;
}

impl dyn MailRepository {
    pub fn as_any(&self) -> &dyn Any {
        MailRepository::as_any(self)
    }
}

/// In-memory fake repository for testing tools without a database.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FakeMailRepository {
    pub messages: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<i64, MessageRow>>>,
    pub mailboxes: std::sync::Arc<std::sync::Mutex<Vec<(i64, String)>>>,
    pub accounts: std::sync::Arc<std::sync::Mutex<Vec<AccountRow>>>,
    pub epoch_offset: i64,
}

#[cfg(test)]
impl Default for FakeMailRepository {
    fn default() -> Self {
        Self {
            messages: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            mailboxes: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            accounts: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            epoch_offset: 0,
        }
    }
}

#[cfg(test)]
impl MailRepository for FakeMailRepository {
    fn search_messages(&self, params: SearchParams) -> Result<Vec<MessageRow>, MailMcpError> {
        let messages = self.messages.lock().unwrap();
        let mut results: Vec<MessageRow> = messages.values().cloned().collect();

        if let Some(subject) = &params.subject_query {
            let subject_lower = subject.to_lowercase();
            results.retain(|m| {
                m.subject
                    .as_ref()
                    .is_some_and(|s| s.to_lowercase().contains(&subject_lower))
            });
        }
        if let Some(sender) = &params.sender {
            results.retain(|m| m.sender.as_ref() == Some(sender));
        }
        if let Some(account) = &params.account {
            results.retain(|m| {
                m.mailbox_url
                    .as_ref()
                    .is_some_and(|u| u.starts_with(&format!("{account}/")))
            });
        }
        if let Some(allowed) = &params.allowed_accounts
            && !allowed.is_empty()
        {
            results.retain(|m| {
                m.mailbox_url
                    .as_ref()
                    .is_some_and(|u| allowed.iter().any(|a| u.starts_with(&format!("{a}/"))))
            });
        }
        if let Some(mailbox) = &params.mailbox {
            results.retain(|m| m.mailbox_url.as_ref().is_some_and(|u| u.contains(mailbox)));
        }

        results.sort_by_key(|b| std::cmp::Reverse(b.date_received));

        let start = params.offset as usize;
        if start >= results.len() {
            return Ok(vec![]);
        }
        let end = (start + params.limit as usize).min(results.len());
        Ok(results[start..end].to_vec())
    }

    fn get_message(&self, id: i64) -> Result<Option<MessageRow>, MailMcpError> {
        Ok(self.messages.lock().unwrap().get(&id).cloned())
    }

    fn get_recipients(&self, _message_id: i64) -> Result<Vec<(String, i32)>, MailMcpError> {
        Ok(vec![])
    }

    fn list_mailboxes(&self) -> Result<Vec<(i64, String)>, MailMcpError> {
        Ok(self.mailboxes.lock().unwrap().clone())
    }

    fn list_accounts(&self) -> Result<Vec<AccountRow>, MailMcpError> {
        Ok(self.accounts.lock().unwrap().clone())
    }

    fn count_messages_in_mailbox(&self, _mailbox_id: i64) -> Result<i64, MailMcpError> {
        Ok(0)
    }

    fn detect_epoch_offset(&self) -> Result<i64, MailMcpError> {
        Ok(self.epoch_offset)
    }

    fn address_exists(&self, address: &str) -> Result<bool, MailMcpError> {
        let messages = self.messages.lock().unwrap();
        Ok(messages
            .values()
            .any(|m| m.sender.as_deref() == Some(address)))
    }

    fn get_message_metadata(
        &self,
        message_ids: &[i64],
    ) -> Result<HashMap<i64, MessageMetadata>, MailMcpError> {
        let messages = self.messages.lock().unwrap();
        let mut metadata = HashMap::new();
        for id in message_ids {
            if messages.contains_key(id) {
                metadata.insert(
                    *id,
                    MessageMetadata {
                        summary: None,
                        attachment_count: 0,
                    },
                );
            }
        }
        Ok(metadata)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
