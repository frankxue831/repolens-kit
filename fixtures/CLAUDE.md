# fixtures/ guidance

Loaded when working under `fixtures/`.

## Test Fixtures

`fixtures/scenarios/` contains captured Java RepoLens output used for contract parity tests. `fixtures/CONTRACT_SOURCES.md` maps each fixture to its Java source. Scripts in `scripts/` manage fixture capture, refresh, sanitization, and verification.

`fixtures/inputs/` contains small repo fixtures (java-small, rust-small, etc.) used by integration tests for scanning. `fixtures/test-data/` contains standalone data fixtures (e.g., cyclic-graph) consumed directly by unit tests.
