use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::encoding::read_required_text;
use crate::files::display_path;
use crate::output::{
    ClampedText, Receipt, ReceiptCap, ReceiptResult, clamp_text_with_status, emit_json_checked,
    write_receipt_checked,
};

const JSON_SMALL_NODE_LIMIT: usize = 80;
const JSON_SMALL_STRING_CHAR_LIMIT: usize = 4096;

/// One `--where key=value` / `--where-contains key=text` predicate.
#[derive(Debug)]
pub(crate) struct WherePredicate {
    pub(crate) field: String,
    pub(crate) expected: String,
    pub(crate) contains: bool,
}

pub(crate) fn parse_where_predicates(
    exact: &[String],
    contains: &[String],
) -> Result<Vec<WherePredicate>> {
    let mut predicates = Vec::with_capacity(exact.len() + contains.len());
    for (values, is_contains, flag) in [
        (exact, false, "--where"),
        (contains, true, "--where-contains"),
    ] {
        for raw in values {
            let (field, expected) = raw
                .split_once('=')
                .ok_or_else(|| anyhow!("{flag} requires FIELD=VALUE, found {raw:?}"))?;
            if field.is_empty() {
                return Err(anyhow!("{flag} requires a non-empty field name: {raw:?}"));
            }
            predicates.push(WherePredicate {
                field: field.to_owned(),
                expected: expected.to_owned(),
                contains: is_contains,
            });
        }
    }
    Ok(predicates)
}

