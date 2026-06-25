# InstanceSpec

`InstanceSpec` is the source of truth for a Minecraft instance before launch resolution. Every creation path writes this model.

## Purpose

`InstanceSpec` captures user intent without encoding the resolved launch command. It answers what the user wants to run, where it should live, and which runtime preferences should apply.

Suggested fields:

- Instance id.
- Display name.
- Game version.
- Loader type: Vanilla, Fabric, or Forge.
- Loader version when required.
- Java runtime selection.
- Memory profile.
- Game directory.
- Extra JVM arguments.
- Extra game arguments.

## Ownership Boundary

Guided setup and manual setup both produce `InstanceSpec`. They may collect information differently, but they must not produce separate internal models.

The UI may validate form completeness and display friendly errors. Core validation decides whether the spec can be resolved into a launch plan.

## Required Behavior

- Instance ids must be stable and suitable for storage paths.
- Loader selection must distinguish Vanilla from loader-backed launches.
- Java runtime selection must allow explicit user choice and later managed runtimes.
- Extra arguments must remain separate between JVM and game arguments.
- Game directory must be part of the spec so launch planning is reproducible.

## Non-Goals

- `InstanceSpec` does not resolve libraries, assets, native files, or classpaths.
- `InstanceSpec` does not build process commands.
- `InstanceSpec` does not diagnose crashes or infer repairs.

## First Implementation Slice

Create a core crate with typed enums for loader kind, Java selection, and memory profile. Add validation tests for required fields, unsupported loader combinations, and argument separation.

