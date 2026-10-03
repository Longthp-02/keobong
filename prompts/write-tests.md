# Write Tests

Target:
[class / endpoint / flow]

Rules:
- Test behavior, not implementation.
- Cover the happy path, important edge cases and error cases.
- Choose the level per `docs/testing-strategy.md` (real PostGIS via testcontainers for persistence).
- Follow the existing test style.
- Do not encode current broken behavior as expected.
- Do not change production code unless asked.

Return: the scenario list, the tests, and the run output.
