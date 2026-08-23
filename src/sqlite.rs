use std::cmp::min;
use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::ptr;

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::encoding::read_required_text;
use crate::files::display_path;
use crate::json_commands::contains_any;
use crate::json_input::{parse_json_text, parse_jsonl_text};
use crate::output::{
    ClampedText, Receipt, ReceiptCap, ReceiptResult, clamp_text, clamp_text_with_status,
    emit_json_checked, write_receipt_checked,
};
use crate::text::collect_single_text_source;

#[derive(Debug)]
struct SqliteTableSummary {
    schema: String,
    name: String,
    kind: String,
    column_count_declared: i64,
    without_rowid: bool,
    strict: bool,
    columns: Vec<SqliteColumnSummary>,
    indexes: Vec<SqliteIndexSummary>,
    columns_total: usize,
    indexes_total: usize,
    detail_elided: bool,
}

#[derive(Debug)]
struct SqliteColumnSummary {
    name: String,
    type_name: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_rank: i64,
    hidden: i64,
    foreign_key: Option<SqliteForeignKeySummary>,
}

#[derive(Clone, Debug)]
struct SqliteForeignKeySummary {
    table: String,
    column: String,
}

#[derive(Debug)]
struct SqliteIndexSummary {
    name: String,
    unique: bool,
    origin: String,
    partial: bool,
    columns: Vec<String>,
}

