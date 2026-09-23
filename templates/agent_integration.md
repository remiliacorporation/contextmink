# Contextmink integration reference

Detail behind the Contextmink skill. Flags and defaults: `contextmink <command> --help`.

## Invocation

- Run the native executable directly from any shell: a personal install's bound
  path, or `tools/contextmink/bin/contextmink[.exe]` from the project root.
  Run from the consuming project so its `.contextmink.toml` applies.
- PowerShell: `& <path-to-contextmink.exe> ...` (the call operator is required
  when the path is quoted).
- `scripts/contextmink` exists only in `setup-project` or source-vendored
  installs; it is a Bash launcher, not a requirement.
- Git Bash rewrites native arguments that start with `/` (JSON Pointers,
  patterns, path fragments) into Windows paths. Contextmink refuses such
  arguments before doing work; prefix the command with `MSYS_NO_PATHCONV=1`.

## Receipts

Every retrieval command and `capture` ends with a `contextmink.receipt.v2`
envelope (the JSON object itself with `--json`).

- `complete` is `scope_complete && !output_truncated`.
- `scope_complete: false`: only a subset was inspected. Totals with
  `result.total_is_lower_bound: true` are lower bounds, and a no-match proves
  nothing (`no_match_scope: "scanned_subset"`).
- `output_truncated: true`: the scope was inspected but payload was omitted or
  shortened.
- `caps[]` rows give `boundary` (`scope` or `output`), `dimension`, `limit`.
- `output_cap_arguments` names the display flags whose caps were exhausted;
  raise only those, after narrowing the query. `--max-*` flags bound scope.
- `remaining_range` on a capped `slice` is the `--range` that continues the
  omitted lines; character clipping is a separate cap.
- `json-select` `all_null_fields` lists fields null in every scanned row; a
  missing field and a null field differ.
- `--require-complete-scope` and `--fail-if-truncated` (after the subcommand)
  turn an incomplete receipt into a nonzero exit after it is printed.

## Capture

`capture -- PROGRAM ARGS` runs a non-interactive command once and keeps the
head and tail of each stream within its display caps. Read `child_exit_code`,
`child_exit_zero`, and `exit_expected` (see `--expect-exit`). Capture is not an
archive: redirect producer output to a file on the first run when full bytes
may matter, and never rerun a mutation just to recover clipped output.

## Scope

- Configured excludes and Git ignore rules quiet broad scans. An explicit file
  or subdirectory inside a configured-exclude tree is honored; `--with-excluded`
  lifts exclude globs and `--with-git-ignored` lifts Git ignore rules.
- Broad scans cross nested Git repositories (including submodules and
  Git-ignored sibling checkouts) and report `nested_repos_entered_total` plus a
  bounded sample. `--skip-nested-repos` stays inside each explicit root; pass a
  nested repository explicitly when it is the target.

## When to read directly

Direct reads are fine when output is known to be small and structurally
bounded: `git status --short`, a focused test, a domain tool's compact query,
or one known file region under about 120 lines. Above that, use `outline`,
`grep`, and `slice`: choosing a large range does not make its output small.
Domain parsing, validation, and indexing stay in project-native tools.
