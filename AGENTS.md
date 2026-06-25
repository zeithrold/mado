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
- `just test` for workspace tests.
- `just coverage` for line coverage.
- `just mutants-gate` for mutation score enforcement after `cargo-mutants` output exists.
- `just check` for the normal full gate.
- `just check-full` when nightly-only checks and fuzz smoke are appropriate.

Docs-only changes do not require Rust tests. Code changes should at least run the smallest relevant test command, and shared/core behavior should run `just test` plus any focused fuzz, coverage, or mutation checks that apply.

## Implementation Style

- Keep APIs deterministic and testable before connecting them to GPUI.
- Model data explicitly instead of letting UI state become a hidden source of truth.
- Use structured parsers and typed models for Minecraft metadata instead of ad hoc string handling.
- Prefer precise errors that explain what failed to resolve, download, validate, or launch.
- Add comments only when they clarify a non-obvious boundary or invariant.

