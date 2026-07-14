//! Shell-syntax layer of the destructive-command guard: tokenize a shell
//! payload into candidate commands, strip constructs whose bodies are data
//! (quotes, comments, heredocs, PowerShell here-strings), surface command
//! substitutions for recursive inspection, and expand literal brace
//! alternatives. Policy evaluation lives in the parent module.

use super::ShellDialect;

#[derive(Debug, Default)]
pub(super) struct ParsedShellPayload {
    pub(super) commands: Vec<Vec<String>>,
    pub(super) substitutions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellQuote {
    Unquoted,
    Single,
    Double,
}

/// Parse enough shell structure to identify executable commands without
/// treating quoted arguments, comments, or heredoc bodies as commands. The
/// guard is not a shell interpreter: expansions remain opaque except for
/// command substitutions, which are recursively inspected.
pub(super) fn parse_shell_payload(text: &str, dialect: ShellDialect) -> ParsedShellPayload {
    let continued = (dialect == ShellDialect::Posix).then(|| remove_posix_line_continuations(text));
    let text = continued.as_deref().unwrap_or(text);
    let here_strings =
        (dialect == ShellDialect::Powershell).then(|| strip_powershell_here_strings(text));
    let text = here_strings.as_deref().unwrap_or(text);
    let heredocs = (dialect == ShellDialect::Posix).then(|| strip_heredoc_bodies(text));
    let text = heredocs.as_deref().unwrap_or(text);
    let chars = text.chars().collect::<Vec<_>>();
    let mut parsed = ParsedShellPayload::default();
    let mut command = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut quote = ShellQuote::Unquoted;
    let mut index = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        match quote {
            ShellQuote::Single => {
                if ch == '\'' {
                    quote = ShellQuote::Unquoted;
                } else {
                    word.push(ch);
                }
                word_started = true;
                index += 1;
            }
            ShellQuote::Double => {
                if ch == '"' {
                    quote = ShellQuote::Unquoted;
                    word_started = true;
                    index += 1;
                } else if (ch == '\\' || dialect == ShellDialect::Powershell && ch == '`')
                    && index + 1 < chars.len()
                {
                    word.push(chars[index + 1]);
                    word_started = true;
                    index += 2;
                } else if ch == '$' && chars.get(index + 1) == Some(&'(') {
                    if let Some((substitution, next)) = extract_parenthesized(&chars, index + 2) {
                        parsed.substitutions.push(substitution);
                        word.push_str("$()");
                        word_started = true;
                        index = next;
                    } else {
                        word.push(ch);
                        word_started = true;
                        index += 1;
                    }
                } else if dialect == ShellDialect::Posix && ch == '`' {
                    if let Some((substitution, next)) = extract_backticks(&chars, index + 1) {
                        parsed.substitutions.push(substitution);
                        word.push_str("``");
                        word_started = true;
                        index = next;
                    } else {
                        word.push(ch);
                        word_started = true;
                        index += 1;
                    }
                } else {
                    word.push(ch);
                    word_started = true;
                    index += 1;
                }
            }
            ShellQuote::Unquoted => match ch {
                '\'' => {
                    quote = ShellQuote::Single;
                    word_started = true;
                    index += 1;
                }
                '"' => {
                    quote = ShellQuote::Double;
                    word_started = true;
                    index += 1;
                }
                '`' if dialect == ShellDialect::Powershell && index + 1 < chars.len() => {
                    word.push(chars[index + 1]);
                    word_started = true;
                    index += 2;
                }
                '^' if dialect == ShellDialect::Cmd && index + 1 < chars.len() => {
                    word.push(chars[index + 1]);
                    word_started = true;
                    index += 2;
                }
                '\\' if dialect != ShellDialect::Cmd && index + 1 < chars.len() => {
                    word.push(chars[index + 1]);
                    word_started = true;
                    index += 2;
                }
                '$' if chars.get(index + 1) == Some(&'(') => {
                    if let Some((substitution, next)) = extract_parenthesized(&chars, index + 2) {
                        parsed.substitutions.push(substitution);
                        word.push_str("$()");
                        word_started = true;
                        index = next;
                    } else {
                        word.push(ch);
                        word_started = true;
                        index += 1;
                    }
                }
                '`' if dialect == ShellDialect::Posix => {
                    if let Some((substitution, next)) = extract_backticks(&chars, index + 1) {
                        parsed.substitutions.push(substitution);
                        word.push_str("``");
                        word_started = true;
                        index = next;
                    } else {
                        word.push(ch);
                        word_started = true;
                        index += 1;
                    }
                }
                '{' | '}'
                    if shell_group_brace_is_boundary(&chars, index, word_started, dialect) =>
                {
                    flush_shell_word(&mut word, &mut word_started, &mut command);
                    flush_shell_command(&mut command, &mut parsed.commands);
                    index += 1;
                }
                '#' if !word_started => {
                    flush_shell_word(&mut word, &mut word_started, &mut command);
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                }
                '&' if chars.get(index + 1) == Some(&'>') => {
                    flush_shell_word(&mut word, &mut word_started, &mut command);
                    command.push("__contextmink_redirection__".to_owned());
                    index += 2;
                    while matches!(chars.get(index), Some('<' | '>')) {
                        index += 1;
                    }
                }
                ';' | '&' | '|' | '\n' | '(' | ')' => {
                    flush_shell_word(&mut word, &mut word_started, &mut command);
                    flush_shell_command(&mut command, &mut parsed.commands);
                    index += 1;
                    if matches!(ch, '&' | '|') && chars.get(index) == Some(&ch) {
                        index += 1;
                    }
                }
                '<' | '>' => {
                    if word_started && word.chars().all(|digit| digit.is_ascii_digit()) {
                        word.clear();
                        word_started = false;
                    } else {
                        flush_shell_word(&mut word, &mut word_started, &mut command);
                    }
                    command.push("__contextmink_redirection__".to_owned());
                    index += 1;
                    while matches!(chars.get(index), Some('<' | '>')) {
                        index += 1;
                    }
                    if matches!(chars.get(index), Some('&' | '|')) {
                        index += 1;
                    }
                }
                _ if ch.is_whitespace() => {
                    flush_shell_word(&mut word, &mut word_started, &mut command);
                    index += 1;
                }
                _ => {
                    word.push(ch);
                    word_started = true;
                    index += 1;
                }
            },
        }
    }
    flush_shell_word(&mut word, &mut word_started, &mut command);
    flush_shell_command(&mut command, &mut parsed.commands);
    for command in &mut parsed.commands {
        let mut normalized = Vec::with_capacity(command.len());
        let mut discard_target = false;
        for token in command.drain(..) {
            if token == "__contextmink_redirection__" {
                discard_target = true;
            } else if discard_target {
                discard_target = false;
            } else {
                normalized.push(token);
            }
        }
        *command = normalized;
    }
    parsed
}

