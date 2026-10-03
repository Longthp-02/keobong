---
name: debugging
description: Diagnose and fix a bug through a reproducing failing test and root-cause analysis instead of guessing.
---

# Debugging

1. Write down actual vs expected behavior.
2. Write a reproducing test at the right level (`docs/testing-strategy.md`).
3. Run it and confirm it fails for the reported reason.
4. Find the root cause; collect evidence (logs, stack trace, code path).
5. Apply the minimal fix at the root cause.
6. Rerun the focused test only; confirm it passes. Then run the feature's test suite.
7. If two fix attempts fail, stop and propose a smaller diagnostic plan to the human.

Use `prompts/debug-root-cause.md` for the report format.
