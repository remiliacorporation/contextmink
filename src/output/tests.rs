use super::*;
use serde_json::{Value, json};

#[test]
fn clamp_text_is_character_safe() {
    assert_eq!(clamp_text("abcdef", 0), "");
    assert_eq!(clamp_text("abcdef", 1), ".");
    assert_eq!(clamp_text("abcdef", 2), "..");
    assert_eq!(clamp_text("abcdef", 3), "...");
    assert_eq!(clamp_text("abcdef", 4), "a...");
    assert_eq!(clamp_text("éclair", 5), "éc...");
    assert_eq!(clamp_text("éclair", 5).chars().count(), 5);
    assert_eq!(clamp_text("abc", 3), "abc");
    assert_eq!(clamp_text("a->b", 20), "a->b");
}

#[test]
fn text_clamp_records_character_omissions_once_for_a_receipt() {
    let mut clamp = TextClamp::new(3, "--show-line-chars");
    assert_eq!(clamp.clamp("abc"), "abc");
    assert_eq!(clamp.clamp("éclair"), "...");

    let mut receipt = Receipt::new("slice", None, ReceiptResult::new("lines", 2, false, 2));
    clamp.add_receipt_cap(&mut receipt);
    let value = receipt.into_value();
    assert_eq!(value["output_truncated"], json!(true));
    assert_eq!(value["complete"], json!(false));
    assert_eq!(value["caps"].as_array().unwrap().len(), 1);
    assert_eq!(value["caps"][0]["dimension"], json!("line_characters"));
    assert_eq!(value["caps"][0]["limit"], json!(3));
    assert_eq!(value["output_cap_arguments"], json!(["--show-line-chars"]));
    assert!(value["caps"][0].get("argument").is_none());
}

#[test]
fn receipt_v2_derives_completion_from_structured_caps() {
    let mut receipt = Receipt::new(
        "grep",
        Some("demo"),
        ReceiptResult::new("matching_files", 12, true, 3),
    );
    receipt.add_cap(ReceiptCap::scope("candidate_files", Some(100)));
    receipt.add_cap(ReceiptCap::output(
        "matching_files",
        Some(3),
        "--show-files",
    ));
    receipt.add_cap(ReceiptCap::output(
        "line_characters",
        Some(9),
        "--show-line-chars",
    ));
    receipt.add_cap(ReceiptCap::output("paths", Some(3), "--show-files"));
    receipt.insert("pattern", json!("needle"));

    assert!(!receipt.scope_complete());
    assert!(receipt.output_truncated());

    let value = receipt.into_value();
    assert_eq!(value["schema"], json!("contextmink.receipt.v2"));
    assert_eq!(value["scope_complete"], json!(false));
    assert_eq!(value["output_truncated"], json!(true));
    assert_eq!(value["complete"], json!(false));
    assert_eq!(value["result"]["unit"], json!("matching_files"));
    assert_eq!(value["result"]["shown"], json!(3));
    assert_eq!(value["result"]["total"], json!(12));
    assert_eq!(value["result"]["total_is_lower_bound"], json!(true));
    assert_eq!(value["caps"][0]["boundary"], json!("scope"));
    assert_eq!(value["caps"][0]["dimension"], json!("candidate_files"));
    assert_eq!(value["caps"][0]["limit"], json!(100));
    assert_eq!(value["caps"][1]["boundary"], json!("output"));
    assert_eq!(value["pattern"], json!("needle"));
    assert_eq!(
        value["output_cap_arguments"],
        json!(["--show-files", "--show-line-chars"]),
        "every exhausted output control is named once, scope caps never"
    );
    assert!(value.get("truncated").is_none());
    assert!(value.get("cap_reason").is_none());
    assert!(value.get("unit").is_none());
    assert!(value.get("shown").is_none());
    assert!(value.get("total").is_none());
}

#[test]
fn receipt_without_caps_is_complete() {
    let receipt = Receipt::new("files", None, ReceiptResult::new("files", 5, false, 5));
    assert!(receipt.scope_complete());
    assert!(!receipt.output_truncated());

    let value = receipt.into_value();
    assert_eq!(value["profile"], Value::Null);
    assert_eq!(value["caps"], json!([]));
    assert_eq!(value["complete"], json!(true));
    assert!(value.get("output_cap_arguments").is_none());
}

#[test]
fn scope_only_caps_name_no_output_arguments() {
    let mut receipt = Receipt::new("sqlite", None, ReceiptResult::new("rows", 5, true, 5));
    receipt.add_cap(ReceiptCap::scope("rows_processed", Some(5)));
    let value = receipt.into_value();
    assert_eq!(value["output_truncated"], json!(false));
    assert!(value.get("output_cap_arguments").is_none());
}