fn shell_group_brace_is_boundary(
    chars: &[char],
    index: usize,
    word_started: bool,
    dialect: ShellDialect,
) -> bool {
    if dialect == ShellDialect::Cmd {
        return false;
    }
    if dialect == ShellDialect::Powershell {
        return true;
    }
    if word_started {
        return false;
    }
    chars.get(index + 1).is_none_or(|next| {
        next.is_whitespace() || matches!(next, ';' | '&' | '|' | '(' | ')' | '{' | '}')
    })
}

fn remove_posix_line_continuations(text: &str) -> String {
    text.replace("\\\r\n", "").replace("\\\n", "")
}

fn strip_powershell_here_strings(text: &str) -> String {
    let mut lines = text.split_inclusive('\n');
    let mut stripped = String::with_capacity(text.len());
    while let Some(line) = lines.next() {
        stripped.push_str(line);
        let trimmed = line.trim_end_matches(['\r', '\n']).trim_end();
        let terminator = powershell_here_string_terminator(trimmed);
        let Some(terminator) = terminator else {
            continue;
        };
        for body_line in lines.by_ref() {
            if body_line.trim_end_matches(['\r', '\n']) == terminator {
                stripped.push_str(body_line);
                break;
            }
        }
    }
    stripped
}

fn powershell_here_string_terminator(line: &str) -> Option<&'static str> {
    let mut quote = ShellQuote::Unquoted;
    let mut chars = line.char_indices().peekable();
    while let Some((_index, ch)) = chars.next() {
        match quote {
            ShellQuote::Unquoted => match ch {
                '#' => return None,
                '\'' => quote = ShellQuote::Single,
                '"' => quote = ShellQuote::Double,
                '@' => {
                    let Some(&(quote_index, quote_char @ ('\'' | '"'))) = chars.peek() else {
                        continue;
                    };
                    if line[quote_index + quote_char.len_utf8()..]
                        .trim()
                        .is_empty()
                    {
                        return Some(if quote_char == '\'' { "'@" } else { "\"@" });
                    }
                }
                '`' => {
                    chars.next();
                }
                _ => {}
            },
            ShellQuote::Single => {
                if ch == '\'' {
                    if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        chars.next();
                    } else {
                        quote = ShellQuote::Unquoted;
                    }
                }
            }
            ShellQuote::Double => match ch {
                '"' => quote = ShellQuote::Unquoted,
                '`' => {
                    chars.next();
                }
                _ => {}
            },
        }
    }
    None
}

