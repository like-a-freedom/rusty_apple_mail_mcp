---
name: apple-mail-cli
description: Use when an AI agent must run rusty_apple_mail_mcp CLI to discover accounts, search mail, read message or attachment windows, close pagination, or handle CLI errors.
---

# Apple Mail CLI SOP

Use the CLI as a read-only evidence retriever. Keep the loop tight: resolve the installed command, set Scope, discover identifiers, search a bounded shortlist, retrieve selected content windows, then stop with an explicit evidence boundary.

Load extra files only for the branch that needs them:

- For exact commands, flags, and shell examples, read [CLI recipes](references/cli-recipes.md).
- For non-zero exits, stderr parsing, and retry/access decisions, read [process contract](references/process-contract.md).

## 1. Resolve the CLI contract

Use the installed command from the user's environment. Prefer `rusty_apple_mail_mcp` from `PATH`; if absent, use the user-supplied installed binary path. Do not assume a repository build artifact.

Run local help before relying on remembered examples. Read [CLI recipes](references/cli-recipes.md) when exact help commands or common command forms are needed.

Completion criterion: the executable path and available global/subcommand flags are known from the user's environment.

## 2. Set Scope deliberately

Scope is the startup allowlist. Prefer `--scope-account <selector>` or `APPLE_MAIL_ACCOUNT=<selector>` when the task names an account. The legacy top-level `--account` is a compatibility alias. `search --account` is a per-call Filter.

Use `list-accounts` before guessing account IDs or mailbox names. Read [CLI recipes](references/cli-recipes.md) for account and mailbox discovery commands.

Completion criterion: Scope and Filter are kept distinct, and any account/mailbox selector used later came from the CLI output or the user.

## 3. Search a shortlist

Search requires at least one Filter. Use the narrowest truthful filter first, then broaden only when `outcome: "not_found"` or the evidence is insufficient.

Read [CLI recipes](references/cli-recipes.md) for common search commands.

Close pagination when the task needs exhaustive evidence. Continue with `--offset <next_offset>` while `has_more` is true. Stop only when `has_more` is false.

Completion criterion: for narrow lookups, the selected message IDs are justified by the shortlist; for audit, legal, financial, or whole-thread work, every relevant page is closed.

## 4. Retrieve message windows

Read selected messages with `get-message`. Recipients are omitted unless requested.

If `outcome` is `partial`, continue the same message with the returned `window.next_offset` and `window.source_revision`. Read [CLI recipes](references/cli-recipes.md) for exact continuation commands.

Completion criterion: partial content remains incomplete until required continuations are closed. For audit, legal, financial, or whole-thread claims, retrieve every required window.

## 5. Retrieve attachment windows

Use attachment IDs from `get-message` output. Attachment IDs have the shape `{message_id}:{index}`.

Read `window.extraction_limitations` separately from `window.complete`. A complete window can still be complete delivery of partially extracted source text.

Completion criterion: every attachment used as evidence is either complete for delivery, explicitly partial with continuation closed as needed, or reported with its extraction limitation.

## 6. Interpret process results

Success writes one JSON document to stdout and exits `0`. Failures leave stdout empty, write a structured JSON error as the final stderr line, and exit non-zero.

For any non-zero exit, read [process contract](references/process-contract.md), branch by `error_kind` and `guidance`, and separate bad parameters, missing resources, retryable failures, and access/configuration failures.

Completion criterion: final answer separates retrieved evidence from missing data, partial delivery, extraction limitations, and configuration/access failures.
hi ther