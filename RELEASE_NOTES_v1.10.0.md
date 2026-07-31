# Release Notes — v1.10.0

## Fixes

- **DOCX text extraction honors `<w:br/>` line breaks** — intra-paragraph line breaks in Word documents are now preserved as newlines instead of being dropped. This fixes glued words in subtitle-style DOCX attachments (e.g. `tableto` → `table\nto access`, `providesan` → `provides\nan instant`, `00:00:06,000Open` → timestamp and text on separate lines).
- Regression coverage added: `<w:br/>` in paragraphs, `<w:br/>` in table cells, and real-world subtitle content. Timestamps and words split across `<w:t>` runs remain intact (no spurious spaces inserted).
