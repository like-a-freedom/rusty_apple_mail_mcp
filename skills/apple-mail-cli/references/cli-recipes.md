# CLI recipes

Exact command forms for the installed command. The command name `rusty_apple_mail_mcp` is either the
executable found in `PATH` or the installed binary path the user gave you.

## Help

```bash
rusty_apple_mail_mcp --help
rusty_apple_mail_mcp search --help
rusty_apple_mail_mcp get-message --help
rusty_apple_mail_mcp get-attachment --help
```

Done when: you know the global flags and the subcommand flags the installed binary accepts.

## Account and mailbox discovery

List accounts before you guess an account ID or a mailbox name:

```bash
rusty_apple_mail_mcp list-accounts
rusty_apple_mail_mcp list-accounts --include-mailboxes
```

Set the startup Scope when the task names an account:

```bash
APPLE_MAIL_ACCOUNT="Exchange" rusty_apple_mail_mcp list-accounts
rusty_apple_mail_mcp --scope-account "Exchange" search --sender "person@example.com"
```

Done when: every selector you use later came from `list-accounts` output or from the user.

## Search

```bash
rusty_apple_mail_mcp search --subject-query "invoice" --limit 20
rusty_apple_mail_mcp search --sender "person@example.com" --date-from "2026-01-01" --limit 100
rusty_apple_mail_mcp search --participant "person@example.com" --mailbox "Sent%20Items"
```

Continue while `has_more` is true:

```bash
rusty_apple_mail_mcp search --sender "person@example.com" --limit 100 --offset 100
```

Done when: a narrow lookup names only message IDs the result list justifies, and an exhaustive
lookup ends with `has_more` false.

## Message windows

```bash
rusty_apple_mail_mcp get-message --message-id "12345"
rusty_apple_mail_mcp get-message --message-id "12345" --include-recipients
```

Continue a partial window with the returned `window.next_offset` and `window.source_revision`:

```bash
rusty_apple_mail_mcp get-message --message-id "12345" --offset 8192 --source-revision "..."
```

Done when: every claim that depends on full content has all required windows closed.

## Attachment windows

Take attachment IDs from `get-message` output:

```bash
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0"
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0" --offset 8192 --source-revision "..."
```

Done when: every attachment you cite is complete for delivery, partial with its continuation
closed, or reported with its extraction limitation.
