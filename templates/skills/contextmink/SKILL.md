---
name: contextmink
description: Use before broad file discovery, repository searches, large file/log reads or structured-data inspection, including find/rg output you would otherwise pipe to head. Skip known-small reads and project-native compact queries.
---

# Contextmink

Keep uncertain-size retrieval out of the transcript until its scope and output
are bounded. Use this capability during ordinary work, without waiting for the
user to name it. Return to direct tools when the exact result is known to be small.

## Invoke

<!-- installed-command -->

For a project installation, resolve `tools/contextmink/bin/contextmink[.exe]`
from the project root (Bash-vendored projects may use `scripts/contextmink`).
Run from the project so its config applies. If no executable is installed,
report the incomplete installation; don't guess a path. PowerShell: invoke with `&`.

## Retrieve progressively

Choose the narrowest useful scope; `dirs` orients in an unknown tree. Use live
`--help` rather than guessing flags.

```text
contextmink files [PATH] --path-contains TEXT --ext rs --show-files 20
contextmink grep --pattern PATTERN [PATH] --context 2 --show-files 8
contextmink outline FILE --contains TEXT
contextmink slice FILE --range START:END
contextmink slice FILE --tail 30
contextmink json-select FILE --keys
contextmink json-select FILE --at /rows --fields FIELD --show-rows 20
contextmink sqlite DB --sql-file QUERY.sql --show-rows 20
contextmink capture --show-lines 40 -- PROGRAM ARGS
```

`grep-terms --term TEXT [PATH]` avoids fragile literal quoting; repeated terms
must all match one line unless `--any` is passed. Narrow before raising caps.
If capped, raise only the flags named in `output_cap_arguments`. Budget batched
calls together; per-command caps do not bound their combined tool response.

JSON selectors are literal keys or JSON Pointers, not dotted paths. Enter nested
values with `--at`; summaries are not their contents. For objects keyed by IDs,
use `--entries --fields FIELD` to project children while preserving their keys.
Prefer domain tools' own compact queries.

In Git Bash, native arguments starting with `/` are rewritten; Contextmink
refuses them — prefix `MSYS_NO_PATHCONV=1`.

## Preserve evidence limits

Do not pipe bounded output through `head` or `tail`: that can remove the receipt.

Check `complete`. If false: `scope_complete: false` means only a subset was
inspected (totals may be lower bounds; a no-match proves nothing);
`output_truncated: true` means payload was omitted. A receipt describes files
as they were when it ran; edits make it stale.

Capture runs a command once. Check `child_exit_code`, `child_exit_zero` and
`exit_expected`. Retain full producer output in files on the first run if it may
matter. Never repeat a mutation or costly command just to recover clipped output.

Read [the integration reference](../../../tools/contextmink/agent_integration.md)
only for unfamiliar receipt fields or invoking from another shell. Setup, config
and hooks: `contextmink setup-user|setup-project|guard-hook-snippet --help`.
