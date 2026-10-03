# AI Reviewer — Strict Diff Review

Review this diff as a senior engineer.

Focus on:
- requirement mismatch
- invented behavior
- hardcoded secrets/config
- missing validation
- missing auth/authz
- IDOR bugs
- vulnerable or unnecessary dependencies
- silent failures
- weak, deleted or weakened tests
- architecture drift (see `docs/architecture.md`)
- performance risk
- unnecessary blast radius
- unnecessary renames/refactors
- missing docs/context updates
- frontend handoff mismatch (`docs/handoffs/`)

Severity:
- **P1** = must fix before merge
- **P2** = should review carefully / likely fix
- **P3** = optional cleanup/style

For each finding: severity, file, issue, why it matters, suggested fix.
