# Process contract

Use this reference when a CLI command fails, returns partial data, or the user asks how stdout, stderr, and exit codes should be interpreted.

## Output streams

Success writes one JSON document to stdout and exits `0`.

Failures leave stdout empty, write diagnostics to stderr, write a structured JSON error as the final stderr line, and exit non-zero.

Completion criterion: parse stdout only for successful commands; parse the final stderr JSON object for failed commands.

## Exit categories

| Code | Meaning |
|---:|---|
| `0` | Command executed, including empty search/list outcomes |
| `1` | Internal or unclassified failure |
| `2` | Usage or invalid input |
| `3` | Requested message or attachment not found |
| `4` | Retryable or temporarily unavailable failure |
| `5` | Configuration, environment, or Scope failure |

Completion criterion: command handling branches by category instead of treating every non-zero exit as the same failure.

## Error handling

On a structured error, branch by `error_kind` and `guidance`:

- Fix bad parameters directly when the failure is usage or validation.
- Broaden the query only after `outcome: "not_found"` or a not-found exit proves the current evidence path failed.
- Retry only retryable or unavailable failures.
- Ask for access or configuration when Full Disk Access, mail path, mail version, account selector, or Scope is missing.

Completion criterion: the final answer distinguishes retrieved evidence from missing data, partial delivery, extraction limitations, and configuration or access failures.
