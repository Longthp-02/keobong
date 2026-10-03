# Clarification Loop

Before acting on a non-trivial request, fill in the Prompt Intake Record:

```md
Goal:
[clear / missing / vague]

Context:
[clear / missing / vague]

Constraints:
[clear / missing / vague]

Output Format:
[clear / missing / vague]

Scope:
[simple / non-trivial / large / risky]

Risk:
[low / medium / high]

Missing Information:
[list only what matters]
```

Rules:
1. If nothing that affects correctness is missing, proceed.
2. Otherwise ask one targeted question per missing piece — never ask for a full rewrite.
3. Merge the answer into the original request and continue.
4. Unknowns that do not block progress become `TODO: verify`.

## Example

Original: "Fix this endpoint."

Clarification asked: "What should `POST /api/matches/{shareId}/slots` return when the slot is already taken, and what does it return now?"

Answer: "It should return 409 with the current slot state; now it returns 500."

Merged request:
- **Goal:** return 409 with the current slot state when a slot is already taken.
- **Context:** `slot` feature, claim endpoint; currently a constraint violation bubbles up as 500.
- **Constraints:** no schema change, no change to the success response.
- **Output format:** failing test first, minimal fix, test output.
