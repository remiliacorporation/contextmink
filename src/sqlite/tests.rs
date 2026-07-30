use super::*;

#[test]
fn sqlite_value_summary_reports_character_truncation_for_every_value_kind() {
    let text = sqlite_value_summary(ValueRef::Text(b"abcdefghijklmnopqrstuvwxyz"), 10);
    assert_eq!(text.text, "\"abcde...\"");
    assert!(text.truncated);

    let integer = sqlite_value_summary(ValueRef::Integer(123), 2);
    assert_eq!(integer.text, "..");
    assert!(integer.truncated);

    let null = sqlite_value_summary(ValueRef::Null, 10);
    assert_eq!(null.text, "null");
    assert!(!null.truncated);
}

#[test]
fn sqlite_validation_preserves_the_engine_diagnostic() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY);")
        .unwrap();

    let error =
        reject_multiple_sqlite_statements(&connection, "SELECT missing_column FROM records")
            .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("no such column: missing_column"),
        "missing SQLite diagnostic: {message}"
    );
    assert!(
        message.contains("at byte 7"),
        "wrong error offset: {message}"
    );
}
