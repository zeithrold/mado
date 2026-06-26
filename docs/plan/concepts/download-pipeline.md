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

## Rust Backend Boundary

The first Rust backend slice should make the service boundary explicit before it performs real HTTP I/O.

`DownloadManagerState` remains a synchronous, deterministic state machine. It owns the job table, validates terminal transitions, schedules ready jobs, and converts commands or worker reports into `DownloadManagerAction` values. It does not spawn tasks, open sockets, read files, or depend on an async runtime.

`DownloadService` is the orchestration layer above the manager. It accepts user commands and worker reports, applies manager actions to a `DownloadBackend`, and immediately asks the manager to schedule any newly-ready work. This layer is where an application or runtime integration can later connect command channels, worker report channels, and typed event delivery.

`DownloadBackend` is an execution boundary, not a source of truth. A backend starts jobs and stops workers in response to manager actions. Workers report back with `WorkerReport`; they do not mutate shared job state, decide plan completion, or publish UI events directly.

The native Rust HTTP backend may use Tokio internally, but Tokio types should not appear in core plan, command, event, state, verifier, storage, or service APIs. Runtime ownership belongs to the application or integration layer, which passes a `tokio::runtime::Handle` into the backend. GPUI owns the UI task model and should observe Mado's typed events rather than drive download workers directly. Tests for the manager, verifier, storage helpers, and service dispatch should stay network-free and runtime-free; backend tests can use a local HTTP fixture once HTTP behavior is introduced.

The first backend-supporting primitives are:

- `DownloadStoragePaths` for deterministic target, partial, and partial metadata paths.
- `PartialDownloadMetadata` and `ResumeValidator` for future `.part.json` resume records.
- `ArtifactVerifier` for size and checksum validation before a file is considered ready.
- `DownloadBackend` for start/stop execution commands.
- `DownloadService` for manager/backend orchestration.

This keeps Mado's downloader model stable if the native backend implementation changes, and it leaves room for a future optional aria2 backend without changing planner, command, event, state, or UI integration contracts.

Before real HTTP I/O is introduced, the Rust boundary includes a runtime-free typed service loop:

- `DownloadServiceInput` carries either a user `DownloadCommand` or a worker `WorkerReport`.
- `DownloadServiceHandle` is the typed sending side used by UI/runtime integration and future workers.
- `DownloadEventStream` is the typed receiving side observed by UI/runtime integration.
- `DownloadServiceLoop` owns the `DownloadService`, consumes service inputs in mailbox order, schedules ready work, dispatches backend actions, and publishes manager events.

This loop deliberately uses standard-library channels in the core crate. The Tokio HTTP backend bridges externally-spawned internal tasks to `DownloadServiceHandle`, but Tokio channel types should remain inside that backend or the application integration layer.

## HTTP Backend Preconditions

The native HTTP backend should be added only after the synchronous primitives below are stable. They are part of Mado's downloader contract rather than incidental HTTP implementation details.

Partial metadata is stored as `.part.json` beside the partial file. The format is versioned with `schema_version`. Version `1` contains the job id, URL, target path, optional expected size, optional checksum, downloaded byte count, and optional resume validators (`ETag` and `Last-Modified`). Unknown schema versions must not be resumed blindly; callers should treat them as incompatible metadata and restart the transfer according to retention policy.

Storage helpers prepare deterministic file paths and perform local filesystem operations:

- create parent directories for target, partial, and partial metadata paths;
- write partial bytes to the configured partial path;
- write and read versioned partial metadata;
- remove stale partial metadata when it has served its purpose;
- promote the partial file to the target path, using atomic rename when configured;
- optionally fsync completed files and their parent directory when configured.

`ArtifactVerifier` classifies an existing target before scheduling HTTP work:

- `Ready` means the target exists and passes size/checksum validation.
- `Missing` means the target does not exist and must be downloaded.
- `NeedsRedownload` means the target exists but is incomplete or corrupt in a way the configured policy permits replacing, such as a size mismatch or a checksum mismatch with one bounded redownload attempt enabled.
- `Failed` means verification failed for a reason the downloader should not silently repair.

Checksum mismatch remains a hard integrity failure. The policy may allow one bounded redownload attempt, but the mismatched file must not be promoted or considered ready.

## Native HTTP Backend

`NativeHttpBackend` is Mado's first-party Rust HTTP execution backend. It implements `DownloadBackend` and uses Tokio plus Reqwest internally, but async task, client, request, and response types remain private to the backend module. It does not create or own a Tokio runtime; callers provide a runtime handle from the application or integration layer.

The backend is wired through `DownloadServiceLoop::try_with_backend_factory` so setup failures, such as HTTP client construction failures, return typed backend errors before any job is scheduled. Once running, the backend responds only to manager actions:

- `StartJob` spawns one cooperative worker task for that job.
- `StopWorker` sends a cooperative stop reason to that worker.

Workers report back through `DownloadServiceHandle` using `WorkerReport`; they do not emit `DownloadEvent` directly and they do not mutate manager state.

The first native backend behavior is intentionally narrow:

- classify an existing target with `ArtifactVerifier` before starting HTTP;
- immediately report completion when the target is already ready;
- write HTTP response bytes to `.part`;
- write versioned `.part.json` metadata during transfer;
- report typed progress;
- support HTTP `Range` resume when compatible partial metadata and validators exist;
- verify the partial file before promotion;
- atomically promote `.part` to the target path when configured;
- verify the promoted target and remove stale partial metadata;
- classify HTTP/server/IO failures as retryable or permanent worker failures.

Stop handling is cooperative. Pause and cancel do not update job state directly inside the backend; the worker reports `Stopped(Paused)` or `Stopped(Cancelled)`, and the manager performs the state transition. Partial retention follows `DownloadResumeConfig`.

Native HTTP tests should prefer local fixtures. The initial integration coverage uses a loopback HTTP fixture for fresh download and promotion, existing-target fast completion, and Range resume. Real Minecraft URLs remain outside the local unit and integration gate.

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

Start with a pure download planner that emits required artifact records from resolved metadata fixtures. Add manager, service-loop, storage, and verifier tests around local files before adding real HTTP behavior. Real HTTP should then enter behind `DownloadBackend`, using local HTTP fixtures first and preserving the existing command, report, event, storage, and verification contracts.