#[derive(Debug)]
struct SqliteFileParam {
    sql_name: String,
    path: PathBuf,
    format: &'static str,
    value: String,
    /// Top-level value count when the bound document is an array (`json_each`
    /// row cardinality); None for a non-array JSON document.
    values: Option<usize>,
    source_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_sqlite(
    cli: &Cli,
    config: &ContextConfig,
    db: &Path,
    sql: Option<&str>,
    sql_file: Option<&Path>,
    json_params: &[String],
    jsonl_params: &[String],
    max_param_bytes: u64,
    max_rows: usize,
    max_rows_scanned: usize,
    timeout_secs: u64,
    max_value_chars: usize,
) -> Result<()> {
    if max_rows == 0 {
        return Err(anyhow!("sqlite --limit must be greater than zero"));
    }
    if max_rows_scanned == 0 {
        return Err(anyhow!(
            "sqlite --max-rows-scanned must be greater than zero"
        ));
    }
    if max_rows_scanned < max_rows {
        return Err(anyhow!(
            "sqlite --max-rows-scanned must be greater than or equal to --limit"
        ));
    }
    if max_param_bytes == 0 {
        return Err(anyhow!(
            "sqlite --max-param-bytes must be greater than zero"
        ));
    }
    if max_value_chars == 0 {
        return Err(anyhow!(
            "sqlite --max-value-chars must be greater than zero"
        ));
    }
    let sql = collect_single_text_source("sqlite SQL", sql, sql_file, false)?;
    if sql.trim().is_empty() {
        return Err(anyhow!("sqlite SQL must not be empty"));
    }
    let params = collect_sqlite_file_params(json_params, jsonl_params, max_param_bytes)?;
    let conn = open_sqlite_readonly(db)?;
    let _watchdog = QueryWatchdog::arm(&conn, timeout_secs);
    reject_multiple_sqlite_statements(&conn, &sql)?;
    let mut stmt = conn.prepare(&sql).context("failed to prepare sqlite SQL")?;
    if stmt.parameter_count() != 0 && params.is_empty() {
        return Err(anyhow!(
            "sqlite query contains parameters; bind named JSON inputs with --json-param or --jsonl-param"
        ));
    }
    if !stmt.readonly() {
        return Err(anyhow!("sqlite command only accepts read-only statements"));
    }
    bind_sqlite_file_params(&mut stmt, &params)?;
    let column_count = stmt.column_count();
    let columns = stmt
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut row_iter = stmt.raw_query();
    let mut rendered_rows = Vec::new();
    let mut json_rows = Vec::new();
    let mut total_seen = 0usize;
    let mut row_scope_capped = false;
    let mut value_characters_truncated = false;
    while let Some(row) = row_iter
        .next()
        .map_err(|error| annotate_interrupt(error, timeout_secs))?
    {
        total_seen += 1;
        if total_seen <= max_rows {
            let mut rendered = Vec::with_capacity(column_count);
            let mut fields = serde_json::Map::new();
            for (index, column) in columns.iter().enumerate() {
                let summary = sqlite_value_summary(row.get_ref(index)?, max_value_chars);
                value_characters_truncated |= summary.truncated;
                rendered.push((column.clone(), summary.text.clone()));
                fields.insert(column.clone(), json!(summary.text));
            }
            rendered_rows.push(rendered);
            json_rows.push(json!({
                "row": total_seen - 1,
                "fields": fields,
            }));
        }
        if total_seen > max_rows_scanned {
            row_scope_capped = true;
            break;
        }
    }
    let shown = rendered_rows.len();
    let mut receipt = Receipt::new(
        "sqlite",
        config.profile.as_deref(),
        ReceiptResult::new("rows", total_seen, row_scope_capped, shown),
    );
    if row_scope_capped {
        receipt.add_cap(ReceiptCap::scope("rows_processed", Some(max_rows_scanned)));
    }
    if shown < total_seen {
        receipt.add_cap(ReceiptCap::output("rows", Some(max_rows)));
    }
    if value_characters_truncated {
        receipt.add_cap(ReceiptCap::output(
            "value_characters",
            Some(max_value_chars),
        ));
    }
    receipt.insert("db", json!(display_path(db)));
    receipt.insert("columns", json!(columns));
    receipt.insert("params", sqlite_param_receipt_rows(&params));
    receipt.insert("rows_examined", json!(total_seen));
    if cli.json {
        receipt.insert("rows", json!(json_rows));
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        writeln!(
            stdout,
            "[contextmink] sqlite db={} columns={}",
            display_path(db),
            columns.join(",")
        )?;
        if rendered_rows.is_empty() {
            writeln!(stdout, "no_rows")?;
        }
        for (row_index, fields) in rendered_rows.iter().enumerate() {
            let rendered = fields
                .iter()
                .map(|(column, value)| format!("{column}={value}"))
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(stdout, "{row_index}: {rendered}")?;
        }
        if row_scope_capped {
            writeln!(
                stdout,
                "[contextmink] capped sqlite row processing at {max_rows_scanned} rows; add WHERE/LIMIT or narrow the query before treating this as complete."
            )?;
        } else if shown < total_seen {
            writeln!(
                stdout,
                "[contextmink] capped sqlite output at {max_rows} rows; increase --limit or narrow the query."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

fn reject_multiple_sqlite_statements(conn: &Connection, sql: &str) -> Result<()> {
    if sql.as_bytes().contains(&0) {
        return Err(anyhow!("sqlite SQL must not contain NUL bytes"));
    }
    let mut offset = 0usize;
    let mut statements = 0usize;
    while offset < sql.len() {
        let remaining = &sql.as_bytes()[offset..];
        let byte_count =
            i32::try_from(remaining.len()).context("sqlite SQL is too large to prepare safely")?;
        let mut raw_statement = ptr::null_mut();
        let mut tail = ptr::null();
        // SQLite owns SQL grammar and comment handling. Preparing each tail is
        // the only reliable way to distinguish executable statements from
        // semicolons inside strings or trailing whitespace/comments.
        let status = unsafe {
            rusqlite::ffi::sqlite3_prepare_v3(
                conn.handle(),
                remaining.as_ptr().cast(),
                byte_count,
                0,
                &mut raw_statement,
                &mut tail,
            )
        };
        if status != rusqlite::ffi::SQLITE_OK {
            // sqlite3_errmsg returns a connection-owned UTF-8 string that is
            // valid until the next SQLite call on this connection.
            let detail = unsafe {
                let raw = rusqlite::ffi::sqlite3_errmsg(conn.handle());
                if raw.is_null() {
                    String::from("no SQLite error message")
                } else {
                    std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
                }
            };
            let sqlite_error_offset = unsafe { rusqlite::ffi::sqlite3_error_offset(conn.handle()) };
            let location = if sqlite_error_offset >= 0 {
                format!("at byte {}", offset + sqlite_error_offset as usize)
            } else {
                format!("from byte {offset}")
            };
            if !raw_statement.is_null() {
                let _ = unsafe { rusqlite::ffi::sqlite3_finalize(raw_statement) }; // guardrail: allow-ignore-result validation-only finalize cannot recover meaningfully
            }
            return Err(anyhow!(
                "failed to validate sqlite SQL {location}: {detail} (SQLite status {status})"
            ));
        }
        if !raw_statement.is_null() {
            statements += 1;
            let _ = unsafe { rusqlite::ffi::sqlite3_finalize(raw_statement) }; // guardrail: allow-ignore-result validation-only finalize cannot recover meaningfully
        }
        let consumed = if tail.is_null() {
            remaining.len()
        } else {
            // `tail` points inside `remaining` by SQLite contract for a
            // successful prepare call using an explicit byte count.
            usize::try_from(unsafe { tail.offset_from(remaining.as_ptr().cast()) })
                .context("SQLite returned an invalid SQL tail pointer")?
        };
        if consumed == 0 {
            break;
        }
        offset = offset.saturating_add(consumed);
    }
    if statements != 1 {
        return Err(anyhow!(
            "sqlite accepts exactly one executable read-only statement; found {statements}"
        ));
    }
    Ok(())
}

fn collect_sqlite_file_params(
    json_params: &[String],
    jsonl_params: &[String],
    max_param_bytes: u64,
) -> Result<Vec<SqliteFileParam>> {
    let mut params = Vec::with_capacity(json_params.len() + jsonl_params.len());
    let mut names = BTreeSet::new();
    for raw in json_params {
        let (sql_name, path) = parse_sqlite_file_param(raw, "--json-param")?;
        if !names.insert(sql_name.clone()) {
            return Err(anyhow!("duplicate sqlite parameter binding {sql_name}"));
        }
        params.push(load_sqlite_json_param(
            sql_name,
            path,
            "json",
            max_param_bytes,
        )?);
    }
    for raw in jsonl_params {
        let (sql_name, path) = parse_sqlite_file_param(raw, "--jsonl-param")?;
        if !names.insert(sql_name.clone()) {
            return Err(anyhow!("duplicate sqlite parameter binding {sql_name}"));
        }
        params.push(load_sqlite_json_param(
            sql_name,
            path,
            "jsonl",
            max_param_bytes,
        )?);
    }
    Ok(params)
}

fn parse_sqlite_file_param(raw: &str, flag: &str) -> Result<(String, PathBuf)> {
    let (name, path) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("{flag} requires NAME=FILE, found {raw:?}"))?;
    let sql_name = normalize_sqlite_param_name(name, flag)?;
    let path = path.trim();
    if path.is_empty() {
        return Err(anyhow!("{flag} requires a non-empty FILE path: {raw:?}"));
    }
    Ok((sql_name, PathBuf::from(path)))
}

fn normalize_sqlite_param_name(name: &str, flag: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("{flag} requires a non-empty parameter name"));
    }
    let mut chars = name.chars();
    let first = chars.next().expect("empty name checked above");
    let (prefix, body) = if matches!(first, ':' | '@' | '$') {
        (first, chars.as_str())
    } else {
        (':', name)
    };
    if body.is_empty() {
        return Err(anyhow!("{flag} requires a name after {prefix:?}"));
    }
    if body
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
    {
        return Err(anyhow!(
            "{flag} parameter name {name:?} may contain only ASCII letters, digits, and underscores"
        ));
    }
    Ok(format!("{prefix}{body}"))
}

fn load_sqlite_json_param(
    sql_name: String,
    path: PathBuf,
    format: &'static str,
    max_param_bytes: u64,
) -> Result<SqliteFileParam> {
    let metadata =
        std::fs::metadata(&path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.len() > max_param_bytes {
        return Err(anyhow!(
            "{} is {} bytes, larger than sqlite --max-param-bytes {}",
            path.display(),
            metadata.len(),
            max_param_bytes
        ));
    }
    let (text, _) = read_required_text(&path)
        .with_context(|| format!("failed to read sqlite parameter {}", path.display()))?;
    let (value, values) = match format {
        "json" => {
            let value = parse_json_text(&path, &text)
                .map_err(|error| json_param_parse_error(&path, &text, error))?;
            let values = value.as_array().map(Vec::len);
            (
                serde_json::to_string(&value).context("failed to serialize JSON parameter")?,
                values,
            )
        }
        "jsonl" => jsonl_to_json_array_text(&path, &text)?,
        _ => unreachable!("sqlite param formats are fixed by callers"),
    };
    Ok(SqliteFileParam {
        sql_name,
        path,
        format,
        value,
        values,
        source_bytes: metadata.len(),
    })
}

/// A `--json-param` file that fails as a single JSON document but parses as
/// multiple JSONL values is almost certainly a JSONL worklist bound with the
/// wrong flag; teach the fix instead of surfacing a bare serde error.
fn json_param_parse_error(path: &Path, text: &str, error: anyhow::Error) -> anyhow::Error {
    let jsonl_values = parse_jsonl_text(path, text).map_or(0, |rows| rows.len());
    if jsonl_values > 1 {
        return anyhow!(
            "{} is not a single JSON document but parses as {} JSONL values; bind it with --jsonl-param instead",
            path.display(),
            jsonl_values
        );
    }
    error.context(format!("failed to parse JSON parameter {}", path.display()))
}

fn jsonl_to_json_array_text(path: &Path, text: &str) -> Result<(String, Option<usize>)> {
    let rows = parse_jsonl_text(path, text)?;
    // A lone top-level array is a plain JSON array file: wrapping it would
    // bind [[...]] and json_each would silently see one row instead of N.
    if let [Value::Array(inner)] = rows.as_slice() {
        return Err(anyhow!(
            "{} holds a single top-level JSON array ({} elements); binding it as JSONL would wrap it to one json_each row — use --json-param instead",
            path.display(),
            inner.len()
        ));
    }
    let values = Some(rows.len());
    Ok((
        serde_json::to_string(&rows)
            .context("failed to serialize JSONL parameter as JSON array")?,
        values,
    ))
}

fn bind_sqlite_file_params(
    stmt: &mut rusqlite::Statement<'_>,
    params: &[SqliteFileParam],
) -> Result<()> {
    let mut bound_indexes = BTreeSet::new();
    for param in params {
        let index = stmt
            .parameter_index(&param.sql_name)
            .with_context(|| format!("failed to inspect sqlite parameter {}", param.sql_name))?
            .ok_or_else(|| {
                anyhow!(
                    "sqlite parameter binding {} was supplied but is not used by the SQL",
                    param.sql_name
                )
            })?;
        stmt.raw_bind_parameter(index, param.value.as_str())
            .with_context(|| format!("failed to bind sqlite parameter {}", param.sql_name))?;
        bound_indexes.insert(index);
    }

    for index in 1..=stmt.parameter_count() {
        if bound_indexes.contains(&index) {
            continue;
        }
        let name = stmt.parameter_name(index).ok_or_else(|| {
            anyhow!(
                "anonymous sqlite parameter at index {index} is unsupported; use a named parameter like :input"
            )
        })?;
        return Err(anyhow!(
            "unbound sqlite parameter {name}; provide --json-param or --jsonl-param"
        ));
    }
    Ok(())
}

fn sqlite_param_receipt_rows(params: &[SqliteFileParam]) -> Value {
    json!(
        params
            .iter()
            .map(|param| json!({
                "name": param.sql_name,
                "path": display_path(&param.path),
                "format": param.format,
                "values": param.values,
                "source_bytes": param.source_bytes,
            }))
            .collect::<Vec<_>>()
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_sqlite_schema(
    cli: &Cli,
    config: &ContextConfig,
    db: &Path,
    requested_tables: &[String],
    name_contains: &[String],
    include_shadow: bool,
    include_system: bool,
    max_tables: usize,
    max_columns: usize,
    max_indexes: usize,
    max_line_chars: usize,
) -> Result<()> {
    if max_tables == 0 {
        return Err(anyhow!(
            "sqlite-schema --max-tables must be greater than zero"
        ));
    }
    if max_line_chars == 0 {
        return Err(anyhow!(
            "sqlite-schema --max-line-chars must be greater than zero"
        ));
    }
    let conn = open_sqlite_readonly(db)?;
    let requested = requested_tables.iter().collect::<BTreeSet<_>>();
    let mut stmt = conn
        .prepare(
            "SELECT schema, name, type, ncol, wr, strict \
             FROM pragma_table_list \
             ORDER BY schema, name",
        )
        .context("failed to prepare sqlite schema query")?;
    let mut table_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, i64>(5)? != 0,
            ))
        })
        .context("failed to query sqlite schema")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to read sqlite schema rows")?;
    table_rows.retain(|(_, name, kind, _, _, _)| {
        if !include_system && name.starts_with("sqlite_") {
            return false;
        }
        if !include_shadow && kind == "shadow" {
            return false;
        }
        if !requested.is_empty() && !requested.contains(name) {
            return false;
        }
        if !name_contains.is_empty() && !contains_any(name, name_contains) {
            return false;
        }
        true
    });
    let total_tables = table_rows.len();
    let shown_tables = min(total_tables, max_tables);
    let mut remaining_columns = max_columns;
    let mut remaining_indexes = max_indexes;
    let mut columns_total = 0usize;
    let mut columns_shown = 0usize;
    let mut indexes_total = 0usize;
    let mut indexes_shown = 0usize;
    let mut summaries = Vec::with_capacity(shown_tables);
    let mut tables_detail_elided = 0usize;
    for (schema, name, kind, column_count_declared, without_rowid, strict) in
        table_rows.into_iter().take(shown_tables)
    {
        let all_columns = sqlite_schema_columns(&conn, &schema, &name)?;
        let all_indexes = sqlite_schema_indexes(&conn, &schema, &name)?;
        let all_columns_len = all_columns.len();
        let all_indexes_len = all_indexes.len();
        columns_total += all_columns_len;
        indexes_total += all_indexes_len;
        // Table-atomic budget: a table either shows its complete column and
        // index detail or none of it. A partially-columned table with its
        // indexes still attached reads as complete to anyone slicing the
        // middle of the output.
        let detail_elided =
            all_columns_len > remaining_columns || all_indexes_len > remaining_indexes;
        let (columns_take, indexes_take) = if detail_elided {
            tables_detail_elided += 1;
            (0, 0)
        } else {
            (all_columns_len, all_indexes_len)
        };
        columns_shown += columns_take;
        indexes_shown += indexes_take;
        remaining_columns = remaining_columns.saturating_sub(columns_take);
        remaining_indexes = remaining_indexes.saturating_sub(indexes_take);
        summaries.push(SqliteTableSummary {
            schema,
            name,
            kind,
            column_count_declared,
            without_rowid,
            strict,
            columns: all_columns.into_iter().take(columns_take).collect(),
            indexes: all_indexes.into_iter().take(indexes_take).collect(),
            columns_total: all_columns_len,
            indexes_total: all_indexes_len,
            detail_elided,
        });
    }
    let columns_truncated = columns_shown < columns_total;
    let indexes_truncated = indexes_shown < indexes_total;
    let line_characters_truncated = !cli.json
        && summaries.iter().any(|table| {
            sqlite_table_summary_human(table).chars().count() > max_line_chars
                || table.columns.iter().any(|column| {
                    sqlite_column_summary_human(column).chars().count() > max_line_chars
                })
                || table
                    .indexes
                    .iter()
                    .any(|index| sqlite_index_summary_human(index).chars().count() > max_line_chars)
        });
    let truncated = shown_tables < total_tables
        || columns_truncated
        || indexes_truncated
        || line_characters_truncated;
    let mut receipt = Receipt::new(
        "sqlite-schema",
        config.profile.as_deref(),
        ReceiptResult::new("tables", total_tables, false, shown_tables),
    );
    if shown_tables < total_tables {
        receipt.add_cap(ReceiptCap::output("tables", Some(max_tables)));
    }
    if columns_truncated {
        receipt.add_cap(ReceiptCap::output("columns", Some(max_columns)));
    }
    if indexes_truncated {
        receipt.add_cap(ReceiptCap::output("indexes", Some(max_indexes)));
    }
    if line_characters_truncated {
        receipt.add_cap(ReceiptCap::output("line_characters", Some(max_line_chars)));
    }
    receipt.insert("db", json!(display_path(db)));
    receipt.insert("columns_shown", json!(columns_shown));
    receipt.insert("columns_total", json!(columns_total));
    receipt.insert("indexes_shown", json!(indexes_shown));
    receipt.insert("indexes_total", json!(indexes_total));
    receipt.insert("tables_detail_elided", json!(tables_detail_elided));
    if cli.json {
        receipt.insert(
            "tables",
            Value::Array(
                summaries
                    .iter()
                    .map(sqlite_table_summary_json)
                    .collect::<Vec<_>>(),
            ),
        );
        return emit_json_checked(cli, receipt);
    }
    let mut stdout = io::stdout();
    writeln!(
        stdout,
        "[contextmink] sqlite-schema db={}",
        display_path(db)
    )?;
    if summaries.is_empty() {
        writeln!(stdout, "no_tables")?;
    }
    for table in &summaries {
        writeln!(
            stdout,
            "{}",
            clamp_text(&sqlite_table_summary_human(table), max_line_chars)
        )?;
        for column in &table.columns {
            writeln!(
                stdout,
                "  column {}",
                clamp_text(&sqlite_column_summary_human(column), max_line_chars)
            )?;
        }
        for index in &table.indexes {
            writeln!(
                stdout,
                "  index {}",
                clamp_text(&sqlite_index_summary_human(index), max_line_chars)
            )?;
        }
        if table.detail_elided {
            writeln!(
                stdout,
                "  (detail elided: {} columns, {} indexes over budget; rerun with --table {})",
                table.columns_total, table.indexes_total, table.name
            )?;
        }
    }
    if truncated {
        writeln!(
            stdout,
            "[contextmink] capped sqlite schema output at tables={max_tables} columns={max_columns} indexes={max_indexes}; narrow with --table or --name-contains."
        )?;
    }
    write_receipt_checked(cli, receipt)
}

fn sqlite_schema_columns(
    conn: &Connection,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<SqliteColumnSummary>> {
    let mut fks = HashMap::new();
    let mut fk_stmt = conn
        .prepare("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list(?, ?)")
        .context("failed to prepare sqlite foreign-key query")?;
    let fk_rows = fk_stmt
        .query_map([table_name, schema_name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SqliteForeignKeySummary {
                    table: row.get::<_, String>(1)?,
                    column: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                },
            ))
        })
        .with_context(|| format!("failed to query foreign keys for {table_name}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| format!("failed to read foreign keys for {table_name}"))?;
    for (column, fk) in fk_rows {
        fks.insert(column, fk);
    }

    let mut stmt = conn
        .prepare(
            "SELECT name, lower(type), \"notnull\", dflt_value, pk, hidden \
             FROM pragma_table_xinfo(?, ?) \
             ORDER BY cid",
        )
        .context("failed to prepare sqlite column query")?;
    stmt.query_map([table_name, schema_name], |row| {
        let name = row.get::<_, String>(0)?;
        Ok(SqliteColumnSummary {
            foreign_key: fks.get(&name).cloned(),
            name,
            type_name: row.get::<_, String>(1)?,
            not_null: row.get::<_, i64>(2)? != 0,
            default_value: row.get::<_, Option<String>>(3)?,
            primary_key_rank: row.get::<_, i64>(4)?,
            hidden: row.get::<_, i64>(5)?,
        })
    })
    .with_context(|| format!("failed to query columns for {table_name}"))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .with_context(|| format!("failed to read columns for {table_name}"))
}

fn sqlite_schema_indexes(
    conn: &Connection,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<SqliteIndexSummary>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?, ?) ORDER BY seq",
        )
        .context("failed to prepare sqlite index query")?;
    let mut indexes = Vec::new();
    for row in stmt
        .query_map([table_name, schema_name], |row| {
            Ok(SqliteIndexSummary {
                name: row.get::<_, String>(0)?,
                unique: row.get::<_, i64>(1)? != 0,
                origin: row.get::<_, String>(2)?,
                partial: row.get::<_, i64>(3)? != 0,
                columns: Vec::new(),
            })
        })
        .with_context(|| format!("failed to query indexes for {table_name}"))?
    {
        let mut index = row.with_context(|| format!("failed to read index for {table_name}"))?;
        let mut col_stmt = conn
            .prepare("SELECT cid, name FROM pragma_index_xinfo(?, ?) WHERE key != 0 ORDER BY seqno")
            .with_context(|| format!("failed to prepare index-column query for {}", index.name))?;
        index.columns = col_stmt
            .query_map([index.name.as_str(), schema_name], |row| {
                let cid = row.get::<_, i64>(0)?;
                let name = row.get::<_, Option<String>>(1)?;
                Ok(name.unwrap_or_else(|| match cid {
                    -2 => "<expr>".to_owned(),
                    -1 => "<rowid>".to_owned(),
                    _ => "<unknown>".to_owned(),
                }))
            })
            .with_context(|| format!("failed to query columns for index {}", index.name))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .with_context(|| format!("failed to read columns for index {}", index.name))?;
        indexes.push(index);
    }
    Ok(indexes)
}

