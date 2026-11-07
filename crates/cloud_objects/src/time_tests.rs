use super::ServerTimestamp;
use crate::cloud_object::Revision;

/// 2023-11-14T22:13:20.123456Z, as SQLite stores it (microseconds since the epoch).
const MICROS: i64 = 1_700_000_000_123_456;
const RFC3339: &str = "\"2023-11-14T22:13:20.123456Z\"";

#[test]
fn micros_round_trip() {
    let ts = ServerTimestamp::from_unix_timestamp_micros(MICROS).unwrap();
    assert_eq!(ts.timestamp_micros(), MICROS);
    assert_eq!(ts.utc().timestamp(), 1_700_000_000);

    let epoch = ServerTimestamp::from_unix_timestamp_micros(0).unwrap();
    assert_eq!(epoch.timestamp_micros(), 0);

    let before_epoch = ServerTimestamp::from_unix_timestamp_micros(-1).unwrap();
    assert_eq!(before_epoch.timestamp_micros(), -1);
}

#[test]
fn out_of_range_micros_fail() {
    assert!(ServerTimestamp::from_unix_timestamp_micros(i64::MAX).is_err());
}

#[test]
fn serializes_as_rfc3339_string() {
    let ts = ServerTimestamp::from_unix_timestamp_micros(MICROS).unwrap();
    assert_eq!(serde_json::to_string(&ts).unwrap(), RFC3339);

    let whole_seconds = ServerTimestamp::from_unix_timestamp_micros(1_700_000_000_000_000).unwrap();
    assert_eq!(
        serde_json::to_string(&whole_seconds).unwrap(),
        "\"2023-11-14T22:13:20Z\""
    );
}

#[test]
fn parses_stored_strings() {
    let ts: ServerTimestamp = serde_json::from_str(RFC3339).unwrap();
    assert_eq!(ts.timestamp_micros(), MICROS);

    // Offsets other than UTC are accepted and normalized, as chrono always has.
    let offset: ServerTimestamp =
        serde_json::from_str("\"2023-11-15T00:13:20.123456+02:00\"").unwrap();
    assert_eq!(offset, ts);
}

#[test]
fn revision_serializes_like_its_timestamp() {
    let revision = Revision::from_unix_timestamp_micros(MICROS).unwrap();
    assert_eq!(serde_json::to_string(&revision).unwrap(), RFC3339);
    let parsed: Revision = serde_json::from_str(RFC3339).unwrap();
    assert_eq!(parsed, revision);
    assert_eq!(parsed.timestamp_micros(), MICROS);
}
