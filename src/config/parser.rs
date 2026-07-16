use std::path::PathBuf;

use crate::error::MailMcpError;

/// CLI override values for optional configuration fields.
#[derive(Debug, Default)]
pub struct MailConfigOverrides {
    pub mail_directory: Option<PathBuf>,
    pub mail_version: Option<String>,
    pub account: Option<String>,
}

/// Parse comma-separated account selectors, trimming whitespace.
pub fn parse_account_selectors(raw: Option<&str>) -> Result<Vec<String>, MailMcpError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let selectors: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();

    if selectors.is_empty() {
        return Err(MailMcpError::Config(
            "APPLE_MAIL_ACCOUNT was provided, but no account selectors were found".to_string(),
        ));
    }

    Ok(selectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_account_selectors_requires_non_empty_values() {
        let error = parse_account_selectors(Some(" ,  , ")).expect_err("empty selectors fail");
        assert!(error.to_string().contains("APPLE_MAIL_ACCOUNT"));
    }

    #[test]
    fn parse_account_selectors_splits_and_trims_values() {
        let selectors =
            parse_account_selectors(Some(" Work Email, user@work.example.com ,imap://personal "))
                .expect("selectors should parse");

        assert_eq!(
            selectors,
            vec!["Work Email", "user@work.example.com", "imap://personal"]
        );
    }

    #[test]
    fn parse_account_selectors_single_value() {
        let selectors = parse_account_selectors(Some("account1")).expect("single selector parse");
        assert_eq!(selectors, vec!["account1"]);
    }

    #[test]
    fn parse_account_selectors_empty_after_trim() {
        let error = parse_account_selectors(Some("")).expect_err("empty string fails");
        assert!(error.to_string().contains("APPLE_MAIL_ACCOUNT"));
    }

    #[test]
    fn parse_account_selectors_none_returns_empty() {
        let result = parse_account_selectors(None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
