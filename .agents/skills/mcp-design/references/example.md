# Worked example

A SIEM/XDR alert triage server, from the intent matrix to the final tool description.

**Input:** "Design an MCP server for a SIEM/XDR alert triage workflow"

**Output (abbreviated):**

### Server: `xdr-triage`

**Domain:** Alert investigation and triage for security operations
**Persona:** SOC analyst agent, automated triage pipeline
**Layer:** Capability

---

#### Tool: `xdr_triage_alert`

**Intent:** Get everything an agent needs to assess and triage a single alert in one call.

**Description:**
```
Retrieve full triage context for a security alert: severity, affected assets,
related events, MITRE ATT&CK mapping, and recommended next action.

Use this tool when: you need to assess whether an alert requires escalation,
suppression, or investigation.
Do NOT use this tool when: you need bulk alert statistics — use xdr_list_alerts instead.

Arguments:
- `alert_id` (string): Unique alert identifier from the alert feed.
- `include_raw_events` (bool, default false): Set true only when deep forensic
  context is required; increases response size significantly.

Returns: Triage bundle with severity, confidence, affected_hosts, mitre_techniques,
  recommended_action, and analyst_notes.
On error: {"status": "not_found", "guidance": "Call xdr_search_alerts(query=<host or rule name>) to locate the correct alert_id."}
```

#### Intent Coverage Matrix

| Intent | Tool | Calls | Verdict |
|---|---|---|---|
| Triage a single alert | `xdr_triage_alert` | 1 | ✅ |
| Find alerts by host | `xdr_search_alerts` | 1 | ✅ |
| Escalate to ticket | `xdr_escalate_alert` | 1 | ✅ |
| Get alert timeline + triage | `xdr_search_alerts` → `xdr_triage_alert` | 2 | ⚠️ consider composite |
