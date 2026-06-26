# Planning Structure

Mado work moves on three parallel tracks. They should stay connected, but each track has a different output and feedback loop.

## Demo Application Track

The demo application exists to make launcher flows tangible early. It should show the shape of the product without becoming the source of architecture truth.

Expected output:

- A guided instance creation flow.
- A manual instance creation flow.
- An instance list or equivalent entry point.
- A launch status surface with explicit states.
- UI projections of core models, not alternate UI-only models.

Rules:

- The UI gathers user intent and displays state.
- The UI does not build JVM commands.
- Demo shortcuts are acceptable only when they preserve the same model boundaries as production code.

## Architecture And Concept Track

The architecture track keeps names, boundaries, and invariants stable enough for crate work to proceed without re-litigating every small decision.

Expected output:

- Concept documents for core models and pipelines.
- Compatibility decisions for loaders, Java versions, operating systems, and architectures.
- Clear non-goals that prevent scope drift.
- Follow-up notes when a concept needs to split into a dedicated crate.

The concept documents should be implementation-oriented. Each one should define ownership, required behavior, non-goals, and the first useful tests.

## Crate Implementation Track

Small functional slices should move into crates early and pass the full quality flow before being integrated deeply into the app.

Suggested early crate order:

1. Instance specification types and validation.
2. Launch state model.
3. Version metadata parsing and inheritance resolution.
4. Launch plan construction.
5. Download planning.
6. Java runtime detection and compatibility validation.
7. Process launch execution.

Each crate should include focused unit tests first. Add integration tests when behavior spans crates, and fuzz tests when an invariant is compact enough to test with randomized input.

## Quality Flow

The default path for implementation work is:

1. Add or update the smallest model or behavior slice.
2. Write unit tests that pin down deterministic behavior.
3. Run `just check` locally; it formats, lints, and enforces unit-test line coverage.
4. Let `just check-ci` run in pull request and push CI; it adds integration tests that may use real-world Minecraft metadata and JDK fixtures, plus fuzz smoke.
5. Reserve `just check-full` for the daily scheduled gate; it adds mutation, nightly-only dependency checks, and fuzz.
6. Connect the slice to the demo UI only after the core boundary is stable.

The three tiers intentionally serve different scopes:

- `just check` is the local development loop. It should be fast and deterministic enough for contributors and agents to run before finishing ordinary code changes.
- `just check-ci` is the per-push and pull request loop. It proves the local checks still hold when integration coverage, dependency hygiene, and fuzz harness health checks are added in CI.
- `just check-full` is the scheduled confidence loop. It runs the slowest and most toolchain-sensitive checks, including mutation, nightly-only dependency analysis, and fuzz, so those signals do not slow down everyday work.

For integration tests that touch external network APIs or downloadable fixtures, verify provider URLs and parameters with lightweight `curl` metadata requests before writing the test logic. These tests may be CI-only or locally gated, so early curl checks make provider API mistakes visible without requiring a full local fixture run.

This keeps the UI, architecture, and crate tracks synchronized without letting any one track invent a separate product.