const MAX_BRACE_EXPANSIONS: usize = 32;

pub(super) fn expand_literal_braces(tokens: &[String]) -> Result<Vec<Vec<String>>, ()> {
    let mut variants = vec![Vec::new()];
    for token in tokens {
        let alternatives = literal_brace_expansions(token)?;
        if variants
            .len()
            .checked_mul(alternatives.len())
            .is_none_or(|count| count > MAX_BRACE_EXPANSIONS)
        {
            return Err(());
        }
        variants = variants
            .into_iter()
            .flat_map(|prefix| {
                alternatives.iter().map(move |alternative| {
                    let mut variant = prefix.clone();
                    variant.push(alternative.clone());
                    variant
                })
            })
            .collect();
    }
    Ok(variants)
}

fn literal_brace_expansions(token: &str) -> Result<Vec<String>, ()> {
    let mut pending = vec![token.to_owned()];
    let mut expanded = Vec::new();
    while let Some(value) = pending.pop() {
        let Some((open, close, alternatives)) = literal_brace_alternatives(&value)? else {
            expanded.push(value);
            continue;
        };
        if expanded
            .len()
            .checked_add(pending.len())
            .and_then(|count| count.checked_add(alternatives.len()))
            .is_none_or(|count| count > MAX_BRACE_EXPANSIONS)
        {
            return Err(());
        }
        let prefix = &value[..open];
        let suffix = &value[close + 1..];
        for alternative in alternatives.into_iter().rev() {
            pending.push(format!("{prefix}{alternative}{suffix}"));
        }
    }
    Ok(expanded)
}

