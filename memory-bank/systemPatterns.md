# System Patterns

- **Architecture direction:** modular monolith. Backend modules by feature: `matches`, `slots`, `players`, `auth`, `payment`, `share`. See `docs/architecture.md`.
- **Vertical slices:** each change delivers one behavior end-to-end inside one feature.
- **Ports and adapters:** domain defines interfaces (e.g. `MatchRepository`, `QrPayloadGenerator`, `IdentityProvider`); adapters implement them with sqlx, HTTP clients (reqwest) and Axum extractors.
- **Module boundaries:** a feature exposes a small public API class; other features never touch its entities or repositories.
- **TDD workflow:** state test strategy first; one failing test; minimal code; refactor on green. See `prompts/tdd-ai-workflow.md`.
- **Debugging workflow:** reproduce with a failing test, find root cause, minimal fix. See `.github/skills/debugging/SKILL.md`.
- **Performance priority:** geo queries use a GiST index; list endpoints are paginated; no N+1; keep startup instant for scale-to-zero.
- **Scale-ready rules:** stateless API, state only in Postgres/object storage, CDN-cacheable public pages (see `AGENTS.md`).
- **Concurrency:** slot claiming is enforced by the database (unique constraint / conditional update), not by application checks alone.
- **Second-pass review:** offer after every implementation (`.github/skills/second-pass-review/SKILL.md`).
- **Frontend handoff:** any API contract change gets a note in `docs/handoffs/`.
