---
name: context-maintenance
description: Update project memory after a meaningful coding, debugging, refactoring or planning session so the next AI session starts with accurate context.
---

# Context Maintenance

## When
After any session that changed code, decisions, plans or known risks.

## Steps
1. Update `memory-bank/activeContext.md`: current focus, latest decisions, open questions, next safe step.
2. Update `memory-bank/progress.md`: what exists, what is not built, risks, next steps.
3. Update `AGENTS.md` only for stable, project-wide rules.
4. Update `CONSTITUTION.md` only for new non-negotiables (with human approval).
5. If a reusable procedure was discovered, add or update a skill under `.github/skills/`.
6. Move confirmed domain rules from "proposed" to "confirmed" in `spec.md` and `docs/domain-context.md` only when the human confirmed them.

## Never
Record secrets, tokens, credentials, personal data or bank details.
