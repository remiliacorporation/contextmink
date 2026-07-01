### Bounded Output

Use `scripts/contextmink` when a file/text/JSON/SQLite/command-output read may
produce more output than the transcript should carry.

- Start with `dirs` to orient in an unfamiliar tree, then `files` or `grep`
  for candidate discovery. Prefer `files --ext json` / `--extension jsonl`
  across Windows-to-Bash boundaries because wildcard globs can expand before
  contextmink receives them.
- Use `grep --pattern-file <file>` for shell-fragile regex; use `grep-terms`
  for literal tokens or phrases (`--or` / `--any`, `--term-file`, `--limit`,
  `--max-matches`). Narrow either with `--glob` / `--ext`, add `-i` for
  case-insensitive matching, and `--context N` when the surrounding lines
  would otherwise need a follow-up `slice`.
- Use `slice` (including `--tail N` for the end of logs), `json-find`,
  `json-select` (with `--where FIELD=VALUE` / `--where-contains FIELD=TEXT`
  row filters), `sqlite-schema`, and `sqlite --sql-file` for bounded reads
  instead of opening whole large files, reports, or databases.
- Prefer a domain command's native compact/projection/limit flags first. Use
  `capture -- <command> ...` or `run` only when output size is uncertain and no
  native bound exists; read the child `exit_code`/`success` fields in the
  receipt. Truncated captures keep both the head and the tail of the output.
- Configured excludes keep broad scans quiet. Pass an explicit file or
  subdirectory when an excluded tree is the target. Use `--with-excluded` to
  include files matched by contextmink exclude globs, and `--with-git-ignored`
  only for files hidden by Git or `.ignore` rules. Broad scans enter
  git-ignored nested repository roots and disclose them in
  `nested_repos_entered`; pass `--skip-nested-repos` for strict Git scope.
- Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` or `"complete": false`
  as capped output and narrow the query. Use `--fail-if-truncated` /
  `--strict-complete` for automation that requires full displayed output, or
  `--require-complete-scan` when scan-capped totals should fail. When
  `cap_reason` is `"scan"` or lower-bound fields are true, totals and no-match
  results cover only the scanned subset. A no-match grep with
  `no_match_scope: "scanned_subset"` or a `json-select` with `all_null_fields`
  entries needs a narrower or corrected query, not a conclusion.
- Direct commands are fine when output is already known to be small or
  structurally bounded, such as `git status --short`, `git diff --stat`, one
  exact small file, a focused test command, or a domain tool that emits compact
  records.
- Keep domain-specific parsing, validation, indexing, diagnostics, and
  synchronization in project-native tools.
