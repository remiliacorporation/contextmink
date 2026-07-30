use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

use crate::encoding::read_required_text;

pub(crate) const DEFAULT_MAX_JSON_DOCUMENT_BYTES: u64 = 67_108_864;

pub(crate) fn read_bounded_json_text(
    path: &Path,
    max_document_bytes: u64,
) -> Result<(String, &'static str)> {
    require_materialization_budget(path, max_document_bytes)?;
    read_required_text(path).with_context(|| format!("failed to read {}", path.display()))
}

pub(crate) fn parse_jsonl_text(path: &Path, text: &str) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row = parse_json_bytes(path, trimmed.as_bytes()).with_context(|| {
            format!(
                "failed to parse JSONL line {} in {}; every non-empty physical line must contain exactly one JSON value",
                line_index + 1,
                path.display()
            )
        })?;
        rows.push(row);
    }
    Ok(rows)
}

pub(crate) fn visit_jsonl_file(
    path: &Path,
    max_record_bytes: u64,
    mut visit: impl FnMut(usize, Value) -> Result<()>,
) -> Result<&'static str> {
    if max_record_bytes == 0 {
        return Err(anyhow!("--max-document-bytes must be greater than zero"));
    }
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let prefix = reader
        .fill_buf()
        .with_context(|| format!("failed to inspect encoding for {}", path.display()))?;
    if prefix.starts_with(&[0xFF, 0xFE])
        || prefix.starts_with(&[0xFE, 0xFF])
        || prefix.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
    {
        require_materialization_budget(path, max_record_bytes)?;
        drop(reader);
        let (text, encoding) = read_required_text(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        for (index, row) in parse_jsonl_text(path, &text)?.into_iter().enumerate() {
            visit(index, row)?;
        }
        return Ok(encoding);
    }
    if prefix.starts_with(&[0xEF, 0xBB, 0xBF]) {
        reader.consume(3);
    }
    let mut line = Vec::new();
    let mut row_index = 0usize;
    let mut physical_line = 0usize;
    loop {
        physical_line += 1;
        if !read_bounded_physical_line(
            &mut reader,
            &mut line,
            path,
            physical_line,
            max_record_bytes,
        )? {
            break;
        }
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        let row = parse_json_bytes(path, &line).with_context(|| {
            format!(
                "failed to parse JSONL line {physical_line} in {}; every non-empty physical line must contain exactly one JSON value",
                path.display()
            )
        })?;
        visit(row_index, row)?;
        row_index += 1;
    }
    Ok("utf8")
}

fn read_bounded_physical_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    path: &Path,
    physical_line: usize,
    max_record_bytes: u64,
) -> Result<bool> {
    line.clear();
    loop {
        let available = reader
            .fill_buf()
            .with_context(|| format!("failed to read {}", path.display()))?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let payload_len = newline.unwrap_or(available.len());
        if line.len() as u64 + payload_len as u64 > max_record_bytes {
            return Err(anyhow!(
                "JSONL line {physical_line} in {} exceeds --max-document-bytes {max_record_bytes}; raise the bound only if this is an intentional single-record payload",
                path.display()
            ));
        }
        line.extend_from_slice(&available[..payload_len]);
        let consumed = payload_len + usize::from(newline.is_some());
        reader.consume(consumed);
        if newline.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(true);
        }
    }
}

pub(crate) fn parse_json_text(path: &Path, text: &str) -> Result<Value> {
    parse_json_bytes(path, text.as_bytes())
}

fn parse_json_bytes(path: &Path, bytes: &[u8]) -> Result<Value> {
    let mut scanner = JsonKeyScanner {
        bytes,
        cursor: 0,
        path,
    };
    scanner.scan_value(0)?;
    scanner.skip_whitespace();
    if scanner.cursor != bytes.len() {
        return Err(anyhow!(
            "trailing JSON content in {} at byte {}",
            path.display(),
            scanner.cursor
        ));
    }
    serde_json::from_slice(bytes)
        .with_context(|| format!("failed to parse JSON value in {}", path.display()))
}

struct JsonKeyScanner<'a> {
    bytes: &'a [u8],
    cursor: usize,
    path: &'a Path,
}

