# Testing Strategy

## Principles
- Tests are the definition of done. A new test must fail without the implementation and pass with it.
- Human defines correctness; tests encode confirmed rules only.
- CI runs every test on every push and PR.

## Walking Skeleton
First slice proves DB → API → UI end-to-end with one backend acceptance test (real PostGIS via testcontainers) and one Playwright smoke test.

## Test Levels
| Change type | Test level |
|---|---|
| User-visible behavior, API contract, persisted state | Acceptance/behavior test through the HTTP API against a real database |
| Repository, SQL, PostGIS, migrations, router wiring | Integration test with real PostGIS (testcontainers) |
| Pure deterministic logic (fee split, level checks, VietQR payload) | Unit test |
| Frontend components | Vitest + Testing Library |
| Critical user flows | Playwright end-to-end |
| Slot claiming | Concurrency test: many parallel claims, exactly one success |
| Sensitive endpoints | Wrong-user test (403) for every host action |

## Real Dependencies vs Mocks
- Use a real PostGIS via testcontainers for anything touching persistence. No in-memory substitutes.
- Acceptance tests drive the Axum router in-process (`tower::ServiceExt::oneshot`).
- Fake external identity providers (Google/Zalo) behind the `IdentityProvider` port.
- Never mock the class under test's own collaborators inside the same feature unless they cross a port.

## Flaky Test Rule
A flaky test is a bug. Quarantine is not allowed; fix the root cause (usually time, ordering or shared state). Inject a `Clock`; never call `now()` directly in domain code.

## Test Quality Standards
- Test behavior, not implementation details.
- One reason to fail per test; descriptive names (`claimingTakenSlotReturnsConflict`).
- Cover happy path, important edges and error cases.
- Never weaken or delete a test to make code pass.
