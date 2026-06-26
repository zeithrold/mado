# Version Resolution

Version resolution turns Minecraft and loader metadata into the resolved inputs required by launch planning and download planning.

## Purpose

The resolver pipeline reads version metadata, follows inheritance, applies platform rules, and produces a complete resolved version model.

Responsibilities:

- Read official version metadata.
- Handle `inheritsFrom`.
- Resolve libraries.
- Resolve asset index references.
- Resolve native libraries.
- Apply operating-system and architecture rules.
- Distinguish macOS ARM and x86_64 behavior.
- Support Vanilla, Fabric, and basic Forge launch profiles.

## Ownership Boundary

Version resolution belongs in core. Fabric and Forge support are first-party adapters for the current launch-focused scope, not external plugins.

The resolver should produce structured metadata for launch planning and download planning. It should not start downloads and should not construct process commands.

`InstanceSpec` is the persisted user intent. It selects the Minecraft version, loader kind and version, game directory, Java policy, memory profile, and user-supplied arguments. It is not the normalized version model.

`ResolvedVersion` is the normalized output of version resolution. It represents the launch semantics derived from vanilla and loader metadata after inheritance, loader overlays, argument normalization, and rule evaluation. It should no longer expose accidental JSON shape to launch planning.

`LaunchPlan` is built after version resolution and file preparation. It is the only layer that turns resolved metadata into executable process inputs.

## Resolver Pipeline

`VersionResolver` should be a single public facade backed by explicit pipeline stages. Callers should not decide which loader resolver to call or how to order inheritance and loader overlays.

Suggested pipeline:

1. Build a `ResolveRequest` from `InstanceSpec` and `ResolveContext`.
2. Collect raw version documents from vanilla metadata and first-party loader adapters.
3. Build the version document graph, including `inheritsFrom` edges.
4. Merge documents in ancestor-first order into a canonical document.
5. Normalize legacy and modern metadata shapes into a typed draft model.
6. Evaluate operating-system, architecture, and feature rules.
7. Validate required launch semantics and produce `ResolvedVersion`.

Each stage should be a concrete domain type with a clear method name, such as `collect`, `build`, `merge`, `normalize`, `evaluate`, or `validate`. Avoid a highly generic pipeline framework whose type signatures obscure the launch domain.

Loader adapters participate by producing standard version document nodes. They should not patch `ResolvedVersion` after normalization. Vanilla, Fabric, and Forge should flow through the same merge, normalize, evaluate, and validate stages once their documents have been collected.

## Missing Metadata And Preparation

Version resolution is allowed to discover that required metadata is missing, but it must not download files itself.

The resolver should return one of:

- `Ready(ResolvedVersion)`.
- `NeedsFiles(VersionPreparationPlan)`.

`VersionPreparationPlan` describes the metadata required to continue resolution, such as the version manifest, a version JSON by id, loader metadata, or a Forge install profile. It is a request for preparation, not an instruction for UI code to choose URLs or retry policy.

The launch coordinator may perform at most one user-visible version metadata preparation pass for a launch attempt:

1. Run version resolution.
2. If it returns `NeedsFiles`, run `VersionPreparer::prepare_closure`.
3. Run version resolution once more.
4. If metadata is still missing, fail with a precise error instead of starting another visible download pass.

`VersionPreparer::prepare_closure` must try to complete the metadata dependency closure, not only the first missing file. It should ensure the version manifest is available, download requested version JSON files, read newly downloaded documents, recursively follow `inheritsFrom`, download parent documents, prepare loader metadata/profile documents, and inspect newly prepared loader documents for additional inheritance.

This rule protects the user experience: a launch may be interrupted once to prepare metadata, but it must not bounce through repeated resolve/download/resolve loops.

## Inheritance

`inheritsFrom` is a graph edge in the version document model, not a launch-plan concern.

When a document inherits from another document, the resolver must load the parent chain and merge in ancestor-first order. The merged canonical document should preserve provenance so errors can point back to the document that introduced a field, library, argument, or download.

Required inheritance behavior:

- Missing local parent metadata should produce `NeedsFiles` when the parent can be looked up or prepared.
- A parent id that cannot be found in the configured metadata provider should fail as a missing inherited version.
- Self-cycles and longer cycles must be detected before merge.
- Cycle errors should include the chain, for example `a -> b -> c -> a`.
- `LaunchPlanBuilder` must not know about `inheritsFrom`; inheritance is resolved before `ResolvedVersion`.

Merge behavior must be semantic rather than a blind JSON deep merge. Fields such as `mainClass`, legacy `minecraftArguments`, modern `arguments`, `libraries`, `downloads`, `assetIndex`, `javaVersion`, and `logging` have distinct inheritance and overlay rules that should be captured in focused tests.

## Required Behavior

- Metadata inheritance must be deterministic and testable.
- Library inclusion and exclusion rules must account for OS and architecture.
- Native classifiers must resolve to concrete artifacts when available.
- Loader adapters must normalize their metadata into the same resolved model used by Vanilla.
- Missing metadata must be reported as preparation needs when it can be prepared, not hidden behind resolver-side I/O.
- A launch attempt must allow at most one user-visible version metadata preparation pass.
- Inheritance cycles must be detected before merge.
- Unsupported or incomplete metadata should fail with precise errors.

## Non-Goals

- The resolver does not optimize download mirrors.
- The resolver does not detect mod conflicts.
- The resolver does not repair broken instances.
- Loader support is limited to launch profile resolution, not full ecosystem management.

## First Implementation Slice

Implement metadata structs and inheritance merging with fixture-based tests. Add focused tests for OS rule application, macOS architecture distinction, and a minimal Fabric or Forge adapter fixture once the base resolver is stable.
