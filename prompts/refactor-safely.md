# Refactor Safely

Refactor target:
[what and why]

Rules:
- Do not change behavior.
- Do not change public APIs.
- Do not change the database schema.
- Do not add dependencies.
- Do not rename exported classes or functions unless required.
- Preserve all tests; they must pass before and after without modification.
- Keep the diff minimal; one feature at a time.

Return: the diff, test results before and after.
