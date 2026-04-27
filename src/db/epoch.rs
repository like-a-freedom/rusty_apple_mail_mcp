//! Timestamp epoch detection helpers for Apple Mail `SQLite` data.

use chrono::{Datelike, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension};

use crate::error::MailMcpError;

/// `CoreData` epoch offset: seconds from 1970-01-01 to 2001-01-01.
pub const COREDATA_EPOCH_OFFSET: i64 = 978_307_200;

/// Detect the timestamp offset used by the Apple Mail database.
///
/// Returns `0` for Unix epoch or [`COREDATA_EPOCH_OFFSET`] for `CoreData` epoch.
///
/// # Errors
///
/// Returns [`MailMcpError::Sqlite`] if the query fails.
pub fn detect_epoch_offset_seconds(conn: &Connection) -> Result<i64, MailMcpError> {
    let sample: Option<i64> = conn
        .query_row(
            "SELECT MAX(COALESCE(date_received, date_sent)) FROM messages",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    Ok(sample.map_or(0, infer_epoch_offset_from_sample))
}

fn infer_epoch_offset_from_sample(sample: i64) -> i64 {
    let now = Utc::now().timestamp();
    let unix_year = Utc.timestamp_opt(sample, 0).single().map(|dt| dt.year());
    let coredata_year = Utc
        .timestamp_opt(sample + COREDATA_EPOCH_OFFSET, 0)
        .single()
        .map(|dt| dt.year());

    let unix_plausible = unix_year.is_some_and(|year| (1990..=2100).contains(&year));
    let core_plausible = coredata_year.is_some_and(|year| (1990..=2100).contains(&year));

    match (unix_plausible, core_plausible) {
        (false, true) => COREDATA_EPOCH_OFFSET,
        (true, false) => 0,
        _ => {
            let unix_distance = (sample - now).abs();
            let core_distance = (sample + COREDATA_EPOCH_OFFSET - now).abs();
            if core_distance < unix_distance {
                COREDATA_EPOCH_OFFSET
            } else {
                0
            }
        }
    }
}