fn sqlite_table_summary_json(table: &SqliteTableSummary) -> Value {
    json!({
        "schema": table.schema,
        "name": table.name,
        "type": table.kind,
        "ncol": table.column_count_declared,
        "strict": table.strict,
        "without_rowid": table.without_rowid,
        "columns_total": table.columns_total,
        "indexes_total": table.indexes_total,
        "detail_elided": table.detail_elided,
        "columns": table.columns.iter().map(|column| {
            json!({
                "name": column.name,
                "type": column.type_name,
                "not_null": column.not_null,
                "default": column.default_value,
                "primary_key_rank": column.primary_key_rank,
                "hidden": column.hidden,
                "foreign_key": column.foreign_key.as_ref().map(|fk| json!({
                    "table": fk.table,
                    "column": fk.column,
                })),
            })
        }).collect::<Vec<_>>(),
        "indexes": table.indexes.iter().map(|index| {
            json!({
                "name": index.name,
                "unique": index.unique,
                "origin": index.origin,
                "partial": index.partial,
                "columns": index.columns,
            })
        }).collect::<Vec<_>>(),
    })
}

fn sqlite_table_summary_human(table: &SqliteTableSummary) -> String {
    format!(
        "{}.{} type={} ncol={} strict={} without_rowid={}",
        table.schema,
        table.name,
        table.kind,
        table.column_count_declared,
        table.strict,
        table.without_rowid
    )
}

