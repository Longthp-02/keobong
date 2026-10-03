# Prompting Rules

## Prompt Quality Gate
Every non-trivial prompt needs:
1. **Goal** — what outcome is wanted
2. **Context** — relevant files, current behavior, constraints from the domain
3. **Constraints** — what must not change
4. **Output Format** — what to return

If missing information affects correctness, ask only for the missing piece.
Do not ask the human to rewrite the whole prompt.
Remember the original request, merge the clarification into it, and continue when enough information exists.

## Safe Defaults
- Preserve public APIs unless explicitly changed.
- Preserve database schemas unless explicitly changed.
- Preserve existing behavior unless explicitly changed.
- Do not add dependencies.
- Do not modify unrelated files.
- Keep the diff minimal.
- Follow existing patterns.

## Stop Conditions
Stop and ask when:
- expected behavior is unknown
- a public API may change
- a database schema or migration may change
- auth, permissions, payments, security or privacy are involved
- multiple modules are touched and no plan exists
- a refactor is requested without a clear target
