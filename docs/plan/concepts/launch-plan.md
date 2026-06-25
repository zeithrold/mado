# LaunchPlan

`LaunchPlan` is the deterministic intermediate representation used to start Minecraft.

## Purpose

`LaunchPlan` converts a validated `InstanceSpec` plus resolved version metadata into the exact inputs required by a process launcher.

It should contain:

- Java executable path.
- JVM arguments.
- Game arguments.
- Classpath.
- Main class.
- Native libraries path.
- Working directory.
- Environment variables when needed.

## Ownership Boundary

Only the launch plan builder produces the final command shape. UI code, wizard code, manual setup code, and process execution code must not independently assemble JVM commands.

The process launcher receives `LaunchPlan` and converts it into operating-system process APIs.

## Required Behavior

- The same inputs must produce the same plan.
- Argument order must be stable and covered by tests.
- Classpath construction must be deterministic across platforms.
- Native library paths must be explicit.
- Working directory and game directory behavior must be visible in the plan.

## Non-Goals

- `LaunchPlan` does not download files.
- `LaunchPlan` does not detect Java installations.
- `LaunchPlan` does not interpret process output.
- `LaunchPlan` does not choose UI recommendations.

## First Implementation Slice

Start with a pure builder that accepts already-resolved metadata and returns a typed plan. Add snapshot-like unit tests for stable argument order, classpath ordering, main class selection, and working directory selection.