fn sqlite_column_summary_human(column: &SqliteColumnSummary) -> String {
    let mut parts = vec![format!("{} {}", column.name, column.type_name)];
    if column.not_null {
        parts.push("not_null".to_owned());
    }
    if column.primary_key_rank != 0 {
        parts.push(format!("pk#{}", column.primary_key_rank));
    }
    if column.hidden != 0 {
        parts.push(format!("hidden#{}", column.hidden));
    }
    if let Some(default) = &column.default_value {
        parts.push(format!("default={default:?}"));
    }
    if let Some(fk) = &column.foreign_key {
        parts.push(format!("fk={}.{}", fk.table, fk.column));
    }
    parts.join(" ")
}

fn sqlite_index_summary_human(index: &SqliteIndexSummary) -> String {
    let mut parts = vec![format!("{}({})", index.name, index.columns.join(","))];
    if index.unique {
        parts.push("unique".to_owned());
    }
    if index.partial {
        parts.push("partial".to_owned());
    }
    parts.push(format!("origin={}", index.origin));
    parts.join(" ")
}

/// Interrupts a running query after a wall-clock budget so a runaway scan
/// fails with an accountable error instead of hanging until the calling
/// shell kills the process without a receipt.
struct QueryWatchdog {
    cancel: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl QueryWatchdog {
    fn arm(conn: &Connection, timeout_secs: u64) -> Self {
        if timeout_secs == 0 {
            return Self {
                cancel: None,
                thread: None,
            };
        }
        let handle = conn.get_interrupt_handle();
        let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            // A disconnect (watchdog dropped) means the query finished.
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                cancelled.recv_timeout(std::time::Duration::from_secs(timeout_secs))
            {
                handle.interrupt();
            }
        });
        Self {
            cancel: Some(cancel),
            thread: Some(thread),
        }
    }
}

