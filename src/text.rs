use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use regex::Regex;

#[derive(Clone)]
pub(crate) enum TextMatcher {
    Literal(String),
    Regex(Regex),
    Terms { terms: Vec<String>, mode: TermMode },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum TermMode {
    All,
    Any,
}

impl TextMatcher {
    pub(crate) fn new(pattern: &str, literal: bool) -> Result<Self> {
        if literal {
            Ok(Self::Literal(pattern.to_owned()))
        } else {
            Ok(Self::Regex(Regex::new(pattern).with_context(|| {
                format!("invalid regex pattern: {pattern}")
            })?))
        }
    }

    pub(crate) fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Literal(pattern) => text.contains(pattern),
            Self::Regex(pattern) => pattern.is_match(text),
            Self::Terms { terms, mode } => match mode {
                TermMode::All => terms.iter().all(|term| text.contains(term)),
                TermMode::Any => terms.iter().any(|term| text.contains(term)),
            },
        }
    }

    pub(crate) fn count_matches(&self, text: &str) -> usize {
        match self {
            Self::Literal(pattern) => {
                if pattern.is_empty() {
                    0
                } else {
                    text.matches(pattern).count()
                }
            }
            Self::Regex(pattern) => pattern.find_iter(text).count(),
            Self::Terms { .. } => text.lines().filter(|line| self.is_match(line)).count(),
        }
    }

    pub(crate) fn label(&self) -> String {
        match self {
            Self::Literal(pattern) => format!("{pattern:?}"),
            Self::Regex(pattern) => format!("{:?}", pattern.as_str()),
            Self::Terms { terms, mode } => match mode {
                TermMode::All => format!("all_terms({})", terms.join(",")),
                TermMode::Any => format!("any_terms({})", terms.join(",")),
            },
        }
    }
}

pub(crate) fn collect_terms(terms: &[String], term_files: &[PathBuf]) -> Result<Vec<String>> {
    let mut collected = terms.to_vec();
    for file in term_files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read term file {}", file.display()))?;
        let text = strip_utf8_bom(&text);
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if !line.is_empty() {
                collected.push(line.to_owned());
            }
        }
    }
    if collected.is_empty() {
        return Err(anyhow!(
            "grep-terms requires at least one --term or --term-file entry"
        ));
    }
    Ok(collected)
}

pub(crate) fn resolve_term_mode(mode: TermMode, any: bool, all: bool) -> Result<TermMode> {
    match (any, all) {
        (true, true) => Err(anyhow!(
            "grep-terms accepts only one of --any/--or or --all/--and"
        )),
        (true, false) => Ok(TermMode::Any),
        (false, true) => Ok(TermMode::All),
        (false, false) => Ok(mode),
    }
}

pub(crate) fn collect_single_text_source(
    label: &str,
    inline: Option<&str>,
    file: Option<&Path>,
    trim_terminal_newlines: bool,
) -> Result<String> {
    match (inline, file) {
        (Some(_), Some(_)) => Err(anyhow!(
            "{label} accepts either an inline value or a file, not both"
        )),
        (Some(value), None) => Ok(value.to_owned()),
        (None, Some(path)) => {
            let mut text = if path == Path::new("-") {
                let mut text = String::new();
                io::stdin()
                    .read_to_string(&mut text)
                    .with_context(|| format!("failed to read {label} from stdin"))?;
                text
            } else {
                fs::read_to_string(path)
                    .with_context(|| format!("failed to read {label} file {}", path.display()))?
            };
            if trim_terminal_newlines {
                trim_trailing_line_endings(&mut text);
            }
            Ok(strip_utf8_bom(&text).to_owned())
        }
        (None, None) => Err(anyhow!("{label} requires an inline value or file")),
    }
}

fn strip_utf8_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

fn trim_trailing_line_endings(value: &mut String) {
    while value.ends_with('\n') || value.ends_with('\r') {
        value.pop();
    }
}

pub(crate) fn parse_line_range(range: &str) -> Result<(usize, Option<usize>)> {
    let (start, end) = range
        .split_once(':')
        .ok_or_else(|| anyhow!("slice --range must use START:END line numbers"))?;
    if start.is_empty() || end.is_empty() {
        return Err(anyhow!("slice --range requires both START and END"));
    }
    let start = start
        .parse::<usize>()
        .with_context(|| format!("invalid slice --range start: {start}"))?;
    let end = end
        .parse::<usize>()
        .with_context(|| format!("invalid slice --range end: {end}"))?;
    if start == 0 || end == 0 {
        return Err(anyhow!("slice --range is 1-based; line 0 is invalid"));
    }
    if end < start {
        return Err(anyhow!(
            "slice --range end must be greater than or equal to start"
        ));
    }
    Ok((start, Some(end)))
}

#[cfg(test)]
mod tests;
