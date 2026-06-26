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

There are two preparation scopes:

- `VersionPreparer` prepares metadata needed to finish version resolution, such as manifests, version JSON files, inherited parent documents, and loader profile metadata.
- `RuntimePreparer` prepares files needed after `ResolvedVersion` is available, such as the client jar, libraries, assets, native artifacts, and managed Java runtimes.

Version resolution may request missing metadata through a preparation plan, but resolver stages must not perform network or filesystem download side effects directly. A launch attempt should expose at most one version metadata preparation pass to the user; that pass must try to prepare the metadata dependency closure before resolution is retried.

## Download Manager

The download manager is a controlled concurrent job runtime. It owns download job state, accepts typed commands, emits typed events, and schedules workers. It should not be a global event bus, and it should not use process-wide static state for active jobs.

Core pieces:

- `DownloadPlan`: deterministic list of `DownloadJobSpec` records.
- `DownloadManagerConfig`: concurrency, retry, resume, integrity, event, storage, and timeout policy.
- `DownloadCommand`: pause, resume, cancel, and retry requests for jobs or the whole plan.
- `DownloadEvent`: queued, started, progress, paused, resumed, cancelled, retried, completed, failed, and plan terminal events.
- `DownloadJobState`: explicit per-job state machine.
- `DownloadManagerState`: the single owner of the job table and state transitions.
- `DownloadWorker`: the later I/O implementation for one file transfer.

The manager should use an actor-style control plane: workers report progress, completion, failure, or stop reasons back to the manager; the manager updates state and emits events. Workers must not directly mutate a shared job map.

The first implementation should keep the control plane independent from HTTP I/O. This allows the state machine, command handling, retry scheduling, and event emission to be tested without network access. Native Rust downloading should be the default backend; aria2 may be considered later as an optional backend behind Mado's own plan, command, event, and state model.

## Concurrency And Commands

Concurrency must be bounded. `DownloadManagerConfig` should include global and per-host limits plus queue capacity. High throughput comes from controlled parallelism, not unbounded task spawning.

Commands must be accepted while a plan is running:

- Cancelling a pending job moves it directly to `Cancelled`.
- Cancelling a running job requests worker stop, then the worker report moves it to `Cancelled`.
- Pausing a pending job moves it to `Paused`.
- Pausing a running job requests worker stop while preserving partial data according to policy, then the worker report moves it to `Paused`.
- Resuming a paused job moves it back to `Pending`.
- Retrying a failed or cancelled job moves it back to `Pending`.

Progress events should be throttled by time and byte thresholds before they are broadcast to UI listeners.

## Resume And Integrity

Downloads should eventually use partial files plus atomic promotion:

1. Write to a `.part` file.
2. Record resumable metadata beside it when useful.
3. Resume with HTTP `Range` only when server validators such as `ETag` or `Last-Modified` still match, or when checksum policy gives enough confidence.
4. Verify checksum and size when metadata provides them.
5. Atomically rename the partial file to the target path only after verification succeeds.

Checksum mismatch is a hard integrity failure. It may trigger one bounded redownload attempt when configured, but corrupt or partial files must not be promoted as ready.

## Required Behavior

- Planning must be deterministic.
- Execution must treat checksum mismatch as failure.
- Partial or corrupt files must not be promoted as ready.
- Retry policy must be bounded and observable.
- Progress events must be stable enough for the UI to render total work, completed work, active item, and failure state.
- Metadata preparation and runtime preparation must remain distinct so missing version JSON files do not masquerade as missing launch artifacts.
- Active download job state must be owned by the manager, not by worker-local shared maps.
- Download concurrency must be bounded by validated configuration.
- Commands and events must be typed and testable without network access.

## Non-Goals

- The pipeline does not analyze logs.
- The pipeline does not repair arbitrary user files.
- The pipeline does not implement marketplace, modpack, or community distribution features.

## First Implementation Slice

Start with a pure download planner that emits required artifact records from resolved metadata fixtures. Add executor tests around checksum validation using local test files before adding real HTTP behavior.
