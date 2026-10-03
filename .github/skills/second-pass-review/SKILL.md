---
name: second-pass-review
description: Run a strict second-pass AI review of uncommitted changes before merging.
---

# Second-Pass Review

Offer this after every implementation. Run it in a fresh AI session that did not write the code.

Copy/paste prompt:

```md
Review the code changes in this repository as a strict second-pass reviewer.

Context:
- This project prefers minimal, narrowly scoped changes.
- Do not suggest broad rewrites or unnecessary renames.
- Performance of the server is a priority.
- If requirements seem unclear, call that out explicitly instead of assuming.
- For bug fixes, the intended workflow is: reproducing test first, confirm it fails, fix root cause, rerun that test only, confirm it passes.
- For larger features, implementation may come before tests, but missing or weak test coverage should still be called out.

What I need from you:
1. Review the current uncommitted changes / diff.
2. Focus first on actual findings:
   - bugs
   - regressions
   - broken edge cases
   - API contract mismatches
   - auth / permission / validation problems
   - silent failure handling
   - performance risks
   - unnecessary blast radius
   - unnecessary renames / refactors
   - missing tests
   - missing docs / context updates
3. If backend changes affect frontend integration, also review any handoff note under `docs/handoffs/` and verify it matches the code changes.
4. Prefer concrete, file-specific feedback.
5. If there are no findings, say that explicitly and then list residual risks or areas not verified.

Output format:
- Findings first, ordered by severity.
- Include file paths and brief reasoning.
- Then list open questions / assumptions.
- Then a short summary of overall risk.
```
