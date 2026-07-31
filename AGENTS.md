# AGENTS.md — Rust Development Constitution

> This file defines the rules, principles, and idioms that ALL code in this
> repository must follow. It applies to human contributors and AI coding agents
> equally. When in doubt, consult The Rust Programming Language Book
> (https://doc.rust-lang.org/book/) — the canonical reference for every
> rule below.

---

## Code Navigation Rules

ALWAYS use octocode MCP tools before reading files directly:
- Use `semantic_search` to find relevant code by meaning
- Use `view_signatures` to understand file structure
- Use `graphrag` to explore dependencies between files
- Use `structural_search` for AST-level pattern search (replaces grep/rg)

NEVER:
- Run grep, rg, find to locate code — use semantic_search instead
- Read entire files to understand structure — use view_signatures instead
- Guess file locations — use graphrag overview first

WORKFLOW for any task:
1. graphrag overview → understand project structure
2. semantic_search → find relevant files
3. view_signatures → inspect structure of found files
4. Read only specific sections if needed

## 0. Meta-Rules for AI Agents

- **Read this file fully before writing or refactoring any code.**
- **Do NOT change public APIs without explicit user instruction.**
- **Do NOT introduce new dependencies without approval.**
- **Do NOT use `unsafe` unless explicitly requested and justified in comments.**
- **Do NOT fix formatting issues — `rustfmt` handles that automatically.**
- **Every change must compile cleanly:** `cargo check && cargo clippy --all-targets --all-features -- -D warnings`
- **Preserve behavior.** Refactoring must not change observable output or semantics.
- **Always use `#[serde(deny_unknown_fields)]` on all tool param structs** so the JSON Schema shows `additionalProperties: false` — unknown fields are caught by serde, not silently ignored.
- **Map ALL errors to native rmcp error types** via `From<MailMcpError> for McpError` in `error.rs`:
  - `Validation(msg)` → `McpError::invalid_params(msg)` — tool input validation failures
  - `MessageNotFound` / `AttachmentNotFound` → `McpError::invalid_params(...)` — resource not found
  - Everything else → `McpError::internal_error(...)` — server/database failures
- **Tool functions return `Err(MailMcpError)`, not `Ok(Response::error(...))`** — the generic `call_typed_tool` handler auto-converts via `?`. Never wrap errors in a successful `Ok` response body.

### Quality Gate (mandatory before every commit)

> **Zero tolerance for warnings, formatting drift, or test failures.**
> CI is the minimum, not the goal — catch everything locally first.

```bash
# 1. Format — MUST pass before anything else
cargo fmt --all -- --check

# 2. Lint — zero warnings tolerated (all-targets covers test code, matching CI)
cargo clippy --all-targets --all-features -- -D warnings

# 3. Test — all tests must pass, no exceptions
cargo test

# 4. Docs — documentation must build clean
cargo doc --no-deps 2>&1 | grep -q "^error" && exit 1

# 5. Full check — final sanity
cargo check
```

**Rules:**
- **Never** push code that produces a single clippy warning, compiler warning, or `dbg!()` / `todo!()` / `unimplemented!()`.
- **Always** run `cargo fmt --all` before committing. If CI fails on formatting, the commit is rejected.
- **All 4 gates** (fmt, clippy, test, doc) must pass before any push. No exceptions.
- If a test is flaky due to global state, **fix it** with a serialisation lock — never silence or ignore it.
- `#[allow(...)]` attributes are forbidden unless explicitly approved in a code review.

---

## 1. Project Setup

### Edition & Toolchain

```toml
# Cargo.toml
[package]
edition = "2024"  # Always use the latest stable edition (Rust 2024 as of 1.85+)
resolver = "3"
```

#### Required tooling

```bash
# Before every commit
cargo fmt            # Format all code
cargo clippy --all-targets --all-features -- -D warnings  # Treat all warnings as errors (incl. test code)
cargo test           # All tests must pass
cargo doc --no-deps  # Docs must build without warnings
```

#### Recommended dependencies (approved for use without asking)

| Crate | Purpose |
|---|---|
| rmcp | MCP Server framework (server, macros, transport-io) |
| tokio | Async runtime |
| serde + serde_json | Serialization/deserialization |
| thiserror | Derive Error for library error types |
| anyhow | Error handling at application boundaries only |
| schemars | JSON Schema generation from Rust types (paired with serde) |
| tracing + tracing-subscriber | Structured logging |
| rusqlite | SQLite access for Apple Mail envelope index |
| chrono | Date/time handling (with serde feature) |
| clap | CLI argument parsing |
| mail-parser | Email message parsing (emlx) |
| dirs | Platform-specific home directory resolution |
| walkdir | Filesystem recursion for mailbox discovery |
| lru | LRU cache for message/attachment lookups |
| scraper | HTML email content extraction |
| zip | .docx support (deflate feature only) |
| quick-xml | .xlsx support (serialize feature) |
| csv | CSV attachment parsing |
| lopdf | PDF attachment parsing |
| once_cell | Lazy-initialized globals |
| tempfile | Temporary directories in tests (dev-dependency only) |

## 2. Ownership & Borrowing
Rust Book Ch. 4: https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html

Rules
Borrow, don't clone. Clone only when ownership transfer is genuinely needed.

Pass references, not owned values unless the function must take ownership.

Prefer &str over &String and &[T] over &Vec<T> in function signatures —
slices are strictly more flexible.

Use Cow<'_, str> when a function may or may not need to allocate.

Use std::mem::take or std::mem::replace to move out of a struct field
without cloning.

rust
// ✅ GOOD — accept a slice, works with Vec<T>, arrays, and slices
fn process(items: &[Item]) { ... }

// ❌ BAD — forces callers to have a Vec specifically
fn process(items: &Vec<Item>) { ... }

// ✅ GOOD — borrow the string
fn greet(name: &str) { ... }

// ❌ BAD — forces an allocation at the call site
fn greet(name: &String) { ... }

### Lifetimes

Add explicit lifetime annotations only when the compiler cannot infer them.

Prefer returning owned values over complex lifetime-annotated references
when the lifetime logic is non-obvious.

Use named lifetimes ('a, 'conn, 'ctx) that carry semantic meaning.

## 3. Error Handling
Rust Book Ch. 9: https://doc.rust-lang.org/book/ch09-00-error-handling.html

### Rules

Result<T, E> for all recoverable errors. Never use exceptions or boolean return codes.

panic! only for unrecoverable bugs — violated invariants, impossible
states, programmer errors. Never panic! in library code on bad input.

No .unwrap() or .expect() in production/library code. Use ? for
propagation. Reserve .expect("reason") for test code and startup
initialization where failure is truly unrecoverable.

Define domain-specific error enums for libraries.
Use thiserror::Error derive macro.

Use anyhow::Result only at application boundaries (e.g., main, CLI
handlers, integration layer) — never in library crates.

Use MailMcpError (thiserror enum in `error.rs`) for all domain errors.

```rust
// ✅ GOOD — project's own error type
#[derive(Debug, Error)]
pub enum MailMcpError {
    #[error(
        "Envelope Index database not found at: {path}. \
         Ensure Apple Mail is configured with at least one email account, \
         or set APPLE_MAIL_DIR and APPLE_MAIL_VERSION to the correct path."
    )]
    DatabaseNotFound { path: PathBuf },
    #[error("Database is locked by another process (Apple Mail may be running): {0}")]
    DatabaseLocked(String),
    #[error("SQLite query failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Message {id} not found in the index")]
    MessageNotFound { id: String },
    #[error("Attachment {id} not found for message {message_id}")]
    AttachmentNotFound { id: String, message_id: String },
    #[error("Email body file not found on disk: {path}")]
    BodyFileNotFound { path: PathBuf },
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Validation(String),
}
```

### Error wrapping hierarchy

```text
main() / CLI handler
  └── anyhow::Result<()>        ← application boundary, context via .context()
        └── MailMcpServer / tools
              └── MailMcpError    ← typed, structured, using thiserror
                    └── rusqlite::Error, std::io::Error, serde_json::Error, etc.
```

### MCP-specific error rules

- **Tool parameter validation errors** (serde deserialization failures like unknown fields, wrong types):
  return `Err(McpError::invalid_params(...))` — serde's `DeserializeOwned` catches these
  generically in `call_typed_tool` at `src/server/handler.rs:95-96`.

- **Unknown tool name**: `Err(McpError::invalid_request(...))` — protocol-level error.

- **Domain errors** (validation failures, resource not found, DB locked):
  return `Err(MailMcpError)` — auto-converted to native `McpError` via
  `From<MailMcpError> for McpError` in `error.rs`.

- **Implementation**: `MailMcpError` has a `From<MailMcpError> for rmcp::ErrorData` impl
  that maps each variant:
  - `Validation(msg)` → `McpError::invalid_params(msg, None)` — bad input
  - `MessageNotFound { id }` / `AttachmentNotFound { id, message_id }` → `McpError::invalid_params(...)` — resource missing
  - Everything else → `McpError::internal_error(...)` — server/IO/database failures

## 4. Types & Data Modeling

### Make illegal states unrepresentable

Use the type system to eliminate invalid states by construction.

Rust Book Ch. 6: https://doc.rust-lang.org/book/ch06-00-enums.html

rust
// ❌ BAD — boolean blindness, easy to mix up arguments
fn connect(use_tls: bool, use_compression: bool) { ... }

// ✅ GOOD — self-documenting, impossible to pass wrong values
enum TlsMode { Enabled, Disabled }
enum Compression { Enabled, Disabled }
fn connect(tls: TlsMode, compression: Compression) { ... }

#### Newtype pattern (SOLID: Single Responsibility, DRY)

Wrap primitive types in domain-specific newtypes. This prevents mixing up
values of the same underlying type.

rust
// ❌ BAD — what's the difference between these u64s?
fn transfer(from: u64, to: u64, amount: u64) { ... }

// ✅ GOOD — each concept has its own type
struct UserId(u64);
struct AccountId(u64);
struct Cents(u64);
fn transfer(from: AccountId, to: AccountId, amount: Cents) { ... }

### Option<T>

Use `Option<T>` to represent optionality. Never use sentinel values (`-1`, `""`, `0`) as "no value."

Chain with .map(), .and_then(), .unwrap_or_else().

Prefer if let Some(x) = opt { ... } over .unwrap().

## 5. Traits & Generics
Rust Book Ch. 10: https://doc.rust-lang.org/book/ch10-00-generics.html

SOLID: Interface Segregation + Dependency Inversion
Define small, focused traits — each trait has one responsibility.

Depend on trait abstractions, not concrete types.

Accept impl Trait in argument position for flexibility (static dispatch,
no heap allocation).

Use Box<dyn Trait> for runtime polymorphism when you need to store
heterogeneous types.

rust
// ✅ GOOD — small, focused trait; testable via mock implementations
trait EventSink {
    fn emit(&mut self, event: Event) -> Result<(), SinkError>;
}

// ✅ GOOD — depends on abstraction, not concrete type
fn process_events(sink: &mut impl EventSink, events: &[Event]) -> Result<(), SinkError> {
    for event in events {
        sink.emit(event.clone())?;
    }
    Ok(())
}

// ❌ BAD — hard-coded dependency, impossible to test without real Kafka
fn process_events(kafka: &KafkaProducer, events: &[Event]) { ... }

### Standard trait implementations

Always implement these standard traits when appropriate:

| Trait | When |
|---|---|
| Debug | Always, on every type |
| Clone | When value copying is semantically meaningful |
| PartialEq / Eq | When equality comparison makes sense |
| Display | For user-facing strings (never Debug in logs/UI) |
| From<T> / Into<T> | For idiomatic type conversions |
| Default | When a sensible zero-value exists |
| Iterator | When type represents a sequence |

## 6. Pattern Matching

Rust Book Ch. 18: https://doc.rust-lang.org/book/ch18-00-patterns.html

Prefer match over if/else chains for enum values.

Use exhaustive matching — never use _ => as a catch-all unless
genuinely needed, as it hides newly added variants.

Use if let for single-branch pattern matching.

Use while let for iterating over fallible results.

rust
// ✅ GOOD — exhaustive, compiler enforces new variants
match status {
    Status::Active   => handle_active(),
    Status::Inactive => handle_inactive(),
    Status::Pending  => handle_pending(),
}

// ❌ BAD — silently swallows new variants
match status {
    Status::Active => handle_active(),
    _ => {}
}

// ✅ GOOD — concise single-case matching
if let Some(user) = find_user(id) {
    greet(&user);
}

## 7. Functions & Methods

### KISS Principle

Functions ≤ 30 lines. If longer, extract sub-functions.

One function, one responsibility. If you need "and" to describe it, split it.

Max 4 parameters. More than 4 → group into a config/params struct.

Avoid deeply nested code. Use early returns, ?, or match to flatten.

rust
// ❌ BAD — 5 params, boolean flag that controls behavior
fn create_user(name: String, email: String, age: u32,
               is_admin: bool, send_welcome: bool) -> Result<User, UserError> { ... }

// ✅ GOOD — params in a struct, clear semantics
#[derive(Debug)]
pub struct CreateUserParams {
    pub name: String,
    pub email: String,
    pub age: u32,
    pub role: UserRole,
}

pub fn create_user(params: CreateUserParams) -> Result<User, UserError> { ... }
pub fn send_welcome_email(user: &User) -> Result<(), MailError> { ... }

### Naming conventions

| Item | Convention | Example |
|---|---|---|
| Functions / methods | `snake_case` verb phrase | `parse_config`, `validate_input` |
| Types / traits | `CamelCase` | `UserRepository`, `EventSink` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_RETRY_COUNT` |
| Boolean predicates | `is_`, `has_`, `can_`, `should_` | `is_valid()`, `has_permission()` |
| Conversion (cheap, reference) | `as_` | `as_str()` |
| Conversion (allocating/owned) | `to_` | `to_string()` |
| Consuming conversion | `into_` | `into_bytes()` |

Iterator Chains over Manual Loops (DRY)

Prefer declarative iterator combinators over imperative for loops when
the intent is transforming or filtering data.

rust
// ❌ BAD — verbose, imperative
let mut result = Vec::new();
for item in items {
    if item.is_active() {
        result.push(item.name.clone());
    }
}

// ✅ GOOD — declarative, composable, no mutation
let result: Vec<_> = items
    .iter()
    .filter(|item| item.is_active())
    .map(|item| item.name.clone())
    .collect();

## 8. Module Structure & Visibility
Rust Book Ch. 7: https://doc.rust-lang.org/book/ch07-00-managing-growing-projects-with-packages-crates-and-modules.html

### Actual project structure

```text
src/
├── lib.rs              ← re-exports public API only; declares all modules
├── main.rs             ← entry point; thin, delegates to runner
├── cli.rs              ← CLI subcommands (via clap)
│   └── commands.rs
├── config.rs           ← MailConfig: loading/validation
├── accounts.rs         ← AccountMetadata domain type
├── domain.rs           ← re-exports: AttachmentContent, AttachmentMeta, ContentFormat,
│   ├── message.rs      ←   MessageFull, MessageMeta, MessageSummary, RichTextMessage
│   └── attachment.rs
├── error.rs            ← MailMcpError enum (thiserror)
├── db.rs               ← re-exports for SQLite access
│   ├── connection.rs   ←   EnvelopeIndex connection management
│   ├── accounts.rs     ←   account queries
│   ├── mailboxes.rs    ←   mailbox queries
│   ├── messages.rs     ←   message queries
│   └── epoch.rs        ←   Mac absolute time conversion
├── mail.rs             ← re-exports for mail file handling
│   ├── cache.rs        ←   LRU cache for parsed messages
│   ├── locator.rs      ←   emlx file path resolution
│   ├── parser.rs       ←   emlx parsing
│   ├── extractor.rs    ←   HTML text extraction
│   ├── docx.rs         ←   .docx → text
│   ├── xlsx.rs         ←   .xlsx → text
│   ├── pptx.rs         ←   .pptx → text
│   └── pdf.rs          ←   .pdf → text
├── server.rs           ← re-exports for MCP server
│   └── handler.rs      ←   MailMcpServer, call_typed_tool, tool_dispatch
│       └── tools/      ←   tool implementations (one file per tool)
│           ├── search_messages.rs  ← SearchMessagesParams
│           ├── get_message.rs      ← GetMessageParams
│           ├── get_attachment.rs   ← GetAttachmentParams
│           ├── list_accounts.rs    ← ListAccountsParams
│           ├── list_mailboxes.rs
│           └── message_lookup.rs   ← shared message lookup helpers
└── runner.rs           ← wire everything together; start MCP server
```

### Visibility Rules (Principle of Least Privilege)

- `pub` — only for items that are part of the public API (re-exported from `lib.rs`).
- `pub(crate)` — for items shared across the crate but not exported.
- `pub(super)` — for items needed only by the parent module.
- Default (private) — for implementation details.

## 9. MCP Tool Patterns

### Tool parameter structs

Every MCP tool has a dedicated `Params` struct. Every param struct must have:

```rust
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SomeToolParams {
    pub field: String,
    #[serde(default)]
    pub optional_field: Option<String>,
}
```

- `deny_unknown_fields`: client sends `searchText` alongside valid `subject_query` → serde rejects, user gets a clear "Invalid parameters" message listing valid fields.
- `JsonSchema`: auto-generates the tool's input schema for the client.
- `#[serde(default)]`: for optional fields.

### EmptyToolParams

Tools that accept no arguments use a zero-sized struct:

```rust
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyToolParams {}
```

Used in `handler.rs` for `list_mailboxes`.

### call_typed_tool pattern

All tools route through a single generic dispatcher in `handler.rs`:

```rust
fn call_typed_tool<TParams, TResponse, F>(
    &self,
    arguments: Map<String, Value>,
    tool_fn: F,
) -> Result<CallToolResult, McpError>
where
    TParams: DeserializeOwned,
    TResponse: Serialize,
    F: FnOnce(&MailConfig, TParams) -> Result<TResponse, MailMcpError>,
```

- **Deserialization error** → `McpError::invalid_params` with serde message.
- **Business logic error** → `McpError::internal_error` with the error message.
- **Success** → `CallToolResult::success` with JSON content.

### validate_params pattern

Tools with complex validation (e.g. `search_messages`) should have a
`validate_params` function that returns a `Result<(), &'static str>`.
Called before the main query to fail early with actionable guidance.

```rust
fn validate_params(params: &SearchMessagesParams) -> Result<(), &'static str> {
    if params.sender.is_some()
        && !params.subject_query.is_some()
        ...
    {
        // ... validation logic
    }
    Ok(())
}
```

## 10. YAGNI — You Aren't Gonna Need It

Remove all of the following unless they are actively used:
- Dead code (`#[allow(dead_code)]` is a warning sign, not a solution)
- Unused imports (use statements that do nothing)
- Feature flags for features not yet planned
- Trait implementations "for future use"
- Generic type parameters with no constraints used
- `pub` visibility on items only used in one place

```rust
// ❌ BAD — generic over E when only one error type ever exists
pub struct Processor<E: std::error::Error> { ... }

// ✅ GOOD — YAGNI; generalize only when you have a second concrete use case
pub struct Processor { ... }
```

## 11. Testing

Rust Book Ch. 11: https://doc.rust-lang.org/book/ch11-00-testing.html

### Structure

Unit tests live in the same file as the code they test:
`#[cfg(test)] mod tests { ... }`

Integration tests live in `tests/` directory.

Doc tests in `///` comments demonstrate public API usage.

```rust
// In src/domain/user.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_user_has_active_status() {
        let user = User::new("alice", "alice@example.com");
        assert_eq!(user.status, Status::Active);
    }

    #[test]
    fn user_from_invalid_email_returns_error() {
        let result = User::new("bob", "not-an-email");
        assert!(matches!(result, Err(UserError::InvalidEmail { .. })));
    }
}
```

### Tool handler tests

Test tool error responses via JSON serialization, not tight coupling to
`Content` enum variants:

```rust
let call_result = handler.call_tool_by_name("search_messages", args).await;
let json = serde_json::to_string(&call_result).unwrap();
assert!(json.contains("isError"), "should indicate error: {json}");
assert!(json.contains("Invalid parameters"), "should have guidance: {json}");
```

### Testability Rules

- Pure functions are the easiest to test — prefer them for business logic.
- Inject dependencies via traits, not concrete types — allows mock implementations in tests.
- Never test implementation details — test observable behavior.
- Name tests descriptively: `{unit}_when_{condition}_then_{expected_outcome}`

```rust
// ✅ GOOD — test name is self-documenting
#[test]
fn create_user_when_email_is_duplicate_then_returns_conflict_error() { ... }

// ❌ BAD — what does this test?
#[test]
fn test_create() { ... }
```

### Test Isolation

- Use the builder pattern for test fixtures.
- Never rely on external services in unit tests. Mock via trait implementations.
- Use `#[tokio::test]` (not `#[test]`) for async test functions.

## 12. Documentation

### Rules

Every `pub` item (struct, enum, trait, function, constant) MUST have a
`///` doc comment.

Doc comments explain WHAT the item does, not HOW it does it.

Include `# Examples` section for non-trivial public functions.

Use `# Errors` section in `///` for functions returning `Result`.

Use `# Panics` section if the function can panic.

Internal implementation comments use `//` and explain WHY, not what.

```rust
/// Parses a [`Config`] from a TOML file at the given path.
///
/// # Errors
///
/// Returns [`ConfigError::Io`] if the file cannot be read, or
/// [`ConfigError::Parse`] if the TOML is malformed.
///
/// # Examples
///
/// ```rust
/// let config = Config::from_file(Path::new("config.toml"))?;
/// assert_eq!(config.port, 8080);
/// # Ok::<(), ConfigError>(())
/// ```
pub fn from_file(path: &Path) -> Result<Config, ConfigError> { ... }
```

## 13. Performance Awareness (without premature optimization)

Apply these rules always — they are zero-cost good habits, not premature
optimization:

- Prefer iterator chains over collecting into Vec mid-computation.
- Use `Vec::with_capacity(n)` when the final size is known.
- Use `String::with_capacity(n)` when building strings in a loop.
- Prefer `&str` slices over `String` in function params.
- Avoid redundant clones — every `.clone()` must be justified.
- Use `std::mem::take` to move values out of structs without cloning.

```rust
// ✅ GOOD — no intermediate allocation
let sum: u64 = records.iter().map(|r| r.value).sum();

// ❌ BAD — allocates a whole Vec just to sum
let values: Vec<u64> = records.iter().map(|r| r.value).collect();
let sum: u64 = values.iter().sum();
```

## 14. Design Principles Summary

| Principle | Rust application |
|---|---|
| KISS | Functions ≤30 lines; ≤4 params; no speculative abstractions |
| DRY | Extract repeated logic to functions, generics, or macros; use trait defaults |
| YAGNI | Remove dead code; no feature flags for unplanned features; no premature generics |
| SRP | One concept per module, struct, and function |
| OCP | Extend via new trait impls, not by modifying existing types |
| LSP | Trait impls honor the full contract; never silently no-op a required method |
| ISP | Small, focused traits; split fat traits into composable ones |
| DIP | Accept impl Trait or Box<dyn Trait>; never depend on concrete implementations |

## 15. Anti-Patterns Checklist

Before submitting any code, verify that NONE of these are present:

- `.unwrap()` in non-test, non-startup code
- `.clone()` without a comment justifying ownership need
- `&Vec<T>` or `&String` in function parameters
- `pub` on an item used only within one module
- Function longer than 30 lines without a compelling reason
- Struct with more than one unrelated responsibility
- `_ =>` wildcard match on a project-owned enum
- `#[allow(dead_code)]` or `#[allow(unused)]` without a TODO
- `unsafe` block without a safety comment
- Boolean parameters that flip function behavior
- Bare `panic!` in library code called with user input
- Missing `///` doc comment on any pub item
- Param struct without `#[serde(deny_unknown_fields)]`
- Returning `Ok(Response::error(...))` for tool-level validation errors (return `Err(MailMcpError::Validation(...))` instead)
