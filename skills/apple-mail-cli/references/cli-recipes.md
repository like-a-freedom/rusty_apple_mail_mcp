# CLI recipes

Use these examples after resolving the installed command from the user's environment. The command name `rusty_apple_mail_mcp` means either the executable found in `PATH` or the user-supplied installed binary path.

## Help

Run help before relying on remembered flags:

```bash
rusty_apple_mail_mcp --help
rusty_apple_mail_mcp search --help
rusty_apple_mail_mcp get-message --help
rusty_apple_mail_mcp get-attachment --help
```

Completion criterion: the installed command has confirmed the available global flags and subcommand flags.

## Account and mailbox discovery

List accounts before guessing account IDs, account selectors, or mailbox names:

```bash
rusty_apple_mail_mcp list-accounts
rusty_apple_mail_mcp list-accounts --include-mailboxes
```

If the task names an account, prefer startup Scope:

```bash
APPLE_MAIL_ACCOUNT="Exchange" rusty_apple_mail_mcp list-accounts
rusty_apple_mail_mcp --scope-account "Exchange" search --sender "person@example.com"
```

Completion criterion: every selector used later came from user input or CLI output.

## Search

Search requires at least one Filter:

```bash
rusty_apple_mail_mcp search --subject-query "invoice" --limit 20
rusty_apple_mail_mcp search --sender "person@example.com" --date-from "2026-01-01" --limit 100
rusty_apple_mail_mcp search --participant "person@example.com" --mailbox "Sent%20Items"
```

Close pagination for exhaustive tasks:

```bash
rusty_apple_mail_mcp search --sender "person@example.com" --limit 100 --offset 100
```

Completion criterion: narrow lookups justify selected message IDs from the shortlist; exhaustive lookups continue until `has_more` is false.

## Message windows

Read selected messages:

```bash
rusty_apple_mail_mcp get-message --message-id "12345"
rusty_apple_mail_mcp get-message --message-id "12345" --include-recipients
```

Continue partial windows with the returned `window.next_offset` and `window.source_revision`:

```bash
rusty_apple_mail_mcp get-message --message-id "12345" --offset 8192 --source-revision "..."
```

Completion criterion: any claim that depends on full content has all required windows closed.

## Attachment windows

Use attachment IDs from `get-message` output:

```bash
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0"
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0" --offset 8192 --source-revision "..."
```

Completion criterion: every attachment used as evidence is complete for delivery, explicitly partial with required continuations closed, or reported with extraction limitations.
