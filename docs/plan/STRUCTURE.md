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
3. Run `just fmt`, `just clippy`, and `just test`.
4. Run broader gates such as `just coverage`, `just mutants-gate`, or `just check` when touching shared core behavior.
5. Connect the slice to the demo UI only after the core boundary is stable.

This keeps the UI, architecture, and crate tracks synchronized without letting any one track invent a separate product.

