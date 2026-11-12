---
name: apple-mail-cli
description: Use when an AI agent must run rusty_apple_mail_mcp CLI to discover accounts, search mail, read message or attachment windows, close pagination, or handle CLI errors.
---

# Apple Mail CLI SOP

The CLI reads Apple Mail and returns evidence. It never writes. Your answer must separate what the
mail said from what you could not retrieve.

Treat audit, legal, financial, and whole-thread work as exhaustive: close every page and every
window before you answer.

Load a reference only when you reach that branch. Both ship inside the CLI:

```bash
rusty_apple_mail_mcp skills get apple-mail-cli --ref cli-recipes
rusty_apple_mail_mcp skills get apple-mail-cli --ref process-contract
```

- `cli-recipes` gives exact commands, flags, and shell examples.
- `process-contract` gives stdout and stderr, exit codes, and what to do with each failure.

## 1. Resolve the CLI

Use the command installed in the user's environment: `rusty_apple_mail_mcp` from `PATH`, or the
installed binary path the user gave you. Do not assume a repository build artifact.

Run `rusty_apple_mail_mcp --help` before you rely on a remembered flag.

Done when: you know the executable path and the flags it accepts.

## 2. Set Scope

Scope is the startup allowlist and applies to every later call. Pass it with
`--scope-account <selector>` or `APPLE_MAIL_ACCOUNT=<selector>` when the task names an account. The
top-level `--account` is a deprecated alias of the same thing. The `--account` flag on `search`
filters a single call; Scope applies to all of them.

Run `list-accounts` before you use any account ID or mailbox name.

Done when: Scope and Filter stay distinct, and every account or mailbox selector came from
`list-accounts` output or from the user.

## 3. Search

`search` needs at least one filter: `--subject-query`, `--date-from`, `--date-to`, `--sender`,
`--participant`, `--account`, or `--mailbox`. Start with the narrowest filter that can answer the
question. Broaden it only after the result is `not_found` or the evidence is too thin.

For exhaustive work, continue with `--offset <next_offset>` while `has_more` is true, and stop when
`has_more` is false.

Done when: a narrow lookup names only message IDs the result list justifies, and an exhaustive
lookup ends with `has_more` false.

## 4. Read message windows

`get-message` returns one message. Recipients come only when you ask for them.

When `outcome` is `partial`, continue the same message with the returned `window.next_offset` and
`window.source_revision`. For exhaustive work, close every required window.

Done when: no claim rests on a partial window you have not closed.

## 5. Read attachment windows

Attachment IDs come from `get-message` output and look like `{message_id}:{index}`. Pass one to
`get-attachment`.

`window.complete` describes delivery. The extracted text can still be lossy, so read
`window.extraction_limitations` separately.

Done when: every attachment you cite is complete for delivery, partial with its continuation
closed, or reported with its extraction limitation.

## 6. Interpret failures

Success prints one JSON document on stdout and exits `0`. A failure prints nothing on stdout, writes
a JSON error as the last stderr line, and exits non-zero.

Read the process contract before you decide whether to retry, fix the request, or report a gap.

Done when: your answer separates retrieved evidence from missing data, partial delivery, extraction
limitations, and configuration or access failures.
