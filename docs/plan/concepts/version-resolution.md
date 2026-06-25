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

## Required Behavior

- Metadata inheritance must be deterministic and testable.
- Library inclusion and exclusion rules must account for OS and architecture.
- Native classifiers must resolve to concrete artifacts when available.
- Loader adapters must normalize their metadata into the same resolved model used by Vanilla.
- Unsupported or incomplete metadata should fail with precise errors.

## Non-Goals

- The resolver does not optimize download mirrors.
- The resolver does not detect mod conflicts.
- The resolver does not repair broken instances.
- Loader support is limited to launch profile resolution, not full ecosystem management.

## First Implementation Slice

Implement metadata structs and inheritance merging with fixture-based tests. Add focused tests for OS rule application, macOS architecture distinction, and a minimal Fabric or Forge adapter fixture once the base resolver is stable.

