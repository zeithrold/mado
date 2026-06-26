# Testing Strategy

Mado tests should protect the launcher core goal: turn an instance into a deterministic launch plan, prepare required files, and start the game process.

Use the smallest test style that can prove the behavior clearly. Unit tests define expected semantics. Integration tests prove crate and provider boundaries. Fuzz tests search broad input spaces for invariant violations after the expected semantics are already pinned down.

## Test Layers

### Unit Tests

Use unit tests for deterministic behavior inside a crate.

Unit tests should cover:

- Instance validation rules.
- Metadata parsing and typed model conversion.
- Version inheritance and merge behavior.
- Library, asset, native, Java runtime, and download planning decisions.
- Launch plan argument order, classpath order, main class selection, working directory, native path, and environment.
- Precise error cases for resolve, download, validate, prepare, and launch boundaries.

Unit tests must not depend on network access, real Java installations, temporary executable scripts, wall-clock timing, or host-specific process behavior. Use fakes for those boundaries.

### Integration Tests

Use integration tests when behavior spans crates, adapters, or real-world provider fixtures.

Integration tests are appropriate for:

- Vanilla, Fabric, and Forge profile compatibility checks.
- Real-world Minecraft version metadata fixtures.
- JDK fixture compatibility behavior.
- Download pipeline behavior that crosses resolver, planner, and fetcher boundaries.
- End-to-end launch preparation that does not require starting a real game process.

When adding integration tests that depend on network APIs or downloadable fixtures, first validate provider URLs and parameters with lightweight `curl` metadata requests. Keep those tests CI-only or explicitly gated unless they are stable and deterministic enough for local development.

### Fuzz Tests

Use fuzz tests when an invariant is compact enough to check with randomized input and the input space is too broad for handwritten examples.

Fuzz tests should not define the desired behavior by themselves. First add focused unit tests for representative valid, invalid, and edge cases. Then add a fuzz target to search for panics, invalid states, traversal bugs, non-determinism, or invariant violations around that already-defined behavior.

Add or extend a fuzz target when code handles:

- External structured input, including Minecraft version manifests, version metadata, library rules, asset indexes, Fabric profiles, and Forge profiles.
- Path, file name, URL, or archive-entry normalization, especially traversal prevention, platform separators, empty strings, reserved names, unusual bytes, and Unicode edge cases.
- `LaunchPlan` invariants, including argument placeholder substitution, classpath ordering, main class selection, native path construction, working directory selection, and environment construction.
- Rule composition, including OS, architecture, feature, allow, and disallow precedence.
- Loader or profile merging, including inheritance, override, and adapter-specific profile composition.
- Deduplication, sorting, or merging of libraries, artifacts, assets, downloads, Java runtime components, or classpath entries.
- Pure or mostly pure functions where small inputs can create many meaningful states.
- Any area that has already produced a boundary bug. Keep the exact regression as a unit test, then use fuzzing to search for related cases.

Do not fuzz UI behavior, real network downloads, real process launching, local Java discovery, wall-clock timing, or filesystem race behavior directly. Extract deterministic core logic behind those boundaries and fuzz that smaller layer.

## Fuzz Target Shape

A useful fuzz target should:

- Exercise one clear boundary or invariant.
- Convert arbitrary bytes into typed input through the same parser or constructor used by production code when possible.
- Assert deterministic invariants instead of only checking for panics.
- Keep host-dependent behavior behind fakes or small in-memory adapters.
- Minimize fixture size so failures shrink to readable cases.
- Avoid sleeps, network calls, process execution, and dependence on the contributor's machine.

Good fuzz invariants include:

- Parsing never panics and invalid input returns a structured error.
- Untrusted metadata values enforce size and character-safety limits before they can reach UI labels, logs, errors, command construction, or persisted state.
- Error variants for hostile or malformed input avoid echoing unbounded raw input.
- Normalized paths never escape the intended root.
- Resolution returns stable output for identical input.
- Sorting and deduplication produce deterministic order.
- A constructed `LaunchPlan` never contains unresolved placeholders where production launch execution requires concrete values.
- Merge operations preserve required fields or return a precise error.

## Fuzz Corpus Seeds

Fuzz targets that parse external or locally-discovered metadata should include a hand-written seed corpus in addition to any generated corpus produced by `cargo fuzz`. Generated corpus files are useful for continued local exploration, but hand seeds document the threat model and keep new environments from starting cold.

Seed corpora should cover:

- Representative valid provider data, including known Java vendors, loader profiles, Minecraft metadata shapes, and supported architectures.
- Strange but plausible user inputs, including empty values, duplicate keys, unknown vendors, unknown architectures, older formats, and partial files.
- Damaged local installations, including missing required fields, malformed quotes, truncated files, noisy probe output, and mismatched field names.
- Hostile strings, including shell-looking text, path-looking text, very long values, control characters, escape sequences, Unicode edge cases, and repeated lines.
- Boundary sizes that exercise documented limits, such as maximum metadata field length or maximum metadata file size.

Keep hand seeds small and readable. If a crash or invariant failure is found, keep the minimized generated input as a regression seed only when it communicates a distinct boundary that is not already represented by a clearer hand-written seed.

## Defensive Fuzzing

Fuzzing is part of the defensive boundary for untrusted metadata, not just a panic finder. Treat local files from unknown Java runtimes, downloaded provider metadata, loader profiles, archives, and user-selected paths as untrusted inputs even when they come from the user's machine.

Defensive fuzz targets should prefer invariants that model product safety:

- Parsed metadata is deterministic for identical input.
- Accepted strings are bounded before display, logging, persistence, or error formatting.
- Accepted strings exclude control characters unless the target explicitly validates an escaping strategy.
- Malformed input returns typed errors or falls back through an intentional adapter boundary.
- Parser behavior for duplicate keys, missing fields, defaulted optional fields, and unknown classifications is explicit and covered by unit tests.
- Fuzz targets do not directly execute discovered commands, invoke Java, read arbitrary host paths, or perform network access.

Short fuzz runs such as `just fuzz-smoke` prove that targets still build, start, and survive their seed corpus. They are not evidence of broad input-space coverage. Use `cargo run -p xtask -- fuzz nightly` in scheduled jobs to run all registered fuzz targets with bounded longer runs. Use target-specific longer `xtask` runs, such as `cargo run -p xtask -- fuzz run java-runtime-metadata --seconds 300`, when validating one defensive boundary locally.

## Quality Gates

Docs-only changes do not require Rust tests.

For code changes:

- Run the smallest relevant local unit test while developing.
- Run `just check` before handing off shared or core behavior.
- Rely on `just check-ci` for CI-only integration tests, dependency hygiene, and fuzz smoke.
- Reserve `just check-full` for scheduled deep validation, including mutation and longer fuzz runs.

Fuzz coverage should grow around core deterministic boundaries first: version metadata, launch plan construction, download planning, path normalization, and loader profile composition.
