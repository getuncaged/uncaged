use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A point in time recorded on a cloud object (revisions, last-updated and trashed times).
///
/// SQLite stores these as microseconds since the Unix epoch, and serde writes them as an
/// RFC 3339 string; both formats are what persisted data already holds, so they must not change.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct ServerTimestamp(DateTime<Utc>);

impl ServerTimestamp {
    pub fn new(time: DateTime<Utc>) -> Self {
        Self(time)
    }

    /// Converts microseconds since the Unix epoch into a [`ServerTimestamp`].
    pub fn from_unix_timestamp_micros(micros_since_epoch: i64) -> Result<Self> {
        let date_time = DateTime::from_timestamp_micros(micros_since_epoch)
            .ok_or_else(|| anyhow!("Unable to convert microseconds into NaiveDateTime"))?;
        Ok(ServerTimestamp::new(date_time))
    }

    pub fn timestamp_micros(&self) -> i64 {
        self.0.timestamp_micros()
    }

    pub fn utc(&self) -> DateTime<Utc> {
        self.0
    }
}

impl From<DateTime<Utc>> for ServerTimestamp {
    fn from(value: DateTime<Utc>) -> Self {
        ServerTimestamp::new(value)
    }
}

#[cfg(test)]
#[path = "time_tests.rs"]
mod tests;
