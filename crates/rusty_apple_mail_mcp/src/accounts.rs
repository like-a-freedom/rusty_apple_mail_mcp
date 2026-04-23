//! Read-only account metadata loading and selector resolution.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};

use crate::error::MailMcpError;

/// Human-friendly metadata for a Mail account derived from `Accounts4.sqlite`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMetadata {
    /// Canonical Mail account identifier, such as `ews://UUID`.
    pub account_id: String,
    /// Friendly account name, when available.
    pub account_name: Option<String>,
    /// Primary email address, when available.
    pub email: Option<String>,
    /// Raw username from the Accounts database, when available.
    pub username: Option<String>,
    /// Stable UUID-like identifier stored by macOS Accounts.
    pub source_identifier: String,
    /// Mail protocol family, such as `ews`, `imap`, `pop`, or `local`.
    pub account_type: String,
}

/// Default path to the macOS Accounts `SQLite` database.
#[must_use]
pub fn default_accounts_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library/Accounts/Accounts4.sqlite"))
}

/// Load Mail account metadata from the macOS Accounts database.
///
/// # Errors
///
/// Returns [`MailMcpError::Sqlite`] if the database cannot be opened or queried.
pub fn load_account_metadata(
    accounts_db_path: &Path,
) -> Result<HashMap<String, AccountMetadata>, MailMcpError> {
    let conn = Connection::open_with_flags(
        accounts_db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    load_account_metadata_with_conn(&conn)
}

/// Load Mail account metadata from an already-open Accounts database connection.
///
/// # Errors
///
/// Returns [`MailMcpError::Sqlite`] if the query fails.
pub fn load_account_metadata_with_conn(
    conn: &Connection,
) -> Result<HashMap<String, AccountMetadata>, MailMcpError> {
    let mut stmt = conn.prepare(
        r"
        SELECT
            a.Z_PK,
            a.ZACCOUNTDESCRIPTION,
            a.ZUSERNAME,
            a.ZIDENTIFIER,
            t.ZIDENTIFIER,
            t.ZACCOUNTTYPEDESCRIPTION
        FROM ZACCOUNT a
        LEFT JOIN ZACCOUNTTYPE t ON t.Z_PK = a.ZACCOUNTTYPE
        ORDER BY a.Z_PK
        ",
    )?;

    let mut rows = stmt.query([])?;
    let mut metadata = HashMap::new();

    while let Some(row) = rows.next()? {
        let owner_id: i64 = row.get(0)?;
        let account_name = normalize_optional(row.get::<_, Option<String>>(1)?);
        let username = normalize_optional(row.get::<_, Option<String>>(2)?);
        let source_identifier = normalize_optional(row.get::<_, Option<String>>(3)?)
            .ok_or_else(|| MailMcpError::Config("Accounts4 row missing ZIDENTIFIER".to_string()))?;
        let type_identifier = normalize_optional(row.get::<_, Option<String>>(4)?);
        let type_description = normalize_optional(row.get::<_, Option<String>>(5)?);

        let Some(account_type) =
            mail_scheme(type_identifier.as_deref(), type_description.as_deref())
        else {
            continue;
        };

        let properties = load_property_values(conn, owner_id)?;
        let email = properties
            .get("IdentityEmailAddress")
            .and_then(|bytes| extract_email(bytes))
            .or_else(|| {
                properties
                    .get("EmailAliases")
                    .and_then(|bytes| extract_email(bytes))
            })
            .or_else(|| username.as_deref().and_then(normalize_email));

        let property_name = properties
            .get("ACPropertyFullName")
            .and_then(|bytes| extract_name(bytes));

        let record = AccountMetadata {
            account_id: format!("{account_type}://{source_identifier}"),
            account_name: account_name.or(property_name),
            email,
            username,
            source_identifier,
            account_type: account_type.to_string(),
        };

        metadata.insert(record.account_id.clone(), record);
    }

    Ok(metadata)
}

/// Resolve human-friendly selectors to canonical Mail account identifiers.
///
/// # Errors
///
/// Returns [`MailMcpError::Config`] if no accounts match the selectors.
#[allow(clippy::implicit_hasher)]
pub fn resolve_account_selectors(
    selectors: &[String],
    accounts: &HashMap<String, AccountMetadata>,
) -> Result<Vec<String>, MailMcpError> {
    let mut resolved = BTreeSet::new();

    for selector in selectors {
        let normalized = normalize_selector(selector);
        let matches = accounts
            .values()
            .filter(|account| selector_matches(account, &normalized))
            .map(|account| account.account_id.clone())
            .collect::<BTreeSet<_>>();

        match matches.len() {
            0 => {
                return Err(MailMcpError::Config(format!(
                    "APPLE_MAIL_ACCOUNT selector '{selector}' did not match any Mail account"
                )));
            }
            1 => {
                resolved.extend(matches);
            }
            _ => {
                let joined = matches.into_iter().collect::<Vec<_>>().join(", ");
                return Err(MailMcpError::Config(format!(
                    "APPLE_MAIL_ACCOUNT selector '{selector}' is ambiguous and matched multiple accounts: {joined}"
                )));
            }
        }
    }

    Ok(resolved.into_iter().collect())
}

fn load_property_values(
    conn: &Connection,
    owner_id: i64,
) -> Result<HashMap<String, Vec<u8>>, MailMcpError> {
    let mut stmt = conn.prepare(
        r"
        SELECT ZKEY, ZVALUE
        FROM ZACCOUNTPROPERTY
        WHERE ZOWNER = ?
          AND ZKEY IN ('IdentityEmailAddress', 'EmailAliases', 'ACPropertyFullName')
        ",
    )?;

    let rows = stmt.query_map(params![owner_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;

    let mut values = HashMap::new();
    for row in rows {
        let (key, value) = row?;
        values.insert(key, value);
    }

    Ok(values)
}

fn mail_scheme(
    type_identifier: Option<&str>,
    type_description: Option<&str>,
) -> Option<&'static str> {
    match type_identifier {
        Some("com.apple.account.Exchange") => Some("ews"),
        Some("com.apple.account.IMAP") => Some("imap"),
        Some("com.apple.account.POP") => Some("pop"),
        Some("com.apple.account.OnMyDevice") => Some("local"),
        _ => match type_description {
            Some(description) if description.eq_ignore_ascii_case("Exchange") => Some("ews"),
            Some(description) if description.eq_ignore_ascii_case("IMAP") => Some("imap"),
            Some(description) if description.eq_ignore_ascii_case("POP") => Some("pop"),
            Some(description) if description.eq_ignore_ascii_case("On My Device") => Some("local"),
            _ => None,
        },
    }
}

fn selector_matches(account: &AccountMetadata, selector: &str) -> bool {
    [
        account.account_name.as_deref(),
        account.email.as_deref(),
        account.username.as_deref(),
        Some(account.account_id.as_str()),
    ]
    .into_iter()
    .flatten()
    .map(normalize_selector)
    .any(|candidate| candidate == selector)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_selector(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_email(value: &str) -> Option<String> {
    let trimmed = value.trim();
    trimmed.contains('@').then(|| trimmed.to_ascii_lowercase())
}

fn extract_email(bytes: &[u8]) -> Option<String> {
    extract_printable_fragments(bytes)
        .into_iter()
        .find_map(|fragment| normalize_email(&fragment))
}

fn extract_name(bytes: &[u8]) -> Option<String> {
    extract_printable_fragments(bytes)
        .into_iter()
        .filter(|fragment| !is_archive_noise(fragment))
        .max_by_key(String::len)
}

fn extract_printable_fragments(bytes: &[u8]) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut current = Vec::new();

    for byte in bytes {
        let is_printable = matches!(byte, b' '..=b'~');
        if is_printable {
            current.push(*byte);
        } else if current.len() >= 3 {
            fragments.push(String::from_utf8_lossy(&current).trim().to_string());
            current.clear();
        } else {
            current.clear();
        }
    }

    if current.len() >= 3 {
        fragments.push(String::from_utf8_lossy(&current).trim().to_string());
    }

    fragments
        .into_iter()
        .filter(|fragment| !fragment.is_empty())
        .collect()
}

fn is_archive_noise(fragment: &str) -> bool {
    matches!(
        fragment,
        "$null"
            | "$objects"
            | "$top"
            | "$class"
            | "NSKeyedArchiver"
            | "NSDictionary"
            | "NSArray"
            | "NSMutableString"
            | "NSString"
            | "NSObject"
            | "NS.keys"
            | "NS.objects"
    ) || fragment.starts_with('$')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_accounts_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE ZACCOUNT (
                Z_PK INTEGER PRIMARY KEY,
                ZACCOUNTDESCRIPTION VARCHAR,
                ZUSERNAME VARCHAR,
                ZIDENTIFIER VARCHAR,
                ZACCOUNTTYPE INTEGER
            );
            CREATE TABLE ZACCOUNTTYPE (
                Z_PK INTEGER PRIMARY KEY,
                ZIDENTIFIER VARCHAR,
                ZACCOUNTTYPEDESCRIPTION VARCHAR
            );
            CREATE TABLE ZACCOUNTPROPERTY (
                Z_PK INTEGER PRIMARY KEY,
                ZOWNER INTEGER,
                ZKEY VARCHAR,
                ZVALUE BLOB
            );

            INSERT INTO ZACCOUNTTYPE VALUES
                (1, 'com.apple.account.Exchange', 'Exchange'),
                (2, 'com.apple.account.IMAP', 'IMAP'),
                (3, 'com.apple.account.OnMyDevice', 'On My Device');

            INSERT INTO ZACCOUNT VALUES
                (10, 'Work Email', 'user\\work', 'EWS-UUID', 1),
                (20, 'Personal Gmail', 'solovey.anton@gmail.com', 'IMAP-UUID', 2),
                (30, 'On My Mac', NULL, 'LOCAL-UUID', 3);

            INSERT INTO ZACCOUNTPROPERTY VALUES
                (1, 10, 'IdentityEmailAddress', x'7573657240776F726B2E6578616D706C652E636F6D'),
                (2, 10, 'ACPropertyFullName', x'576F726B2055736572'),
                (3, 20, 'IdentityEmailAddress', x'736F6C6F7665792E616E746F6E40676D61696C2E636F6D');
            "#,
        )
        .expect("seed accounts db");
        conn
    }

    #[test]
    fn load_account_metadata_maps_mail_accounts_and_extracts_email() {
        let conn = make_accounts_db();

        let accounts = load_account_metadata_with_conn(&conn).expect("accounts metadata");

        assert_eq!(accounts.len(), 3);
        let exchange = accounts.get("ews://EWS-UUID").expect("exchange account");
        assert_eq!(exchange.account_name.as_deref(), Some("Work Email"));
        assert_eq!(exchange.email.as_deref(), Some("user@work.example.com"));

        let imap = accounts.get("imap://IMAP-UUID").expect("imap account");
        assert_eq!(imap.email.as_deref(), Some("solovey.anton@gmail.com"));
    }

    #[test]
    fn resolve_account_selectors_matches_name_and_email() {
        let conn = make_accounts_db();
        let accounts = load_account_metadata_with_conn(&conn).expect("accounts metadata");

        let resolved = resolve_account_selectors(
            &[
                "Work Email".to_string(),
                "solovey.anton@gmail.com".to_string(),
            ],
            &accounts,
        )
        .expect("selectors should resolve");

        assert_eq!(resolved, vec!["ews://EWS-UUID", "imap://IMAP-UUID"]);
    }

    #[test]
    fn resolve_account_selectors_rejects_unknown_selector() {
        let conn = make_accounts_db();
        let accounts = load_account_metadata_with_conn(&conn).expect("accounts metadata");

        let error = resolve_account_selectors(&["missing".to_string()], &accounts)
            .expect_err("unknown selector should fail");

        assert!(error.to_string().contains("did not match any Mail account"));
    }

    #[test]
    fn resolve_account_selectors_rejects_ambiguous_selector() {
        let conn = make_accounts_db();
        conn.execute(
            "INSERT INTO ZACCOUNT VALUES (40, 'Work Email', 'other@example.com', 'IMAP-UUID-2', 2)",
            [],
        )
        .expect("insert extra account");

        let accounts = load_account_metadata_with_conn(&conn).expect("accounts metadata");
        let error = resolve_account_selectors(&["Work Email".to_string()], &accounts)
            .expect_err("ambiguous selector should fail");

        assert!(error.to_string().contains("ambiguous"));
    }

    #[test]
    fn normalize_selector_trims_and_lowercases() {
        assert_eq!(normalize_selector("  WORK EMAIL  "), "work email");
        assert_eq!(normalize_selector("User@Example.com"), "user@example.com");
        assert_eq!(normalize_selector(""), "");
        assert_eq!(normalize_selector("  "), "");
    }

    #[test]
    fn normalize_email_requires_at_symbol() {
        assert_eq!(
            normalize_email("user@example.com"),
            Some("user@example.com".to_string())
        );
        assert_eq!(
            normalize_email("  USER@EXAMPLE.COM  "),
            Some("user@example.com".to_string())
        );
        assert_eq!(normalize_email("not-an-email"), None);
        assert_eq!(normalize_email(""), None);
        assert_eq!(normalize_email("  "), None);
    }

    #[test]
    fn extract_printable_fragments_skips_short_sequences() {
        // Sequences < 3 bytes are skipped
        let bytes = b"AB\x00CD\x00XYZ";
        let fragments = extract_printable_fragments(bytes);
        assert_eq!(fragments, vec!["XYZ"]);

        // Empty bytes
        let fragments = extract_printable_fragments(b"");
        assert!(fragments.is_empty());

        // All non-printable
        let fragments = extract_printable_fragments(&[0x00, 0x01, 0x02]);
        assert!(fragments.is_empty());
    }

    #[test]
    fn is_archive_noise_detects_common_patterns() {
        assert!(is_archive_noise("$null"));
        assert!(is_archive_noise("$objects"));
        assert!(is_archive_noise("$top"));
        assert!(is_archive_noise("$class"));
        assert!(is_archive_noise("NSKeyedArchiver"));
        assert!(is_archive_noise("NSDictionary"));
        assert!(is_archive_noise("NSArray"));
        assert!(is_archive_noise("NSString"));
        assert!(is_archive_noise("NSObject"));
        assert!(is_archive_noise("NS.keys"));
        assert!(is_archive_noise("NS.objects"));
        assert!(is_archive_noise("$custom"));
        assert!(!is_archive_noise("John Doe"));
        assert!(!is_archive_noise("Hello World"));
    }

    #[test]
    fn selector_matches_checks_all_fields() {
        let account = AccountMetadata {
            account_id: "ews://test-uuid".to_string(),
            account_name: Some("Test Account".to_string()),
            email: Some("test@example.com".to_string()),
            username: Some("testuser".to_string()),
            source_identifier: "test-uuid".to_string(),
            account_type: "ews".to_string(),
        };

        // Match by account name (case-insensitive)
        assert!(selector_matches(&account, "test account"));

        // Match by email
        assert!(selector_matches(&account, "test@example.com"));

        // Match by username
        assert!(selector_matches(&account, "testuser"));

        // Match by account_id
        assert!(selector_matches(&account, "ews://test-uuid"));

        // No match
        assert!(!selector_matches(&account, "unknown"));
        assert!(!selector_matches(&account, "other@example.com"));
    }
}
