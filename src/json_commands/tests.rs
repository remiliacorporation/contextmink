use super::*;

#[test]
fn value_summary_reports_character_truncation() {
    let summary = value_summary(&json!("abcdefghijklmnopqrstuvwxyz"), 10);
    assert_eq!(summary.text, "\"abcdef...");
    assert!(summary.truncated);

    let summary = value_summary(&json!(42), 10);
    assert_eq!(summary.text, "42");
    assert!(!summary.truncated);
}

#[test]
fn json_search_text_is_not_bounded_by_rendering_limits() {
    let value = json!({"message": "prefix-value-after-the-render-cap"});
    assert!(json_search_text(&value).contains("value-after-the-render-cap"));
}
use serde_json::json;

#[test]
fn json_identifier_filter_matches_plain_keys_only() {
    assert!(is_json_identifier("alpha_beta1"));
    assert!(!is_json_identifier("1alpha"));
    assert!(!is_json_identifier("alpha-beta"));
}

#[test]
fn value_summary_keeps_large_json_structural() {
    let large = json!({
        "items": (0..120).map(|index| json!({"index": index})).collect::<Vec<_>>(),
        "kind": "large",
    });
    let summary = value_summary(&large, 80);
    assert!(summary.text.starts_with("<object:2 keys sample="));
    assert!(!summary.text.contains("\"index\":119"));
    assert!(!summary.truncated);

    let small = json!({"address": "0x7FF954", "function_count": 12});
    let summary = value_summary(&small, 200);
    assert_eq!(
        summary.text,
        "{\"address\":\"0x7FF954\",\"function_count\":12}"
    );
    assert!(!summary.truncated);
}

#[test]
fn shape_mismatched_json_pointer_token_is_a_non_match() {
    let row = json!({"v": [9]});
    assert_eq!(json_pointer_lookup(&row, "/v/x").unwrap(), None);
}
