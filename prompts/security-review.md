# Security Review

Review security-sensitive changes against `docs/threat-model.md` for:
- auth bypass
- authorization / ownership checks (host-only actions, own-slot-only actions)
- input validation
- injection (SQL, HTML in link previews)
- secret leakage
- unsafe logging (tokens, bank account numbers)
- sensitive data exposure
- insecure defaults
- broken session handling (OAuth state/nonce, cookie flags)

Return issues grouped as critical / high / medium / low, each with the minimal fix and the regression test to add.
