### Context Hygiene

Use `scripts/contextmink` for broad or uncertain file/text/JSON reconnaissance before opening raw files or command output in the transcript. Start with `files` or `grep`, use `grep-terms` for shell-fragile multi-token searches, use `slice` for exact line windows, and use `json-find` for sidecars, reports, manifests, logs, or other structured command output. Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` as incomplete evidence and narrow the query.

`contextmink` is only a generic transcript guard. Prefer project-native tools for domain-specific parsing, validation, indexing, diagnostics, or synchronization.
