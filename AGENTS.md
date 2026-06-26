# Mado Contributor Guide

Mado is a Minecraft launcher. The current scope is narrow: create, prepare, and launch a Minecraft instance reliably.

Keep every change pointed at that goal unless the task explicitly says otherwise.

## Product Boundary

The core responsibility is to resolve an instance into a deterministic launch plan, prepare every required file, then start the game process.

In scope:

- Instance creation through both guided and manual UI flows.
- Deterministic launch plan construction from one shared instance model.
- Vanilla, Fabric, and basic Forge launch profile support as first-party core adapters.
- Version metadata, library, asset, native, Java runtime, download, and process-launch plumbing.
- Basic launch state reporting and raw stdout/stderr capture.

Out of scope:

- Crash diagnosis, log analysis, mod conflict prediction, repair suggestions, and one-click fixes.
- Runtime Java agents, expert toolboxes, plugin systems, WASM extensions, marketplaces, server lists, and community features.
- Modpack distribution, paid tunneling, and third-party authentication systems by default.

## Architecture Rules

- UI code must not construct JVM commands.
- Guided and manual creation flows must write the same `InstanceSpec` shape.
- `LaunchPlan` is the only layer that turns resolved instance data into Java executable, JVM arguments, game arguments, classpath, main class, native path, working directory, and environment.
- Core adapters for Vanilla, Fabric, and Forge belong in launcher core for the current scope.
- State must be explicit. Do not hide resolving, downloading, preparing, launching, running, exit, or failure behind UI-local flags.
- Logs may be captured for viewing or export, but not interpreted as diagnostics in the current scope.

## Repository Shape

Use the existing Rust workspace conventions:

- `apps/` contains runnable applications.
- `crates/` contains reusable implementation crates.
- `fuzz/` contains fuzz targets for invariants that deserve randomized coverage.
- `scripts/` contains quality-gate helpers.
- `docs/plan/` contains architecture planning documents.

Prefer small crates with explicit boundaries over broad application code. When adding a crate, wire it through the workspace manifest and give it focused unit tests before integrating it into the app.

## Quality Gates

Use the existing `justfile` recipes:

- `just fmt` for formatting.
- `just clippy` for strict linting.
- `just test-unit` for local unit tests.
- `just test-integration` for CI-only integration tests that may use real-world Minecraft metadata or JDK fixtures.
- `just coverage` for local unit-test line coverage.
- `just mutants-gate` for mutation score enforcement after `cargo-mutants` output exists.
- `just check` for the local development gate, including unit-test coverage.
- `just check-ci` for the per-push and pull request CI gate, including integration tests.
- `just check-full` for the daily full gate, including mutation, nightly-only checks, and fuzz smoke.

Docs-only changes do not require Rust tests. Code changes should at least run the smallest relevant local test command. Shared/core behavior should run `just check` locally, rely on `just check-ci` for integration coverage in CI, and reserve `just check-full` for the scheduled daily gate.

When adding integration tests that depend on network APIs or downloadable fixtures, first validate the provider URLs and parameters with lightweight `curl` metadata requests. Do this before encoding the test fixture logic, because CI-only or gated network tests are often hard to run locally and otherwise make it unclear whether failures come from the code under test or from incorrect provider API assumptions.

### UI Coverage

Demo applications are allowed to be outside the coverage gate. Do not spend coverage budget forcing tests onto GPUI demo shells whose purpose is visual exploration.

Formal UI applications must use the GPUI test framework for UI behavior. When writing, debugging, or reproducing those tests, use the repository `gpui-test` skill and prefer `#[gpui::test]` with `TestAppContext` or the deterministic GPUI scheduler instead of ad hoc UI test harnesses.

## Implementation Style

- Keep APIs deterministic and testable before connecting them to GPUI.
- Model data explicitly instead of letting UI state become a hidden source of truth.
- Use structured parsers and typed models for Minecraft metadata instead of ad hoc string handling.
- Prefer precise errors that explain what failed to resolve, download, validate, or launch.
- Add comments only when they clarify a non-obvious boundary or invariant.
