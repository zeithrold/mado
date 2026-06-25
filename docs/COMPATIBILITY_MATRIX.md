# Compatibility Matrix

This matrix tracks how close each Mado module is to being suitable for a 1.0 release. It is a development status board for feature completeness, interface stability, behavior stability, and test evidence.

It is not a Minecraft version, loader, operating-system, or Java runtime support table.

## Status Legend

| Status | Meaning |
| --- | --- |
| Not Started | No implementation or only notes exist. |
| Draft | Shape is being explored; breaking changes are expected. |
| Implementing | Core behavior exists, but gaps remain. |
| Integrating | Connected across crates or UI, but still needs hardening. |
| Stabilizing | Intended behavior is mostly fixed; tests and edge cases are being expanded. |
| Stable | Interface and behavior are expected to survive into 1.0 without major change. |

## Stability Levels

| Level | Interface Stability | Behavior Stability |
| --- | --- | --- |
| Volatile | Public names, types, and ownership boundaries may change freely. | Semantics are exploratory and may be rewritten. |
| Provisional | Main shape is visible, but callers should expect targeted breaking changes. | Common cases work, but edge cases are still being defined. |
| Candidate | API changes need a specific reason and migration path. | Behavior is covered by focused tests and should be predictable. |
| Stable | API changes require strong justification. | Behavior is documented, tested, and suitable for 1.0. |

## Module Matrix

| Module | Status | Interface Stability | Behavior Stability | Evidence Needed For 1.0 |
| --- | --- | --- | --- | --- |
| Demo UI shell | Draft | Volatile | Volatile | Guided flow, manual flow, instance list, and launch status connected to core models. |
| InstanceSpec | Not Started | Volatile | Volatile | Typed model, validation tests, shared output from guided and manual creation. |
| Launch state | Not Started | Volatile | Volatile | Explicit transition tests for resolving, downloading, preparing, launching, running, exited, and failed states. |
| Version resolution | Not Started | Volatile | Volatile | Fixture tests for metadata parsing, inheritance, libraries, assets, natives, and OS/architecture rules. |
| Loader adapters | Not Started | Volatile | Volatile | Vanilla, Fabric, and basic Forge fixtures normalized into the same resolved model. |
| Java runtime validation | Not Started | Volatile | Volatile | Detection and compatibility tests for user-selected runtimes. |
| LaunchPlan builder | Not Started | Volatile | Volatile | Deterministic tests for JVM args, game args, classpath, main class, natives path, working directory, and environment. |
| Download planner | Not Started | Volatile | Volatile | Deterministic artifact planning tests for client, libraries, assets, and natives. |
| Download executor | Not Started | Volatile | Volatile | Checksum, retry, concurrency, partial-file, and progress-event tests. |
| Process launcher | Not Started | Volatile | Volatile | Process start, stdout/stderr capture, exit code, and failure tests. |
| Raw log capture | Not Started | Volatile | Volatile | Logs captured for viewing/export without diagnostic interpretation. |
| Icon assets | Implementing | Candidate | Candidate | Existing generated asset tests, fuzz target, and mutation gate remain healthy. |

## 1.0 Readiness Rule

A module can be treated as ready for 1.0 only when all of the following are true:

- Status is `Stable`.
- Interface stability is `Stable`.
- Behavior stability is `Stable`.
- Required evidence is covered by tests or documented manual verification.
- Known non-goals are documented so future work does not silently expand the module.

## Update Rule

Update this matrix in the same change that materially changes a module boundary, public type, behavior guarantee, or test evidence. If a module regresses, move its status or stability level backward instead of preserving optimistic labels.