impl Drop for QueryWatchdog {
    fn drop(&mut self) {
        drop(self.cancel.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join(); // guardrail: allow-ignore-result watchdog thread cannot fail meaningfully after cancellation
        }
    }
}

fn annotate_interrupt(error: rusqlite::Error, timeout_secs: u64) -> anyhow::Error {
    let interrupted = matches!(
        &error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::OperationInterrupted
    );
    if interrupted {
        anyhow::Error::new(error).context(format!(
            "sqlite query interrupted after --timeout-secs {timeout_secs}; narrow the query (WHERE/LIMIT) or raise --timeout-secs (0 disables)"
        ))
    } else {
        anyhow::Error::new(error).context("failed to read sqlite row")
    }
}

fn open_sqlite_readonly(db: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("failed to open sqlite DB {}", db.display()))?;
    // A concurrent writer committing (rollback/TRUNCATE journals) briefly
    // locks readers out; wait instead of failing with SQLITE_BUSY.
    conn.busy_timeout(std::time::Duration::from_millis(5000))
        .context("failed to set sqlite busy timeout")?;
    conn.execute_batch("PRAGMA query_only = ON")
        .context("failed to enable sqlite query_only mode")?;
    register_hexint(&conn)?;
    conn.authorizer(Some(|context: rusqlite::hooks::AuthContext<'_>| {
        use rusqlite::hooks::{AuthAction, Authorization};
        match context.action {
            AuthAction::Select
            | AuthAction::Read { .. }
            | AuthAction::Function { .. }
            | AuthAction::Recursive
            | AuthAction::Pragma {
                pragma_value: None, ..
            } => Authorization::Allow,
            AuthAction::Pragma { pragma_name, .. }
                if matches!(
                    pragma_name.to_ascii_lowercase().as_str(),
                    "table_info"
                        | "table_xinfo"
                        | "index_list"
                        | "index_xinfo"
                        | "foreign_key_list"
                        | "table_list"
                ) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }
    }))
    .context("failed to install sqlite read-only authorizer")?;
    Ok(conn)
}

