# Launch State

Launch state is the explicit state machine for preparing and running an instance.

## Purpose

The launcher should expose what it is doing without relying on implicit UI flags. Core launch orchestration owns state transitions, and the UI renders them.

Suggested states:

- `Idle`.
- `ResolvingVersion`.
- `PreparingVersionFiles`.
- `PreparingRuntime`.
- `BuildingLaunchPlan`.
- `Ready`.
- `LaunchingProcess`.
- `Running`.
- `Exited`.
- `Failed`.

## Ownership Boundary

Core orchestration emits state changes. UI code subscribes to state and progress updates.

State should connect instance selection, resolution, downloads, launch planning, process startup, process output capture, exit code capture, and failure reporting.

## Required Behavior

- State transitions must be explicit and testable.
- Failures must preserve enough context to explain which stage failed.
- Version metadata preparation must be visible as a distinct state from runtime artifact preparation.
- A launch attempt must not repeatedly alternate between version resolution and user-visible metadata preparation.
- `Running` must represent an active child process.
- `Exited` must include exit code information when available.
- Captured logs may be attached for viewing or export, but not analyzed.

## Non-Goals

- Launch state does not classify crashes.
- Launch state does not recommend repairs.
- Launch state does not replace structured progress events for downloads.

## First Implementation Slice

Define the state enum and transition helpers in a core crate. Add tests for allowed transitions, failure propagation, process exit handling, and the distinction between download progress and launch lifecycle state.
