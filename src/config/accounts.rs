use std::collections::HashMap;

use crate::accounts::{AccountMetadata, default_accounts_db_path, load_account_metadata};
use crate::error::MailMcpError;

/// Load account metadata from the system Accounts database for the given selectors.
pub fn load_account_metadata_for_selectors(
    _account_selectors: &[String],
) -> Result<HashMap<String, AccountMetadata>, MailMcpError> {
    let Some(accts_db) = default_accounts_db_path() else {
        return Ok(HashMap::new());
    };
    if !accts_db.exists() {
        return Ok(HashMap::new());
    }
    load_account_metadata(&accts_db)
}
