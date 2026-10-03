# AI-Assisted TDD Workflow

## Rules
- Human defines correctness.
- AI must not invent domain behavior.
- Non-trivial work requires a test strategy before implementation.
- A new project or major feature starts with a walking skeleton.
- User-visible, business, API or persisted-state changes require acceptance/behavior tests.
- Infrastructure, ORM, SQL or framework behavior requires integration tests.
- Pure deterministic logic may use unit tests.
- Write one failing test first.
- Confirm it fails for the right reason.
- Implement the minimum code to pass.
- Do not modify or weaken the test.
- Refactor only after green.
- Do not accept flaky tests.

## Before Coding, State
1. Test strategy decision
2. Reason
3. Test scenario list
4. First test to write
5. Real dependencies vs mocks/fakes
6. Missing business/domain rules (`TODO: verify`)
