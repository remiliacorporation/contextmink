use std::io::{self, Write};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::Cli;

const RECEIPT_PREFIX: &str = "CONTEXTMINK_RECEIPT ";
const RECEIPT_SCHEMA: &str = "contextmink.receipt.v2";

static COMMAND_START: OnceLock<Instant> = OnceLock::new();

/// Record process start so every receipt can carry `duration_ms`; agents use
/// it to judge whether a query is cheap enough to rerun with narrower scope.
pub(crate) fn mark_command_start() {
    let _ = COMMAND_START.set(Instant::now()); // guardrail: allow-ignore-result second initialization in tests is harmless
}

fn elapsed_ms() -> u64 {
    COMMAND_START.get().map_or(0, |start| {
        start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    })
}

pub(crate) fn clamp_text(value: &str, max_chars: usize) -> String {
    clamp_text_with_status(value, max_chars).text
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClampedText {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

pub(crate) fn clamp_text_with_status(value: &str, max_chars: usize) -> ClampedText {
    let value_chars = value.chars().count();
    if value_chars <= max_chars {
        return ClampedText {
            text: value.to_owned(),
            truncated: false,
        };
    }
    let output = if max_chars <= 3 {
        ".".repeat(max_chars)
    } else {
        let mut output = value.chars().take(max_chars - 3).collect::<String>();
        output.push_str("...");
        output
    };
    ClampedText {
        text: output,
        truncated: true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceiptCapBoundary {
    Scope,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ReceiptCap {
    boundary: ReceiptCapBoundary,
    dimension: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
}

impl ReceiptCap {
    pub(crate) fn scope(dimension: &'static str, limit: Option<usize>) -> Self {
        Self {
            boundary: ReceiptCapBoundary::Scope,
            dimension,
            limit: limit.map(|value| {
                u64::try_from(value).expect("usize fits into u64 on supported targets")
            }),
        }
    }

    pub(crate) fn scope_bytes(dimension: &'static str, limit: u64) -> Self {
        Self {
            boundary: ReceiptCapBoundary::Scope,
            dimension,
            limit: Some(limit),
        }
    }

    pub(crate) fn output(dimension: &'static str, limit: Option<usize>) -> Self {
        Self {
            boundary: ReceiptCapBoundary::Output,
            dimension,
            limit: limit.map(|value| {
                u64::try_from(value).expect("usize fits into u64 on supported targets")
            }),
        }
    }
}

/// Shared renderer for command payload strings. Commands render JSON and text
/// from the same clamped values, then project this telemetry into the receipt.
pub(crate) struct TextClamp {
    max_chars: usize,
    truncated: bool,
}

impl TextClamp {
    pub(crate) fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            truncated: false,
        }
    }

    pub(crate) fn clamp(&mut self, value: &str) -> String {
        let clamped = clamp_text_with_status(value, self.max_chars);
        self.truncated |= clamped.truncated;
        clamped.text
    }

    pub(crate) fn record_truncated(&mut self, truncated: bool) {
        self.truncated |= truncated;
    }

    pub(crate) fn was_truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn add_receipt_cap(&self, receipt: &mut Receipt) {
        if self.truncated {
            receipt.add_cap(ReceiptCap::output("line_characters", Some(self.max_chars)));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ReceiptResult {
    unit: &'static str,
    total: usize,
    total_is_lower_bound: bool,
    shown: usize,
}

impl ReceiptResult {
    pub(crate) fn new(
        unit: &'static str,
        total: usize,
        total_is_lower_bound: bool,
        shown: usize,
    ) -> Self {
        assert!(
            shown <= total,
            "receipt result shown count cannot exceed its total"
        );
        Self {
            unit,
            total,
            total_is_lower_bound,
            shown,
        }
    }
}

/// Typed authority for the v2 receipt envelope. Completion is derived from
/// structured cap boundaries so serialized fields and strict-mode behavior
/// cannot disagree.
#[derive(Debug, Clone)]
pub(crate) struct Receipt {
    command: String,
    profile: Option<String>,
    result: ReceiptResult,
    caps: Vec<ReceiptCap>,
    fields: serde_json::Map<String, Value>,
}

impl Receipt {
    pub(crate) fn new(
        command: impl Into<String>,
        profile: Option<&str>,
        result: ReceiptResult,
    ) -> Self {
        Self {
            command: command.into(),
            profile: profile.map(str::to_owned),
            result,
            caps: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }

    pub(crate) fn add_cap(&mut self, cap: ReceiptCap) {
        self.caps.push(cap);
    }

    pub(crate) fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        assert!(
            !matches!(
                key.as_str(),
                "schema"
                    | "tool"
                    | "command"
                    | "profile"
                    | "duration_ms"
                    | "scope_complete"
                    | "output_truncated"
                    | "complete"
                    | "caps"
                    | "result"
            ),
            "receipt extension field collides with the typed envelope: {key}"
        );
        assert!(
            self.fields.insert(key.clone(), value).is_none(),
            "duplicate receipt extension field: {key}"
        );
    }

    pub(crate) fn scope_complete(&self) -> bool {
        !self
            .caps
            .iter()
            .any(|cap| cap.boundary == ReceiptCapBoundary::Scope)
    }

    pub(crate) fn output_truncated(&self) -> bool {
        self.caps
            .iter()
            .any(|cap| cap.boundary == ReceiptCapBoundary::Output)
    }

    pub(crate) fn complete(&self) -> bool {
        self.scope_complete() && !self.output_truncated()
    }

    pub(crate) fn into_value(self) -> Value {
        let scope_complete = self.scope_complete();
        let output_truncated = self.output_truncated();
        let mut map = serde_json::Map::new();
        map.insert("schema".to_string(), json!(RECEIPT_SCHEMA));
        map.insert("tool".to_string(), json!("contextmink"));
        map.insert("command".to_string(), json!(self.command));
        map.insert("profile".to_string(), json!(self.profile));
        map.insert("duration_ms".to_string(), json!(elapsed_ms()));
        map.insert("scope_complete".to_string(), json!(scope_complete));
        map.insert("output_truncated".to_string(), json!(output_truncated));
        map.insert(
            "complete".to_string(),
            json!(scope_complete && !output_truncated),
        );
        map.insert("caps".to_string(), json!(self.caps));
        map.insert("result".to_string(), json!(self.result));
        for (key, value) in self.fields {
            map.insert(key, value);
        }
        Value::Object(map)
    }
}

pub(crate) fn emit_json(value: Value) -> Result<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer_pretty(&mut stdout, &value)?;
    writeln!(stdout)?;
    Ok(())
}

pub(crate) fn emit_json_checked(cli: &Cli, receipt: Receipt) -> Result<()> {
    let scope_complete = receipt.scope_complete();
    let output_truncated = receipt.output_truncated();
    emit_json(receipt.into_value())?;
    fail_after_receipt(cli, scope_complete, output_truncated)
}

pub(crate) fn write_receipt(receipt: Receipt) -> Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{RECEIPT_PREFIX}{}", receipt.into_value())?;
    Ok(())
}

pub(crate) fn write_receipt_checked(cli: &Cli, receipt: Receipt) -> Result<()> {
    let scope_complete = receipt.scope_complete();
    let output_truncated = receipt.output_truncated();
    write_receipt(receipt)?;
    fail_after_receipt(cli, scope_complete, output_truncated)
}

pub(crate) fn fail_after_receipt(
    cli: &Cli,
    scope_complete: bool,
    output_truncated: bool,
) -> Result<()> {
    if cli.require_complete_scope && !scope_complete {
        return Err(anyhow!(
            "contextmink scope was incomplete (--require-complete-scope)"
        ));
    }
    if cli.fail_if_truncated && output_truncated {
        return Err(anyhow!(
            "contextmink output was truncated (strict completion requested)"
        ));
    }
    Ok(())
}

pub(crate) fn no_match_scope(no_matches: bool, scope_complete: bool) -> Option<&'static str> {
    if !no_matches {
        None
    } else if scope_complete {
        Some("complete_scope")
    } else {
        Some("scanned_subset")
    }
}

#[cfg(test)]
mod tests;
