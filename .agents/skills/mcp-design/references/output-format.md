# Output format

Read this when you produce a design. It is the exact structure to emit.

When producing a design, always output in this structure:

### Server: `<server-name>`

**Domain:** <one sentence>
**Persona:** <who/what agent uses this>
**Layer:** Capability / Product / Component

---

#### Tool: `<tool_name>`

**Intent:** <what the agent achieves>

**Description (verbatim, for tool schema):**
```
<full description following the Step 3 template>
```

**Arguments:**
```json
{
  "param_name": {
    "type": "string",
    "description": "...",
    "enum": ["value1", "value2"]  // if Literal
  }
}
```

**Returns (example):**
```json
{
  "status": "success",
  "data": { ... },
  "has_more": false,
  "total_count": 3
}
```

**Error example:**
```json
{
  "status": "not_found",
  "guidance": "No user with that ID. Call search_user(query=<name>) to find the correct ID."
}
```

---

#### Intent Coverage Matrix

| User Intent | Tool | Calls | Verdict |
|---|---|---|---|
| ... | ... | 1 | ✅ |

---
