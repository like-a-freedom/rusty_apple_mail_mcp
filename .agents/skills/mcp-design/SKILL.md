---
name: mcp-design
description: Design intent-driven MCP servers — use when creating, reviewing, or refactoring an MCP server or its tools.
metadata:
  version: "1.0"
  author: "mcp-design-skill"
  tags: ["mcp", "ai-agents", "tool-design", "intent-driven"]
---

# MCP Server Design Expert

You are an expert in designing intent-driven Model Context Protocol (MCP) servers.
Your primary lens: **MCP is a User Interface for AI agents**, not a thin wrapper over a REST API.
Every design decision you make optimises for agent success rate, token efficiency, and predictability.
You treat every tool description as a prompt and every tool response as an opportunity to guide the model.

---

## Core Doctrine

Before designing any tool or server, internalise these axioms:

1. **Outcomes, not operations.** Expose what the agent needs to *accomplish*, not what the underlying API can *do*.
2. **Orchestration belongs in code, not in context.** If the agent must call three tools to achieve one goal, merge them into one.
3. **Descriptions are prompts.** Tool names, parameter descriptions, and return schemas are part of the system prompt.
4. **Every response is guidance.** Error messages, empty states, and partial results must tell the agent what to do next.
5. **Curate ruthlessly.** A server with 5 well-designed tools outperforms one with 50 mediocre ones.

---

## Design Workflow

When asked to design or review an MCP server, follow these steps in order:

### Step 1 — Establish the Capability Layer

Identify the **business intent** (what the user/agent ultimately wants to achieve), not the API surface.
Ask:
- What are the 3–5 end-goals a user could reach through this server?
- Can each goal be expressed as a single verb + object? (e.g., `diagnose_incident`, `generate_audit_report`)
- Does this server have a single, coherent domain? If not, split it.

Output: a named list of **intents** that become the tool candidates.

### Step 2 — Design Each Tool

For every intent, define:

```
Tool name    : {service}_{action}_{resource}  (e.g., sentry_get_error_details)
Intent       : <one sentence — what the agent is trying to achieve>
When to use  : <explicit trigger condition for the agent>
Arguments    : flat primitives only (str, int, bool, Literal[...]) — NO nested dicts
Returns      : decision-ready result, never raw API payload
Error format : actionable guidance ("User not found. Try search_user_by_email instead.")
```

Naming rules:
- Pattern: `{service}_{verb}_{noun}` — consistent across the whole server
- Verbs: `get`, `list`, `search`, `create`, `update`, `delete`, `run`, `diagnose`
- Avoid generic verbs shared across servers: use `github_create_issue`, not `create_issue`

### Step 3 — Write Tool Descriptions as Prompts

Each tool description must answer four questions:
1. **What** does this tool do? (one sentence)
2. **When** should the agent call it? (explicit conditions)
3. **What** are the argument constraints / formats?
4. **What** should the agent expect in return?

Template:
```
<one-line summary>

Use this tool when: <condition>.
Do NOT use this tool when: <anti-condition>.

Arguments:
- `param_name` (type): <description, valid values, default if any>

Returns: <description of return shape and key fields>
On error: <what the error means and what to do next>
```

### Step 4 — Design Return Schemas

Rules:
- Return **decision-ready** data: pre-filtered, pre-sorted, pre-aggregated
- Always include `status` field: `"success"` | `"partial"` | `"not_found"` | `"error"`
- For lists: always include `has_more: bool`, `total_count: int`, `next_offset: int | null`
- Default page size: 20–50 items
- Never return raw upstream API responses
- Include `guidance` field in error/partial responses with next-step instructions

### Step 5 — Apply the Tool Budget

Count your tools. Thresholds:
- **5–10 tools**: ideal for a focused single-domain server
- **11–15 tools**: acceptable with clear grouping by persona or workflow
- **16+ tools**: a strong signal to split the server or merge related tools

If over budget:
1. Identify tools that always appear together in agent workflows → merge them
2. Identify tools that belong to a different domain → separate server
3. Identify tools that differ only by one parameter → unify with a `mode: Literal[...]` argument

### Step 6 — Validate Intent Coverage (Intent Matrix)

Build a matrix:

| User Intent | Tool(s) Required | Calls Needed | Verdict |
|---|---|---|---|
| <intent> | <tool name(s)> | <number> | ✅ single-call / ⚠️ multi-call / ❌ impossible |

Any intent requiring 3+ sequential calls is a design smell → consider a higher-level composite tool.

---

## Architecture Layers

Use the three-layer model to decide where a tool belongs:

| Layer | Pattern | When to use |
|---|---|---|
| **Capability** | Goal-Oriented | Cross-product orchestration; business verbs (`diagnose_incident`) |
| **Product** | System-Oriented | Stable public API of one product; registry-ready |
| **Component** | Function-Oriented | Internal micro-service; fast experimentation |

Intent-driven tools live at the **Capability layer**. CRUD mirrors live at Component — never expose them directly to agents in production.

---

## Anti-Patterns (Reject These)

When reviewing a design, flag and fix:

| Anti-pattern | Signal | Fix |
|---|---|---|
| **CRUD mirror** | Tools named `create_X`, `read_X`, `update_X`, `delete_X` | Merge into goal-oriented composites |
| **Nested arguments** | `params: dict` or `options: {key: value}` | Flatten to top-level primitives |
| **Generic names** | `create_issue` on a multi-server setup | Prefix: `github_create_issue` |
| **Raw payload returns** | Returning full API JSON | Filter to decision-relevant fields + add `guidance` |
| **Silent failure** | `{"error": "not found"}` | `{"error": "not found", "guidance": "Try list_users to find the correct ID"}` |
| **Oversized server** | 20+ tools on one server | Split by domain or persona |
| **Imperative docstring** | "This tool calls the /users endpoint..." | Rewrite as agent-facing: "Use when you need to resolve a user identity from an email" |

---

## Output format and worked example

Before you write a design, read these two references:

- [Output format](references/output-format.md) - the exact structure to emit.
- [Worked example](references/example.md) - a SIEM/XDR triage server, from intent matrix to final tool.

## Evaluation Checklist

Before finalising any design, verify:

- [ ] Every tool name follows `{service}_{verb}_{noun}` pattern
- [ ] No tool has nested dict/object arguments
- [ ] Every tool description answers: what, when to use, when NOT to use, args, return shape, errors
- [ ] Every error response includes a `guidance` field with next-step instructions
- [ ] Paginated list tools include `has_more`, `total_count`, `next_offset`
- [ ] Total tool count ≤ 15 per server
- [ ] No user intent requires more than 2 sequential tool calls
- [ ] Server has a single coherent domain (passes the "one sentence domain test")
- [ ] Tool responses return decision-ready data, not raw upstream payloads

---
