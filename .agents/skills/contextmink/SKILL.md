---
name: contextmink
description: Use before broad file discovery, repository searches, large file/log reads or structured-data inspection, including find/rg output you would otherwise pipe to head. Skip known-small reads and project-native compact queries.
---

# Contextmink

Keep uncertain-size retrieval out of the transcript until its scope and output
are bounded. Use this capability during ordinary work, without waiting for the
user to name it. Return to direct tools when the exact result is known to be small.

## Invoke


Use the installed command above when present. For a project installation, resolve
`tools/contextmink/bin/contextmink[.exe]` from the project root; a source-vendored
Bash project may use `scripts/contextmink`. Keep cwd in the consuming project so
its configuration applies. Native built-in commands need no project guidance edits or new configuration. If the runtime is missing, report the
incomplete project bundle rather than inventing an executable path.
Below, `contextmink` means the resolved native executable (quoted with `&` in
PowerShell when its path contains spaces).

## Retrieve progressively

Choose the narrowest useful scope. Directory orientation is optional; search a
known subtree directly. Use live command `--help` rather than guessing flags.

```text
contextmink files [PATH] --path-contains TEXT --ext rs --limit 20
contextmink grep --pattern PATTERN [PATH] --context 2 --limit 8
contextmink outline FILE --contains TEXT
contextmink slice FILE --range START:END
contextmink slice FILE --tail 30
contextmink json-select FILE --keys
contextmink json-select FILE --at /rows --fields FIELD --limit 20
contextmink sqlite DB --sql-file QUERY.sql --limit 20
contextmink capture --max-lines 40 -- PROGRAM ARGS
```

`grep-terms --term TEXT [PATH]` avoids fragile literal quoting. Grep's `--limit`
counts files, `--lines-per-file` counts matches per file, and `--max-sample-lines`
bounds displayed lines. A capped slice gives `remaining_range` for follow-up.
Narrow before raising caps. Budget batched calls together; per-command caps do
not bound their combined tool response.

JSON selectors are literal keys or JSON Pointers, not dotted paths. Enter nested
values with `--at`; summaries are not their contents. For objects keyed by IDs,
use `--entries --fields FIELD` to project children while preserving their keys.
Missing fields differ from nulls. Prefer domain tools' own compact queries.

## Preserve evidence limits

Do not pipe bounded output through `head` or `tail`: that can remove the receipt.
Use the command's own caps or a narrower query instead.

Read the receipt: `scope_complete: false` means only a subset was inspected;
`output_truncated: true` means payload was omitted. Lower-bound totals are not
exact. A subset no-match cannot establish absence. Follow the reported exhausted
caps or continuation, and recheck relevant regions after edits. Use
`--require-complete-scope` or `--fail-if-truncated` when the conclusion needs it.

Capture runs a command once. Check `child_exit_code`, `child_exit_zero` and
`exit_expected`. Retain full producer output in files on the first run if it may
matter: capture is not an archive. Never repeat a mutation or costly command just
to recover clipped output. Inspect retained artifacts or establish safe replay.

Read [the integration reference](../../../tools/contextmink/agent_integration.md)
only for setup, configuration, hooks or unfamiliar receipt fields.
