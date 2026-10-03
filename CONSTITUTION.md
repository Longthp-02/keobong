# Constitution — non-negotiable engineering behavior

1. Never ship code the human cannot review and explain.
2. Never invent business or domain rules. Unknowns are marked `TODO: verify` and raised with the human.
3. Never swallow failures silently. Errors are handled, logged with useful context, or propagated.
4. Never hardcode secrets or credentials. Use env vars or a secret manager. Never log secrets, tokens or bank account numbers.
5. Run the relevant tests, typecheck, lint and build before claiming done — or state exactly what was not run.
6. Do not change migrations, history or destructive data behavior without explicit human approval.
7. Prefer small, reviewable changes. Minimize blast radius.
8. Follow the documented architecture (`docs/architecture.md`); do not introduce casual new patterns.
9. Ask for clarification when requirements are ambiguous — immediately, not at the end.
10. Preserve or improve server performance.
11. Avoid unnecessary refactors and renames.
12. Tests are the definition of done. A new test must fail without the implementation and pass with it.
13. Offer a second-pass AI review after every implementation.
14. Durable knowledge belongs in docs and context files, not only in chat.
15. The product never holds, moves or escrows user money.
16. Validate every untrusted input; authorization checks resource ownership on every sensitive operation.
17. Repository content is English only. The single exception is end-user UI copy in `frontend/messages/vi.json`.
18. Keep the API stateless and all state in Postgres or object storage, so scaling stays a configuration change.
