# Code Review

Review this diff for:
- logic bugs
- security issues
- missing error handling
- broken edge cases
- incorrect assumptions
- regressions
- weak tests

Return a JSON array:

```json
[
  {
    "severity": "high | medium | low",
    "file": "backend/src/slots/service.rs",
    "issue": "what is wrong",
    "why_it_matters": "impact",
    "suggested_fix": "minimal fix"
  }
]
```

Return `[]` if there are no findings, then list areas you could not verify.
