### Context Hygiene

Use `scripts/contextmink` for broad or uncertain file/text/JSON reconnaissance before opening raw files or command output in the transcript. Start with `files` or `grep`, use `grep-terms` for shell-fragile token or phrase searches (`--mode any` for alternatives; `--term-file` for phrase lists), use `slice` for exact line windows, and use `json-find` for sidecars, reports, manifests, logs, or other structured command output. Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` as incomplete evidence and narrow the query.

Do not route everything through `contextmink`. Direct commands are appropriate when the output is already known to be small or structurally bounded, such as `git status --short`, `git diff --stat`, one exact small file, a focused test command, or a domain tool that already emits compact/limited records. If output turns out broader than expected, stop treating it as evidence and rerun through `contextmink` with narrower paths or caps.

`contextmink` is only a generic transcript guard. Prefer project-native tools for domain-specific parsing, validation, indexing, diagnostics, or synchronization.
