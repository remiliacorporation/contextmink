use std::io::{self, Write};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;

const RECEIPT_PREFIX: &str = "CONTEXTMINK_RECEIPT ";

pub(crate) fn clamp_text(value: &str, max_chars: usize) -> String {
    let mut iter = value.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(ch) = iter.next() else {
            return output;
        };
        output.push(ch);
    }
    if iter.next().is_some() {
        output.push_str("...");
    }
    output
}

pub(crate) fn emit_json(value: Value) -> Result<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer_pretty(&mut stdout, &value)?;
    writeln!(stdout)?;
    Ok(())
}

pub(crate) fn emit_json_checked(cli: &Cli, value: Value) -> Result<()> {
    let truncated = receipt_truncated_from_value(&value);
    let scan_capped = receipt_scan_capped_from_value(&value);
    emit_json(value)?;
    fail_after_receipt(cli, truncated, scan_capped)
}

fn write_receipt(map: serde_json::Map<String, Value>) -> Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{RECEIPT_PREFIX}{}", Value::Object(map))?;
    Ok(())
}

pub(crate) fn write_receipt_checked(cli: &Cli, map: serde_json::Map<String, Value>) -> Result<()> {
    let truncated = receipt_truncated_from_map(&map);
    let scan_capped = receipt_scan_capped_from_map(&map);
    write_receipt(map)?;
    fail_after_receipt(cli, truncated, scan_capped)
}

fn receipt_truncated_from_value(value: &Value) -> bool {
    value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn receipt_truncated_from_map(map: &serde_json::Map<String, Value>) -> bool {
    map.get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn receipt_scan_capped_from_value(value: &Value) -> bool {
    cap_reason_is_scan(value.get("cap_reason"))
        || value
            .get("candidate_files_total_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || value
            .get("matched_files_total_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || value
            .get("total_matches_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || value
            .get("rows_total_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn receipt_scan_capped_from_map(map: &serde_json::Map<String, Value>) -> bool {
    cap_reason_is_scan(map.get("cap_reason"))
        || map
            .get("candidate_files_total_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || map
            .get("matched_files_total_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || map
            .get("total_matches_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || map
            .get("rows_total_is_lower_bound")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn cap_reason_is_scan(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str) == Some("scan")
}

fn fail_after_receipt(cli: &Cli, truncated: bool, scan_capped: bool) -> Result<()> {
    if cli.require_complete_scan && scan_capped {
        return Err(anyhow!(
            "contextmink scan was capped (--require-complete-scan)"
        ));
    }
    if cli.fail_if_truncated && truncated {
        return Err(anyhow!(
            "contextmink output was truncated (strict completion requested)"
        ));
    }
    Ok(())
}

/// Build the common receipt envelope. `shown`/`total` always carry the unit
/// named by `unit` (files, lines, chars, or matches) regardless of which cap
/// fired, so a machine consumer can rely on stable field semantics.
pub(crate) fn base_receipt(
    command: &str,
    profile: Option<&str>,
    unit: &str,
    shown: usize,
    total: usize,
    truncated: bool,
    cap_reason: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("tool".to_string(), json!("contextmink"));
    map.insert("command".to_string(), json!(command));
    map.insert("profile".to_string(), json!(profile));
    map.insert("unit".to_string(), json!(unit));
    map.insert("shown".to_string(), json!(shown));
    map.insert("total".to_string(), json!(total));
    map.insert("truncated".to_string(), json!(truncated));
    map.insert("complete".to_string(), json!(!truncated));
    map.insert("cap_reason".to_string(), json!(cap_reason));
    map
}

/// Grep receipts keep `shown`/`total` in file units in every path and report
/// match/sample/scan detail in dedicated fields, so `cap_reason` (not the unit
/// of `total`) is what tells a consumer why output stopped.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_grep_receipt(
    cli: &Cli,
    command_name: &str,
    config: &ContextConfig,
    files_shown: usize,
    matched_files_total: usize,
    total_matches: usize,
    sample_lines_shown: usize,
    candidate_files_total: usize,
    candidate_files_scanned: usize,
    content_files_scanned: usize,
    skipped_large_or_binary: usize,
    truncated: bool,
    matched_files_total_is_lower_bound: bool,
    total_matches_is_lower_bound: bool,
    cap_reason: Option<&str>,
) -> Result<()> {
    let mut map = base_receipt(
        command_name,
        config.profile.as_deref(),
        "files",
        files_shown,
        matched_files_total,
        truncated,
        cap_reason,
    );
    map.insert("total_matches".to_string(), json!(total_matches));
    map.insert("sample_lines_shown".to_string(), json!(sample_lines_shown));
    map.insert(
        "candidate_files_total".to_string(),
        json!(candidate_files_total),
    );
    map.insert(
        "candidate_files_scanned".to_string(),
        json!(candidate_files_scanned),
    );
    map.insert(
        "content_files_scanned".to_string(),
        json!(content_files_scanned),
    );
    map.insert(
        "candidate_files_total_is_lower_bound".to_string(),
        json!(matches!(cap_reason, Some("scan"))),
    );
    map.insert(
        "matched_files_total_is_lower_bound".to_string(),
        json!(matched_files_total_is_lower_bound),
    );
    map.insert(
        "total_matches_is_lower_bound".to_string(),
        json!(total_matches_is_lower_bound),
    );
    map.insert(
        "skipped_large_or_binary".to_string(),
        json!(skipped_large_or_binary),
    );
    map.insert(
        "no_match_scope".to_string(),
        json!(no_match_scope(
            matched_files_total == 0,
            matches!(cap_reason, Some("scan"))
        )),
    );
    write_receipt_checked(cli, map)
}

pub(crate) fn no_match_scope(no_matches: bool, scan_truncated: bool) -> Option<&'static str> {
    if !no_matches {
        None
    } else if scan_truncated {
        Some("scanned_subset")
    } else {
        Some("complete_scope")
    }
}

#[cfg(test)]
mod tests;