/// Compare a row field against a predicate. Strings compare by their
/// contents (no JSON quotes); other scalars by their JSON rendering.
fn where_predicate_matches(row: &Value, predicate: &WherePredicate) -> Result<bool> {
    let Some(value) = json_select_field(row, &predicate.field)? else {
        return Ok(false);
    };
    let rendered = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    Ok(if predicate.contains {
        rendered.contains(&predicate.expected)
    } else {
        rendered == predicate.expected
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_json_find(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    key_contains: &[String],
    key_regex: Option<&str>,
    path_contains: &[String],
    path_regex: Option<&str>,
    value_contains: &[String],
    max: usize,
    max_value_chars: usize,
) -> Result<()> {
    if max == 0 {
        return Err(anyhow!("json-find --limit must be greater than zero"));
    }
    if max_value_chars == 0 {
        return Err(anyhow!(
            "json-find --max-value-chars must be greater than zero"
        ));
    }
    if key_contains.is_empty()
        && key_regex.is_none()
        && path_contains.is_empty()
        && path_regex.is_none()
        && value_contains.is_empty()
    {
        return Err(anyhow!(
            "json-find requires --key-contains, --key-regex, --path-contains, --path-regex, or --value-contains"
        ));
    }
    let key_re = key_regex
        .map(Regex::new)
        .transpose()
        .context("invalid key regex")?;
    let path_re = path_regex
        .map(Regex::new)
        .transpose()
        .context("invalid path regex")?;
    let document = read_required_text(file)
        .with_context(|| format!("failed to read {}", file.display()))?
        .0;
    let (document, input_format) = parse_json_or_jsonl(&document)?;
    let mut rows = Vec::new();
    let mut value_characters_truncated = false;
    let mut total_matches = 0usize;
    walk_json("$", None, &document, &mut |path, key, value| {
        if let Some(key_re) = &key_re
            && !key.is_some_and(|key| key_re.is_match(key))
        {
            return;
        }
        if !key_contains.is_empty() && !key.is_some_and(|key| contains_any(key, key_contains)) {
            return;
        }
        if let Some(path_re) = &path_re
            && !path_re.is_match(path)
        {
            return;
        }
        if !path_contains.is_empty() && !contains_any(path, path_contains) {
            return;
        }
        if !value_contains.is_empty() && !contains_any(&json_search_text(value), value_contains) {
            return;
        }
        total_matches += 1;
        if rows.len() < max {
            let summary = value_summary(value, max_value_chars);
            value_characters_truncated |= summary.truncated;
            rows.push((path.to_owned(), summary.text));
        }
    });
    let shown = rows.len();
    let truncated = shown < total_matches;
    let mut receipt = Receipt::new(
        "json-find",
        config.profile.as_deref(),
        ReceiptResult::new("matches", total_matches, false, shown),
    );
    if truncated {
        receipt.add_cap(ReceiptCap::output("matches", Some(max)));
    }
    if value_characters_truncated {
        receipt.add_cap(ReceiptCap::output(
            "value_characters",
            Some(max_value_chars),
        ));
    }
    receipt.insert("path", json!(display_path(file)));
    receipt.insert("input_format", json!(input_format));
    if cli.json {
        receipt.insert(
            "matches",
            json!(
                rows.iter()
                    .take(shown)
                    .map(|(path, value)| json!({
                        "path": path,
                        "value": value,
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        if rows.is_empty() {
            writeln!(stdout, "no_matches")?;
        }
        for (path, value) in rows.iter().take(shown) {
            writeln!(stdout, "{path} = {value}")?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped json matches at {max}; narrow the selector."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

fn parse_json_or_jsonl(text: &str) -> Result<(Value, &'static str)> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => Ok((value, "json")),
        Err(json_error) => {
            let whole_document_error = json_error.to_string();
            let mut rows = Vec::new();
            let mut saw_line = false;
            for (index, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                saw_line = true;
                let value: Value = serde_json::from_str(trimmed).with_context(|| {
                    format!(
                        "failed to parse JSON (whole document: {whole_document_error}); \
                             failed to parse JSONL line {}",
                        index + 1
                    )
                })?;
                rows.push(value);
            }
            if saw_line {
                Ok((Value::Array(rows), "jsonl"))
            } else {
                Err(json_error).context("failed to parse JSON")
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_json_select(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    array: Option<&str>,
    fields: &[String],
    where_exact: &[String],
    where_contains: &[String],
    keys: bool,
    max: usize,
    max_value_chars: usize,
) -> Result<()> {
    if max == 0 {
        return Err(anyhow!("json-select --limit must be greater than zero"));
    }
    if max_value_chars == 0 {
        return Err(anyhow!(
            "json-select --max-value-chars must be greater than zero"
        ));
    }
    let array = array.map(str::to_owned);
    let fields = expand_json_select_fields(fields);
    if keys && !fields.is_empty() {
        return Err(anyhow!(
            "json-select --keys reports row shape and cannot be combined with --fields"
        ));
    }
    let predicates = parse_where_predicates(where_exact, where_contains)?;

    // Every selector that can silently produce nothing is typo-audited: a
    // field or predicate field that is null/missing in every scanned row is
    // reported instead of quietly projecting `null`.
    let mut audited_fields: Vec<String> = fields.clone();
    for predicate in &predicates {
        if !audited_fields.contains(&predicate.field) {
            audited_fields.push(predicate.field.clone());
        }
    }
    let mut field_seen_non_null = vec![false; audited_fields.len()];

    let is_jsonl_named = file
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"));
    let mut kept_rows: Vec<Value> = Vec::new();
    let mut key_stats: std::collections::BTreeMap<String, JsonKeyStat> =
        std::collections::BTreeMap::new();
    let mut non_object_rows = 0usize;
    let mut rows_scanned = 0usize;
    let mut rows_matched = 0usize;
    let input_format;
    if is_jsonl_named && array.is_none() {
        let mut consume_row = |index: usize, row: serde_json::Result<Value>| -> Result<()> {
            let row = row.with_context(|| {
                format!(
                    "failed to parse JSONL value {} in {}",
                    index + 1,
                    file.display()
                )
            })?;
            rows_scanned += 1;
            audit_fields(&row, &audited_fields, &mut field_seen_non_null)?;
            if !row_matches_predicates(&row, &predicates)? {
                return Ok(());
            }
            rows_matched += 1;
            if keys {
                collect_row_keys(&row, &mut key_stats, &mut non_object_rows);
            } else if kept_rows.len() < max {
                kept_rows.push(row);
            }
            Ok(())
        };
        // Stream UTF-8 JSONL row-by-row. UTF-16 remains a decoded,
        // materialized fallback so extension choice never changes correctness.
        let handle = std::fs::File::open(file)
            .with_context(|| format!("failed to open {}", file.display()))?;
        let mut reader = io::BufReader::new(handle);
        let prefix = reader
            .fill_buf()
            .with_context(|| format!("failed to inspect encoding for {}", file.display()))?;
        let utf16 = prefix.starts_with(&[0xFF, 0xFE]) || prefix.starts_with(&[0xFE, 0xFF]);
        if prefix.starts_with(&[0xEF, 0xBB, 0xBF]) {
            reader.consume(3);
        }
        if utf16 {
            drop(reader);
            let text = read_required_text(file)
                .with_context(|| format!("failed to read {}", file.display()))?
                .0;
            for (index, row) in serde_json::Deserializer::from_str(&text)
                .into_iter::<Value>()
                .enumerate()
            {
                consume_row(index, row)?;
            }
        } else {
            for (index, row) in serde_json::Deserializer::from_reader(reader)
                .into_iter::<Value>()
                .enumerate()
            {
                consume_row(index, row)?;
            }
        }
        input_format = "jsonl";
    } else {
        let text = read_required_text(file)
            .with_context(|| format!("failed to read {}", file.display()))?
            .0;
        let (document, parsed_format) = parse_json_or_jsonl(&text)?;
        input_format = parsed_format;
        let rows: Vec<&Value> = if let Some(pointer) = array.as_deref() {
            let selected = json_select_field(&document, pointer)?
                .ok_or_else(|| anyhow!("json-select --array selector did not match: {pointer}"))?;
            selected
                .as_array()
                .ok_or_else(|| {
                    anyhow!("json-select --array selector must resolve to an array: {pointer}")
                })?
                .iter()
                .collect()
        } else if input_format == "jsonl" {
            document
                .as_array()
                .expect("JSONL parser returns an array")
                .iter()
                .collect()
        } else {
            vec![&document]
        };
        for row in rows {
            rows_scanned += 1;
            audit_fields(row, &audited_fields, &mut field_seen_non_null)?;
            if !row_matches_predicates(row, &predicates)? {
                continue;
            }
            rows_matched += 1;
            if keys {
                collect_row_keys(row, &mut key_stats, &mut non_object_rows);
            } else if kept_rows.len() < max {
                kept_rows.push(row.clone());
            }
        }
    }

    let all_null_fields = audited_fields
        .iter()
        .zip(&field_seen_non_null)
        .filter_map(|(field, seen)| (rows_scanned > 0 && !seen).then_some(field.clone()))
        .collect::<Vec<_>>();

    if keys {
        return render_json_select_keys(
            cli,
            config,
            file,
            array.as_deref(),
            input_format,
            &key_stats,
            non_object_rows,
            rows_scanned,
            rows_matched,
            &all_null_fields,
            max,
        );
    }

    let shown = kept_rows.len();
    let truncated = shown < rows_matched;
    let where_labels = predicates
        .iter()
        .map(|predicate| {
            format!(
                "{}{}{}",
                predicate.field,
                if predicate.contains { "~=" } else { "=" },
                predicate.expected
            )
        })
        .collect::<Vec<_>>();
    let value_characters_truncated = kept_rows.iter().any(|row| {
        if fields.is_empty() {
            return value_summary(row, max_value_chars).truncated;
        }
        fields.iter().any(|field| {
            json_select_field(row, field)
                .expect("validated JSON selector remains valid during rendering")
                .is_some_and(|value| value_summary(value, max_value_chars).truncated)
        })
    });
    let mut receipt = Receipt::new(
        "json-select",
        config.profile.as_deref(),
        ReceiptResult::new("rows", rows_matched, false, shown),
    );
    if truncated {
        receipt.add_cap(ReceiptCap::output("rows", Some(max)));
    }
    if value_characters_truncated {
        receipt.add_cap(ReceiptCap::output(
            "value_characters",
            Some(max_value_chars),
        ));
    }
    receipt.insert("path", json!(display_path(file)));
    receipt.insert("array", json!(array.as_deref()));
    receipt.insert("input_format", json!(input_format));
    receipt.insert("fields", json!(fields));
    receipt.insert("where", json!(where_labels));
    receipt.insert("rows_scanned", json!(rows_scanned));
    receipt.insert("all_null_fields", json!(all_null_fields));
    if cli.json {
        receipt.insert(
            "rows",
            json!(
                kept_rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| json_select_row(index, row, &fields, max_value_chars))
                    .collect::<Result<Vec<_>>>()?
            ),
        );
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        let source = array.as_deref().unwrap_or(if input_format == "jsonl" {
            "jsonl"
        } else {
            "$"
        });
        let mut header = format!("[contextmink] json-select source={source}");
        if !fields.is_empty() {
            header.push_str(&format!(" fields={}", fields.join(",")));
        }
        if !where_labels.is_empty() {
            header.push_str(&format!(" where={}", where_labels.join(",")));
        }
        writeln!(stdout, "{header}")?;
        if kept_rows.is_empty() {
            writeln!(stdout, "no_rows")?;
        }
        for (index, row) in kept_rows.iter().enumerate() {
            if fields.is_empty() {
                writeln!(
                    stdout,
                    "{index}: {}",
                    value_summary(row, max_value_chars).text
                )?;
                continue;
            }
            let mut parts = Vec::with_capacity(fields.len());
            for field in &fields {
                let summary = json_select_field(row, field.as_str())?
                    .map(|value| value_summary(value, max_value_chars).text)
                    .unwrap_or_else(|| "null".to_owned());
                parts.push(format!("{field}={summary}"));
            }
            writeln!(stdout, "{index}: {}", parts.join(" "))?;
        }
        if !all_null_fields.is_empty() {
            writeln!(
                stdout,
                "[contextmink] warning: field(s) {} were null or missing in all {rows_scanned} scanned row(s); check the field selector against the document shape.",
                all_null_fields
                    .iter()
                    .map(|field| field.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped json rows at {max}; narrow the selector."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

#[derive(Debug, Default)]
struct JsonKeyStat {
    present: usize,
    non_null: usize,
    types: std::collections::BTreeSet<&'static str>,
}

fn collect_row_keys(
    row: &Value,
    key_stats: &mut std::collections::BTreeMap<String, JsonKeyStat>,
    non_object_rows: &mut usize,
) {
    let Some(object) = row.as_object() else {
        *non_object_rows += 1;
        return;
    };
    for (key, value) in object {
        let stat = key_stats.entry(key.clone()).or_default();
        stat.present += 1;
        if !value.is_null() {
            stat.non_null += 1;
        }
        stat.types.insert(json_value_type_name(value));
    }
}

fn json_value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `--keys` output: the union of top-level row keys with presence counts and
/// value types, so an unknown row shape is discoverable in one call instead
/// of a guess → all-null warning → slice → retry loop.
#[allow(clippy::too_many_arguments)]
fn render_json_select_keys(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    array: Option<&str>,
    input_format: &str,
    key_stats: &std::collections::BTreeMap<String, JsonKeyStat>,
    non_object_rows: usize,
    rows_scanned: usize,
    rows_matched: usize,
    all_null_fields: &[String],
    max: usize,
) -> Result<()> {
    let total = key_stats.len();
    let shown = total.min(max);
    let truncated = shown < total;
    let mut receipt = Receipt::new(
        "json-select",
        config.profile.as_deref(),
        ReceiptResult::new("keys", total, false, shown),
    );
    if truncated {
        receipt.add_cap(ReceiptCap::output("keys", Some(max)));
    }
    receipt.insert("path", json!(display_path(file)));
    receipt.insert("array", json!(array));
    receipt.insert("input_format", json!(input_format));
    receipt.insert("keys_mode", json!(true));
    receipt.insert("rows_scanned", json!(rows_scanned));
    receipt.insert("rows_matching", json!(rows_matched));
    receipt.insert("non_object_rows", json!(non_object_rows));
    receipt.insert("all_null_fields", json!(all_null_fields));
    let key_rows = key_stats
        .iter()
        .take(shown)
        .map(|(key, stat)| {
            json!({
                "key": key,
                "present": stat.present,
                "non_null": stat.non_null,
                "types": stat.types.iter().collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    if cli.json {
        receipt.insert("keys", json!(key_rows));
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        writeln!(
            stdout,
            "[contextmink] json-select keys source={} rows={rows_matched}",
            array.unwrap_or(if input_format == "jsonl" {
                "jsonl"
            } else {
                "$"
            })
        )?;
        if key_rows.is_empty() {
            writeln!(stdout, "no_keys")?;
        }
        for (index, row) in key_rows.iter().enumerate() {
            writeln!(
                stdout,
                "{index}: {} present={} non_null={} types={}",
                row["key"].as_str().unwrap_or_default(),
                row["present"],
                row["non_null"],
                row["types"]
                    .as_array()
                    .map(|types| {
                        types
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("|")
                    })
                    .unwrap_or_default()
            )?;
        }
        if non_object_rows > 0 {
            writeln!(
                stdout,
                "[contextmink] {non_object_rows} scanned row(s) were not JSON objects and carry no keys."
            )?;
        }
        if !all_null_fields.is_empty() {
            writeln!(
                stdout,
                "[contextmink] warning: field(s) {} were null or missing in all {rows_scanned} scanned row(s); check the field selector against the document shape.",
                all_null_fields.join(", ")
            )?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped keys at {max}; raise --limit or filter rows."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

fn expand_json_select_fields(fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .flat_map(|field| field.split(','))
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_owned)
        .collect()
}

fn audit_fields(
    row: &Value,
    audited_fields: &[String],
    field_seen_non_null: &mut [bool],
) -> Result<()> {
    for (field, seen) in audited_fields.iter().zip(field_seen_non_null.iter_mut()) {
        if !*seen && json_select_field(row, field)?.is_some_and(|value| !value.is_null()) {
            *seen = true;
        }
    }
    Ok(())
}

fn row_matches_predicates(row: &Value, predicates: &[WherePredicate]) -> Result<bool> {
    for predicate in predicates {
        if !where_predicate_matches(row, predicate)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn json_select_row(
    index: usize,
    row: &Value,
    fields: &[String],
    max_value_chars: usize,
) -> Result<Value> {
    if fields.is_empty() {
        return Ok(json!({
            "row": index,
            "value": value_summary(row, max_value_chars).text,
        }));
    }
    let mut output_fields = serde_json::Map::new();
    for field in fields {
        let summary = json_select_field(row, field.as_str())?
            .map(|value| value_summary(value, max_value_chars).text)
            .unwrap_or_else(|| "null".to_owned());
        output_fields.insert(field.clone(), json!(summary));
    }
    Ok(json!({
        "row": index,
        "fields": output_fields,
    }))
}

fn json_select_field<'a>(row: &'a Value, selector: &str) -> Result<Option<&'a Value>> {
    if selector == "$" || selector.starts_with('/') || selector.is_empty() {
        return json_pointer_lookup(row, selector);
    }
    Ok(row.as_object().and_then(|map| map.get(selector)))
}

fn json_pointer_lookup<'a>(value: &'a Value, pointer: &str) -> Result<Option<&'a Value>> {
    if pointer.is_empty() || pointer == "$" {
        return Ok(Some(value));
    }
    if !pointer.starts_with('/') {
        return Err(anyhow!(
            "JSON pointer must be empty, $, or start with /: {pointer}"
        ));
    }
    let mut current = value;
    for raw_token in pointer[1..].split('/') {
        let token = decode_json_pointer_token(raw_token)?;
        match current {
            Value::Object(map) => {
                let Some(next) = map.get(&token) else {
                    return Ok(None);
                };
                current = next;
            }
            Value::Array(values) => {
                let Ok(index) = token.parse::<usize>() else {
                    return Ok(None);
                };
                let Some(next) = values.get(index) else {
                    return Ok(None);
                };
                current = next;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(current))
}

fn decode_json_pointer_token(token: &str) -> Result<String> {
    let mut output = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '~' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('0') => output.push('~'),
            Some('1') => output.push('/'),
            Some(other) => {
                return Err(anyhow!(
                    "invalid JSON pointer escape: ~{other}; expected ~0 or ~1"
                ));
            }
            None => {
                return Err(anyhow!(
                    "invalid JSON pointer escape at end of token; expected ~0 or ~1"
                ));
            }
        }
    }
    Ok(output)
}

fn walk_json<'a>(
    path: &str,
    key: Option<&'a str>,
    value: &'a Value,
    visit: &mut impl FnMut(&str, Option<&'a str>, &'a Value),
) {
    visit(path, key, value);
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                let child_path = if is_json_identifier(child_key) {
                    format!("{path}.{child_key}")
                } else {
                    format!(
                        "{path}[{}]",
                        serde_json::to_string(child_key).unwrap_or_default()
                    )
                };
                walk_json(&child_path, Some(child_key.as_str()), child, visit);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                walk_json(&child_path, None, child, visit);
            }
        }
        _ => {}
    }
}

pub(crate) fn contains_any(value: &str, needles: &[String]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn value_summary(value: &Value, max_chars: usize) -> ClampedText {
    let full = value_summary_full(value);
    clamp_text_with_status(&full, max_chars)
}

fn value_summary_full(value: &Value) -> String {
    match value {
        Value::String(value) => format!("{value:?}"),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(values) => {
            if is_small_json(value) {
                serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned())
            } else {
                format!("<array:{} items>", values.len())
            }
        }
        Value::Object(map) => {
            if is_small_json(value) {
                serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned())
            } else {
                let sample_keys = map.keys().take(5).cloned().collect::<Vec<_>>();
                format!(
                    "<object:{} keys sample={}>",
                    map.len(),
                    serde_json::to_string(&sample_keys).unwrap_or_else(|_| "[]".to_owned())
                )
            }
        }
    }
}

fn json_search_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value_summary_full(value),
    }
}

fn is_small_json(value: &Value) -> bool {
    let mut nodes = 0usize;
    let mut string_chars = 0usize;
    json_fits_budget(value, &mut nodes, &mut string_chars)
}

fn json_fits_budget(value: &Value, nodes: &mut usize, string_chars: &mut usize) -> bool {
    *nodes += 1;
    if *nodes > JSON_SMALL_NODE_LIMIT {
        return false;
    }
    match value {
        Value::String(value) => {
            *string_chars += value.chars().count();
            *string_chars <= JSON_SMALL_STRING_CHAR_LIMIT
        }
        Value::Array(values) => values
            .iter()
            .all(|value| json_fits_budget(value, nodes, string_chars)),
        Value::Object(map) => map
            .values()
            .all(|value| json_fits_budget(value, nodes, string_chars)),
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
    }
}

fn is_json_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests;
