# Process contract

Read this when a command fails, returns partial data, or you need to know how stdout, stderr, and
exit codes fit together.

## Streams

A successful command writes one JSON document to stdout and exits `0`. A failed command writes
nothing to stdout, writes diagnostics to stderr, and writes a structured JSON error as the final
stderr line.

Done when: you parse stdout only for a successful command, and the last stderr line for a failed
one.

## Exit codes and error kinds

Branch on `error_kind`. The exit code is the coarse category.

| `error_kind` | Exit | Agent action |
|---|---:|---|
| `missing_filter`, `bad_limit`, `bad_date`, `bad_message_id`, `bad_attachment_id`, `account_filter_invalid`, `usage_error` | `2` | Fix the request and call again |
| `message_not_found`, `attachment_not_found` | `3` | Give up, or ask the user for a valid ID |
| `database_locked` | `4` | Retry after a short delay |
| `database_not_found`, `config_error` | `5` | Fix the configuration, including Full Disk Access |
| `io_failure` | `1` or `5` | Retry once, then check the file and its permissions |
| `internal_error` | `1` | Report a bug |

Done when: you branch on the category instead of treating every non-zero exit as the same failure.

## Error handling

A structured error carries `error_kind`, `message`, and `guidance`.

- Fix bad parameters directly when the kind says the arguments are wrong.
- Broaden the query only after `outcome: "not_found"` or a not-found exit proves the current evidence path failed.
- Retry only retryable or unavailable failures.
- Ask for access or configuration when Full Disk Access, the mail path, the mail version, the account selector, or the Scope is missing.

Done when: your answer separates retrieved evidence from missing data, partial delivery, extraction
limitations, and configuration or access failures.
