# Download Pipeline

The download pipeline prepares files required by a resolved instance and launch plan.

## Purpose

The pipeline separates planning from execution. Planning calculates what files are required. Execution fetches missing files, verifies them, retries transient failures, and reports progress.

Responsibilities:

- Calculate required client, library, asset, and native artifacts.
- Skip files that already exist and pass validation.
- Verify checksums when metadata provides them.
- Support concurrent downloads.
- Retry failed downloads.
- Report progress to UI without depending on UI code.

## Ownership Boundary

Download planning consumes resolved metadata and launch plan inputs. Download execution handles I/O and network behavior.

The UI observes progress events. It does not choose artifact URLs, decide checksum policy, or manage retry loops.

## Required Behavior

- Planning must be deterministic.
- Execution must treat checksum mismatch as failure.
- Partial or corrupt files must not be promoted as ready.
- Retry policy must be bounded and observable.
- Progress events must be stable enough for the UI to render total work, completed work, active item, and failure state.

## Non-Goals

- The pipeline does not analyze logs.
- The pipeline does not repair arbitrary user files.
- The pipeline does not implement marketplace, modpack, or community distribution features.

## First Implementation Slice

Start with a pure download planner that emits required artifact records from resolved metadata fixtures. Add executor tests around checksum validation using local test files before adding real HTTP behavior.