impl JsonKeyScanner<'_> {
    fn scan_value(&mut self, depth: usize) -> Result<()> {
        if depth > 256 {
            return Err(anyhow!(
                "JSON nesting exceeds 256 levels in {}",
                self.path.display()
            ));
        }
        self.skip_whitespace();
        match self.bytes.get(self.cursor) {
            Some(b'{') => self.scan_object(depth + 1),
            Some(b'[') => self.scan_array(depth + 1),
            Some(b'"') => {
                self.scan_string()?;
                Ok(())
            }
            Some(_) => {
                while self.bytes.get(self.cursor).is_some_and(|byte| {
                    !matches!(byte, b',' | b']' | b'}' | b' ' | b'\t' | b'\r' | b'\n')
                }) {
                    self.cursor += 1;
                }
                Ok(())
            }
            None => Err(anyhow!("unexpected end of JSON in {}", self.path.display())),
        }
    }

    fn scan_object(&mut self, depth: usize) -> Result<()> {
        self.cursor += 1;
        let mut keys = HashSet::new();
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.cursor) == Some(&b'}') {
                self.cursor += 1;
                return Ok(());
            }
            let key_bytes = self.scan_string()?;
            let key: String = serde_json::from_slice(key_bytes)
                .with_context(|| format!("invalid object key in {}", self.path.display()))?;
            if !keys.insert(key.clone()) {
                return Err(anyhow!(
                    "duplicate JSON object key {key:?} in {}",
                    self.path.display()
                ));
            }
            self.skip_whitespace();
            if self.bytes.get(self.cursor) != Some(&b':') {
                return Err(anyhow!(
                    "expected ':' after object key in {} at byte {}",
                    self.path.display(),
                    self.cursor
                ));
            }
            self.cursor += 1;
            self.scan_value(depth)?;
            self.skip_whitespace();
            match self.bytes.get(self.cursor) {
                Some(b',') => self.cursor += 1,
                Some(b'}') => {
                    self.cursor += 1;
                    return Ok(());
                }
                _ => {
                    return Err(anyhow!(
                        "expected ',' or '}}' in {} at byte {}",
                        self.path.display(),
                        self.cursor
                    ));
                }
            }
        }
    }

    fn scan_array(&mut self, depth: usize) -> Result<()> {
        self.cursor += 1;
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.cursor) == Some(&b']') {
                self.cursor += 1;
                return Ok(());
            }
            self.scan_value(depth)?;
            self.skip_whitespace();
            match self.bytes.get(self.cursor) {
                Some(b',') => self.cursor += 1,
                Some(b']') => {
                    self.cursor += 1;
                    return Ok(());
                }
                _ => {
                    return Err(anyhow!(
                        "expected ',' or ']' in {} at byte {}",
                        self.path.display(),
                        self.cursor
                    ));
                }
            }
        }
    }

    fn scan_string(&mut self) -> Result<&[u8]> {
        let start = self.cursor;
        if self.bytes.get(self.cursor) != Some(&b'"') {
            return Err(anyhow!(
                "expected JSON string in {} at byte {}",
                self.path.display(),
                self.cursor
            ));
        }
        self.cursor += 1;
        let mut escaped = false;
        while let Some(byte) = self.bytes.get(self.cursor).copied() {
            self.cursor += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return Ok(&self.bytes[start..self.cursor]);
            }
        }
        Err(anyhow!(
            "unterminated JSON string in {}",
            self.path.display()
        ))
    }

    fn skip_whitespace(&mut self) {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.cursor += 1;
        }
    }
}

fn require_materialization_budget(path: &Path, max_document_bytes: u64) -> Result<()> {
    if max_document_bytes == 0 {
        return Err(anyhow!("--max-document-bytes must be greater than zero"));
    }
    let bytes = path
        .metadata()
        .with_context(|| format!("failed to stat {}", path.display()))?
        .len();
    if bytes > max_document_bytes {
        return Err(anyhow!(
            "{} is {bytes} bytes, above --max-document-bytes {max_document_bytes}; use JSONL for streaming or raise the explicit materialization bound",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn rejects_duplicate_object_keys_at_every_depth() {
        let path = Path::new("fixture.json");
        let error = parse_json_text(path, r#"{"outer":{"id":1,"id":2}}"#).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("duplicate JSON object key \"id\"")
        );
    }

    #[test]
    fn preserves_integers_larger_than_u64() {
        let value = parse_json_text(Path::new("fixture.json"), "184467440737095516160").unwrap();
        assert_eq!(value.to_string(), "184467440737095516160");
    }

    #[test]
    fn jsonl_requires_one_value_per_physical_line() {
        let error = parse_jsonl_text(
            Path::new("fixture.jsonl"),
            "{\n  \"id\": 1\n}\n{\"id\":2}\n",
        )
        .unwrap_err();
        assert!(error.to_string().contains("physical line"));
    }

    #[test]
    fn physical_line_reader_stops_at_the_record_bound() {
        let mut reader = BufReader::with_capacity(3, Cursor::new(b"123456789\n"));
        let mut line = Vec::new();
        let error =
            read_bounded_physical_line(&mut reader, &mut line, Path::new("fixture.jsonl"), 1, 5)
                .unwrap_err();
        assert!(error.to_string().contains("--max-document-bytes 5"));
        assert!(line.len() <= 5);
    }
}
