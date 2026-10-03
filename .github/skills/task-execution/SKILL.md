---
name: task-execution
description: Execute bug fixes, large features and frontend-affecting changes using the project's plan-first and test-first workflow.
---

# Task Execution

## Bug Workflow
1. Reproducing failing test first; confirm it fails.
2. Fix root cause minimally (`.github/skills/debugging/SKILL.md`).
3. Rerun the focused test, then the feature suite.

## Large Feature Workflow
1. Plan first (`prompts/plan-first.md`); get human approval.
2. Walking skeleton if the feature is new end-to-end.
3. One vertical step at a time (`prompts/implement-one-step.md`), each with its tests.
4. Update docs and memory bank.
5. Offer second-pass review.

## Frontend Handoff Workflow
When a backend change alters an endpoint, payload, validation, auth or error behavior:
1. Copy `docs/handoffs/_template.md` to `docs/handoffs/YYYY-MM-DD-title.md`.
2. Fill in example payloads from the actual tests.
3. Link it in the PR.

## Performance Check
Before claiming done: check new queries use indexes (`EXPLAIN` for geo and list queries), list endpoints are paginated, no N+1, no blocking calls on hot paths.
