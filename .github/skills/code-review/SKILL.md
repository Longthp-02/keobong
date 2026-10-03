---
name: code-review
description: Review a change against this project's architecture, security, testing and documentation rules.
---

# Code Review

Check each item and report concrete, file-specific findings:

- **Architecture match:** package-by-feature, ports before adapters, no framework types in domain, no cross-feature internals.
- **Silent failures:** no swallowed exceptions; errors logged with context.
- **Secrets:** none in code, config or logs; no bank account numbers in logs.
- **Auth/validation:** every host action checks ownership; all input validated.
- **Dependencies:** none added without justification; audit run.
- **Test quality:** right level, fails without the change, no weakened tests, concurrency covered for slot claiming.
- **Docs/context:** memory bank and handoffs updated.
- **Performance:** indexed queries, pagination, no N+1.
- **API contract:** matches the handoff note and the frontend's expectations.

Output format: `prompts/code-review.md`.