/// `hexint(x)`: parse a `0x`-prefixed hex string (or a plain decimal digit
/// string) to INTEGER; integers pass through, NULL stays NULL. `SQLite`'s own
/// CAST cannot parse hex, and inspection data often carries address-like
/// identifiers as `0x...` strings while tables store integer columns. This
/// bridges the two inside an indexed join instead of forcing scratch
/// conversion outside SQL.
fn register_hexint(conn: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "hexint",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let value = ctx.get_raw(0);
            match value {
                ValueRef::Null => Ok(rusqlite::types::Value::Null),
                ValueRef::Integer(value) => Ok(rusqlite::types::Value::Integer(value)),
                ValueRef::Text(bytes) => {
                    let text = std::str::from_utf8(bytes).map_err(|_| {
                        rusqlite::Error::UserFunctionError("hexint: text is not UTF-8".into())
                    })?;
                    let trimmed = text.trim();
                    let parsed = if let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                    {
                        i64::from_str_radix(hex, 16)
                    } else {
                        trimmed.parse::<i64>()
                    };
                    parsed.map(rusqlite::types::Value::Integer).map_err(|_| {
                        rusqlite::Error::UserFunctionError(
                            format!("hexint: cannot parse {trimmed:?} as 0x-hex or decimal").into(),
                        )
                    })
                }
                other => Err(rusqlite::Error::UserFunctionError(
                    format!("hexint: unsupported input type {}", other.data_type()).into(),
                )),
            }
        },
    )
    .context("failed to register hexint SQL function")
}

fn sqlite_value_summary(value: ValueRef<'_>, max_chars: usize) -> ClampedText {
    match value {
        ValueRef::Null => clamp_text_with_status("null", max_chars),
        ValueRef::Integer(value) => clamp_text_with_status(&value.to_string(), max_chars),
        ValueRef::Real(value) => clamp_text_with_status(&value.to_string(), max_chars),
        ValueRef::Text(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => {
                let inner = clamp_text_with_status(text, max_chars.saturating_sub(2));
                let mut rendered = clamp_text_with_status(&format!("{:?}", inner.text), max_chars);
                rendered.truncated |= inner.truncated;
                rendered
            }
            Err(_) => clamp_text_with_status(
                &format!("<invalid-utf8-text:{} bytes>", bytes.len()),
                max_chars,
            ),
        },
        ValueRef::Blob(value) => {
            clamp_text_with_status(&format!("<blob:{} bytes>", value.len()), max_chars)
        }
    }
}

#[cfg(test)]
mod tests;