fn literal_brace_alternatives(token: &str) -> Result<Option<(usize, usize, Vec<String>)>, ()> {
    let mut opens = Vec::new();
    for (index, ch) in token.char_indices() {
        match ch {
            '{' => opens.push(index),
            '}' => {
                let Some(open) = opens.pop() else {
                    continue;
                };
                let body = &token[open + 1..index];
                if let Some(alternatives) = split_top_level_commas(body) {
                    return Ok(Some((open, index, alternatives)));
                }
                if let Some(alternatives) = brace_sequence_alternatives(body)? {
                    return Ok(Some((open, index, alternatives)));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

fn split_top_level_commas(body: &str) -> Option<Vec<String>> {
    let mut alternatives = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                alternatives.push(body[start..index].to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    if alternatives.is_empty() {
        return None;
    }
    alternatives.push(body[start..].to_owned());
    Some(alternatives)
}

fn brace_sequence_alternatives(body: &str) -> Result<Option<Vec<String>>, ()> {
    let parts = body.split("..").collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|part| part.is_empty()) {
        return Ok(None);
    }
    let step = if parts.len() == 3 {
        let Ok(step) = parts[2].parse::<i64>() else {
            return Ok(None);
        };
        step.unsigned_abs().max(1)
    } else {
        1
    };

    if let (Ok(start), Ok(end)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
        let width = [parts[0], parts[1]]
            .into_iter()
            .map(|value| value.trim_start_matches('-'))
            .filter(|digits| digits.len() > 1 && digits.starts_with('0'))
            .map(str::len)
            .max();
        return numeric_range_values(start, end, step, |value| match width {
            Some(width) if value < 0 => {
                format!("-{:0width$}", value.unsigned_abs(), width = width)
            }
            Some(width) => format!("{value:0width$}", width = width),
            None => value.to_string(),
        })
        .map(Some);
    }

    let mut start_chars = parts[0].chars();
    let mut end_chars = parts[1].chars();
    let (Some(start), None, Some(end), None) = (
        start_chars.next(),
        start_chars.next(),
        end_chars.next(),
        end_chars.next(),
    ) else {
        return Ok(None);
    };
    if !start.is_ascii() || !end.is_ascii() {
        return Ok(None);
    }
    character_range_values(start, end, step).map(Some)
}

fn numeric_range_values(
    start: i64,
    end: i64,
    step: u64,
    render: impl Fn(i64) -> String,
) -> Result<Vec<String>, ()> {
    let distance = start.abs_diff(end);
    let count = (distance / step).checked_add(1).ok_or(())?;
    if count > MAX_BRACE_EXPANSIONS as u64 {
        return Err(());
    }
    let ascending = start <= end;
    let mut output = Vec::with_capacity(count as usize);
    for offset in 0..count {
        let delta = i128::from(offset * step);
        let value = if ascending {
            i128::from(start) + delta
        } else {
            i128::from(start) - delta
        };
        let value = i64::try_from(value).map_err(|_| ())?;
        output.push(render(value));
    }
    Ok(output)
}

fn character_range_values(start: char, end: char, step: u64) -> Result<Vec<String>, ()> {
    let start = u32::from(start);
    let end = u32::from(end);
    let distance = u64::from(start.abs_diff(end));
    let count = (distance / step).checked_add(1).ok_or(())?;
    if count > MAX_BRACE_EXPANSIONS as u64 {
        return Err(());
    }
    let ascending = start <= end;
    let mut output = Vec::with_capacity(count as usize);
    for offset in 0..count {
        let delta = u32::try_from(offset * step).map_err(|_| ())?;
        let value = if ascending {
            start.checked_add(delta)
        } else {
            start.checked_sub(delta)
        }
        .and_then(char::from_u32)
        .ok_or(())?;
        output.push(value.to_string());
    }
    Ok(output)
}

fn flush_shell_word(word: &mut String, started: &mut bool, command: &mut Vec<String>) {
    if *started {
        command.push(std::mem::take(word));
        *started = false;
    }
}

fn flush_shell_command(command: &mut Vec<String>, commands: &mut Vec<Vec<String>>) {
    if !command.is_empty() {
        commands.push(std::mem::take(command));
    }
}

fn extract_parenthesized(chars: &[char], mut index: usize) -> Option<(String, usize)> {
    let mut depth = 1usize;
    let mut quote = ShellQuote::Unquoted;
    let mut value = String::new();
    while index < chars.len() {
        let ch = chars[index];
        match quote {
            ShellQuote::Single => {
                if ch == '\'' {
                    quote = ShellQuote::Unquoted;
                }
                value.push(ch);
            }
            ShellQuote::Double => {
                if ch == '"' {
                    quote = ShellQuote::Unquoted;
                }
                value.push(ch);
            }
            ShellQuote::Unquoted => match ch {
                '\'' => {
                    quote = ShellQuote::Single;
                    value.push(ch);
                }
                '"' => {
                    quote = ShellQuote::Double;
                    value.push(ch);
                }
                '(' => {
                    depth += 1;
                    value.push(ch);
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((value, index + 1));
                    }
                    value.push(ch);
                }
                _ => value.push(ch),
            },
        }
        index += 1;
    }
    None
}

fn extract_backticks(chars: &[char], mut index: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    while index < chars.len() {
        match chars[index] {
            '`' => return Some((value, index + 1)),
            '\\' if index + 1 < chars.len() => {
                value.push(chars[index + 1]);
                index += 2;
                continue;
            }
            ch => value.push(ch),
        }
        index += 1;
    }
    None
}

fn strip_heredoc_bodies(text: &str) -> String {
    let mut lines = text.split_inclusive('\n');
    let mut stripped = String::with_capacity(text.len());
    let mut quote = ShellQuote::Unquoted;
    while let Some(line) = lines.next() {
        stripped.push_str(line);
        for (delimiter, strip_tabs) in heredoc_delimiters(line, &mut quote) {
            for body_line in lines.by_ref() {
                let candidate = body_line.trim_end_matches(['\r', '\n']);
                let candidate = if strip_tabs {
                    candidate.trim_start_matches('\t')
                } else {
                    candidate
                };
                if candidate == delimiter {
                    break;
                }
            }
        }
    }
    stripped
}

fn heredoc_delimiters(line: &str, quote: &mut ShellQuote) -> Vec<(String, bool)> {
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut word_started = false;
    let mut delimiters = Vec::new();
    while index + 1 < chars.len() {
        match (*quote, chars[index]) {
            (ShellQuote::Single, '\'') => *quote = ShellQuote::Unquoted,
            (ShellQuote::Double, '"') => *quote = ShellQuote::Unquoted,
            (ShellQuote::Unquoted, '\'') => {
                *quote = ShellQuote::Single;
                word_started = true;
            }
            (ShellQuote::Unquoted, '"') => {
                *quote = ShellQuote::Double;
                word_started = true;
            }
            (ShellQuote::Unquoted, '#') if !word_started => break,
            (ShellQuote::Unquoted, '<') if chars[index + 1] == '<' => {
                if chars.get(index + 2) == Some(&'<') {
                    index += 3;
                    continue;
                }
                index += 2;
                let strip_tabs = chars.get(index) == Some(&'-');
                if strip_tabs {
                    index += 1;
                }
                while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
                    index += 1;
                }
                let mut delimiter = String::new();
                let mut delimiter_quote = ShellQuote::Unquoted;
                while let Some(ch) = chars.get(index).copied() {
                    match (delimiter_quote, ch) {
                        (ShellQuote::Unquoted, '\'') => delimiter_quote = ShellQuote::Single,
                        (ShellQuote::Unquoted, '"') => delimiter_quote = ShellQuote::Double,
                        (ShellQuote::Single, '\'') | (ShellQuote::Double, '"') => {
                            delimiter_quote = ShellQuote::Unquoted
                        }
                        (ShellQuote::Unquoted, ch)
                            if ch.is_whitespace() || matches!(ch, ';' | '&' | '|') =>
                        {
                            break;
                        }
                        (_, ch) => delimiter.push(ch),
                    }
                    index += 1;
                }
                if !delimiter.is_empty() {
                    delimiters.push((delimiter, strip_tabs));
                }
                word_started = false;
                continue;
            }
            (ShellQuote::Unquoted, ch) if ch.is_whitespace() => word_started = false,
            (ShellQuote::Unquoted, ';' | '&' | '|' | '(' | ')') => word_started = false,
            (ShellQuote::Unquoted, _) => word_started = true,
            _ => {}
        }
        index += 1;
    }
    delimiters
}
