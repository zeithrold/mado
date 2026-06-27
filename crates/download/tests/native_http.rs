use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use mado_download::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadCommand, DownloadConcurrencyConfig,
    DownloadEvent, DownloadEventStream, DownloadJobId, DownloadJobPolicy, DownloadJobSpec,
    DownloadManagerConfig, DownloadPlan, DownloadResumeConfig, DownloadResumeMode,
    DownloadRetryConfig, DownloadServiceHandle, DownloadServiceLoop, DownloadStorageConfig,
    DownloadStoragePaths, DownloadUrl, NativeHttpBackend, NativeHttpBackendConfig,
    PartialDownloadMetadata, PartialRetentionPolicy, ResumeValidator, ResumeValidatorPolicy,
};
use sha1::{Digest, Sha1};
use tokio::runtime::{Builder, Runtime};

#[test]
fn native_http_backend_downloads_and_promotes_partial() -> Result<(), Box<dyn std::error::Error>> {
    let body = b"hello native http";
    let fixture = HttpFixture::serve(vec![
        HttpResponse::ok(body).with_header("ETag", "\"fresh\""),
    ])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("libraries/example.jar");
    let job = download_job("library", fixture.url.clone(), target_path.clone(), body)?;
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        NativeHttpBackendConfig::default(),
    )?;

    service_loop.start()?;
    let events = wait_for_plan_completed(&mut service_loop, &events)?;

    assert_event_order(
        &events,
        &[
            EventKind::JobQueued,
            EventKind::JobStarted,
            EventKind::JobProgress,
            EventKind::JobCompleted,
            EventKind::PlanCompleted,
        ],
    );
    assert_eq!(std::fs::read(&target_path)?, body);
    assert!(
        !DownloadStoragePaths::for_target(&target_path, &DownloadStorageConfig::default())
            .partial_metadata_path
            .exists()
    );
    assert!(
        fixture
            .received_request()?
            .starts_with("GET /artifact HTTP/1.1")
    );
    Ok(())
}

#[test]
fn native_http_backend_completes_existing_ready_target_without_http()
-> Result<(), Box<dyn std::error::Error>> {
    let body = b"already here";
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("client.jar");
    std::fs::write(&target_path, body)?;
    let fixture = HttpFixture::serve_none()?;
    let job = download_job("client", fixture.url.clone(), target_path, body)?;
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        NativeHttpBackendConfig::default(),
    )?;

    service_loop.start()?;
    let events = wait_for_plan_completed(&mut service_loop, &events)?;

    assert!(events.contains(&DownloadEvent::PlanCompleted));
    assert!(fixture.no_request_received(Duration::from_millis(250))?);
    Ok(())
}

#[test]
fn native_http_backend_resumes_matching_partial_download() -> Result<(), Box<dyn std::error::Error>>
{
    let complete = b"resume me please";
    let prefix = b"resume ";
    let suffix = b"me please";
    let fixture = HttpFixture::serve(vec![
        HttpResponse::new("206 Partial Content", suffix)
            .with_header("ETag", "\"resume\"")
            .with_header("Content-Range", "bytes 7-15/16"),
    ])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("assets/object");
    let mut job = download_job("asset", fixture.url.clone(), target_path.clone(), complete)?;
    job.expected_size = Some(complete.len() as u64);
    write_partial(
        &target_path,
        &job,
        prefix,
        Some(ResumeValidator {
            etag: Some("\"resume\"".to_string()),
            last_modified: None,
        }),
    )?;
    let backend_config = resume_enabled_config();
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        backend_config,
    )?;

    service_loop.start()?;
    let _events = wait_for_plan_completed(&mut service_loop, &events)?;
    let request = fixture.received_request()?;

    assert_header_contains(&request, "range: bytes=7-");
    assert_header_contains(&request, "if-range: \"resume\"");
    assert_eq!(std::fs::read(&target_path)?, complete);
    Ok(())
}

#[test]
fn native_http_backend_does_not_send_range_for_empty_resume_candidate()
-> Result<(), Box<dyn std::error::Error>> {
    let complete = b"start from zero";
    let fixture = HttpFixture::serve(vec![HttpResponse::ok(complete)])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("empty-resume.jar");
    let job = download_job(
        "empty-resume",
        fixture.url.clone(),
        target_path.clone(),
        complete,
    )?;
    write_partial(
        &target_path,
        &job,
        b"",
        Some(ResumeValidator {
            etag: Some("\"empty\"".to_string()),
            last_modified: None,
        }),
    )?;
    let backend_config = NativeHttpBackendConfig {
        resume: DownloadResumeConfig {
            mode: DownloadResumeMode::Enabled,
            min_size: 0,
            validator_policy: ResumeValidatorPolicy::RequireMatch,
            ..DownloadResumeConfig::default()
        },
        ..NativeHttpBackendConfig::default()
    };
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        backend_config,
    )?;

    service_loop.start()?;
    let _events = wait_for_plan_completed(&mut service_loop, &events)?;
    let request = fixture.received_request()?;

    assert_header_absent(&request, "range: bytes=0-");
    assert_eq!(std::fs::read(&target_path)?, complete);
    Ok(())
}

#[test]
fn native_http_backend_retries_transient_http_failure() -> Result<(), Box<dyn std::error::Error>> {
    let body = b"retry eventually succeeds";
    let fixture = HttpFixture::serve(vec![
        HttpResponse::new("503 Service Unavailable", b"busy"),
        HttpResponse::ok(body),
    ])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("retry.jar");
    let job = download_job("retry", fixture.url.clone(), target_path.clone(), body)?;
    let manager_config = DownloadManagerConfig {
        retry: DownloadRetryConfig {
            max_attempts: 2,
            ..DownloadRetryConfig::default()
        },
        ..DownloadManagerConfig::default()
    };
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        manager_config,
        NativeHttpBackendConfig::default(),
    )?;

    service_loop.start()?;
    let events = wait_for_plan_completed(&mut service_loop, &events)?;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobRetryScheduled { attempt: 2, .. }))
    );
    assert_eq!(fixture.received_requests(2)?.len(), 2);
    assert_eq!(std::fs::read(&target_path)?, body);
    Ok(())
}

#[test]
fn native_http_backend_reports_permanent_http_failure() -> Result<(), Box<dyn std::error::Error>> {
    let expected = b"not downloaded";
    let fixture = HttpFixture::serve(vec![HttpResponse::new("404 Not Found", b"missing")])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("missing.jar");
    let job = download_job("missing", fixture.url, target_path.clone(), expected)?;
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        NativeHttpBackendConfig::default(),
    )?;

    service_loop.start()?;
    let events = wait_for_plan_failed(&mut service_loop, &events)?;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobFailed { .. }))
    );
    assert!(!target_path.exists());
    Ok(())
}

#[test]
fn native_http_backend_fails_checksum_mismatch_without_promoting()
-> Result<(), Box<dyn std::error::Error>> {
    let expected = b"expected";
    let wrong = b"wrongone";
    let fixture = HttpFixture::serve(vec![HttpResponse::ok(wrong)])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("checksum.jar");
    let job = download_job("checksum", fixture.url, target_path.clone(), expected)?;
    let paths = DownloadStoragePaths::for_target(&target_path, &DownloadStorageConfig::default());
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        NativeHttpBackendConfig::default(),
    )?;

    service_loop.start()?;
    let events = wait_for_plan_failed(&mut service_loop, &events)?;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobFailed { error, .. } if error.contains("checksum mismatch")))
    );
    assert!(!target_path.exists());
    assert!(paths.partial_path.exists());
    Ok(())
}

#[test]
fn native_http_backend_restarts_when_resume_gets_full_response()
-> Result<(), Box<dyn std::error::Error>> {
    let complete = b"restart from scratch";
    let stale_prefix = b"stale ";
    let fixture = HttpFixture::serve(vec![
        HttpResponse::ok(complete).with_header("ETag", "\"fresh\""),
    ])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("assets/restart");
    let job = download_job(
        "restart",
        fixture.url.clone(),
        target_path.clone(),
        complete,
    )?;
    write_partial(
        &target_path,
        &job,
        stale_prefix,
        Some(ResumeValidator {
            etag: Some("\"stale\"".to_string()),
            last_modified: None,
        }),
    )?;
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        resume_enabled_config(),
    )?;

    service_loop.start()?;
    let _events = wait_for_plan_completed(&mut service_loop, &events)?;
    let request = fixture.received_request()?;

    assert_header_contains(&request, "range: bytes=6-");
    assert_header_contains(&request, "if-range: \"stale\"");
    assert_eq!(std::fs::read(&target_path)?, complete);
    assert!(
        !DownloadStoragePaths::for_target(&target_path, &DownloadStorageConfig::default())
            .partial_path
            .exists()
    );
    Ok(())
}

#[test]
fn native_http_backend_pauses_live_download_and_resumes() -> Result<(), Box<dyn std::error::Error>>
{
    let complete = b"pause then resume";
    let prefix = b"pause ";
    let suffix = b"then resume";
    let gate = BodyGate::new();
    let first_chunk_sent = OneShotSignal::new();
    let fixture = HttpFixture::serve(vec![
        HttpResponse::blocked_after_chunk(
            "200 OK",
            prefix,
            suffix,
            gate.clone(),
            first_chunk_sent.sender(),
        )
        .with_header("ETag", "\"paused\""),
        HttpResponse::new("206 Partial Content", suffix)
            .with_header("ETag", "\"paused\"")
            .with_header("Content-Range", "bytes 6-16/17"),
    ])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("pause.jar");
    let job_id = DownloadJobId::new("pause")?;
    let mut job = download_job_with_id(job_id.clone(), fixture.url, target_path.clone(), complete)?;
    job.expected_size = Some(complete.len() as u64);
    let paths = DownloadStoragePaths::for_target(&target_path, &DownloadStorageConfig::default());
    let (_runtime, mut service_loop, handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        resume_enabled_config(),
    )?;

    service_loop.start()?;
    first_chunk_sent.wait(Duration::from_secs(2))?;
    let _progress_events = wait_for_event(&mut service_loop, &events, event_is_job_progress)?;
    handle.send_command(DownloadCommand::PauseJob(job_id.clone()))?;
    let paused_events = wait_for_event(&mut service_loop, &events, event_is_job_paused)?;

    assert!(
        paused_events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobPauseRequested { .. }))
    );
    assert!(paths.partial_path.exists());
    assert!(paths.partial_metadata_path.exists());
    assert!(!target_path.exists());

    handle.send_command(DownloadCommand::ResumeJob(job_id))?;
    let completed_events = wait_for_plan_completed(&mut service_loop, &events)?;
    gate.release();

    assert!(
        completed_events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobResumed { .. }))
    );
    assert_eq!(std::fs::read(&target_path)?, complete);
    Ok(())
}

#[test]
fn native_http_backend_cancels_live_download_and_deletes_partial_when_configured()
-> Result<(), Box<dyn std::error::Error>> {
    let complete = b"cancel me later";
    let prefix = b"cancel ";
    let suffix = b"me later";
    let gate = BodyGate::new();
    let first_chunk_sent = OneShotSignal::new();
    let fixture = HttpFixture::serve(vec![
        HttpResponse::blocked_after_chunk(
            "200 OK",
            prefix,
            suffix,
            gate.clone(),
            first_chunk_sent.sender(),
        )
        .with_header("ETag", "\"cancel\""),
    ])?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("cancel.jar");
    let job_id = DownloadJobId::new("cancel")?;
    let job = download_job_with_id(job_id.clone(), fixture.url, target_path.clone(), complete)?;
    let paths = DownloadStoragePaths::for_target(&target_path, &DownloadStorageConfig::default());
    let backend_config = NativeHttpBackendConfig {
        resume: DownloadResumeConfig {
            partial_on_failure: PartialRetentionPolicy::Delete,
            ..DownloadResumeConfig::default()
        },
        ..NativeHttpBackendConfig::default()
    };
    let (_runtime, mut service_loop, handle, events) = service_loop_for(
        DownloadPlan::new(vec![job])?,
        DownloadManagerConfig::default(),
        backend_config,
    )?;

    service_loop.start()?;
    first_chunk_sent.wait(Duration::from_secs(2))?;
    let _progress_events = wait_for_event(&mut service_loop, &events, event_is_job_progress)?;
    handle.send_command(DownloadCommand::CancelJob(job_id))?;
    let events = wait_for_plan_failed(&mut service_loop, &events)?;
    gate.release();

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobCancelled { .. }))
    );
    assert!(!target_path.exists());
    assert!(!paths.partial_path.exists());
    assert!(!paths.partial_metadata_path.exists());
    Ok(())
}

#[test]
fn native_http_backend_respects_global_limit_for_multiple_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let body = b"parallel";
    let gate = BodyGate::new();
    let responses = (0..4)
        .map(|_| HttpResponse::blocked_before_body("200 OK", body, gate.clone()))
        .collect();
    let fixture = HttpFixture::serve(responses)?;
    let temp_dir = tempfile::tempdir()?;
    let jobs = (0..4)
        .map(|index| {
            download_job(
                &format!("job-{index}"),
                fixture.url.clone(),
                temp_dir.path().join(format!("job-{index}.jar")),
                body,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let manager_config = DownloadManagerConfig {
        concurrency: DownloadConcurrencyConfig {
            global_limit: 2,
            per_host_limit: 2,
            queue_capacity: 8,
        },
        ..DownloadManagerConfig::default()
    };
    let (_runtime, mut service_loop, _handle, events) = service_loop_for(
        DownloadPlan::new(jobs)?,
        manager_config,
        NativeHttpBackendConfig::default(),
    )?;

    service_loop.start()?;
    fixture.wait_for_requests(2, Duration::from_secs(2))?;
    let first_requests = fixture.received_requests(2)?;
    assert!(fixture.no_request_received(Duration::from_millis(250))?);
    gate.release();
    let events = wait_for_plan_completed(&mut service_loop, &events)?;

    assert_eq!(first_requests.len(), 2);
    assert_eq!(fixture.received_requests(2)?.len(), 2);
    assert_eq!(fixture.max_active_connections(), 2);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, DownloadEvent::JobCompleted { .. }))
            .count(),
        4
    );
    Ok(())
}

type NativeServiceLoop = DownloadServiceLoop<NativeHttpBackend>;

fn tokio_runtime() -> Result<Runtime, Box<dyn std::error::Error>> {
    Ok(Builder::new_multi_thread()
        .enable_all()
        .thread_name("mado-download-http-test")
        .build()?)
}

fn service_loop_for(
    plan: DownloadPlan,
    manager_config: DownloadManagerConfig,
    backend_config: NativeHttpBackendConfig,
) -> Result<
    (
        Runtime,
        NativeServiceLoop,
        DownloadServiceHandle,
        DownloadEventStream,
    ),
    Box<dyn std::error::Error>,
> {
    let runtime = tokio_runtime()?;
    let runtime_handle = runtime.handle().clone();
    let (service_loop, handle, events) =
        DownloadServiceLoop::try_with_backend_factory(plan, manager_config, |handle| {
            Ok(NativeHttpBackend::new(
                runtime_handle,
                handle,
                backend_config,
            )?)
        })?;
    Ok((runtime, service_loop, handle, events))
}

fn wait_for_plan_completed(
    service_loop: &mut NativeServiceLoop,
    events: &DownloadEventStream,
) -> Result<Vec<DownloadEvent>, Box<dyn std::error::Error>> {
    wait_for_event(service_loop, events, |event| {
        matches!(event, DownloadEvent::PlanCompleted)
    })
}

fn wait_for_plan_failed(
    service_loop: &mut NativeServiceLoop,
    events: &DownloadEventStream,
) -> Result<Vec<DownloadEvent>, Box<dyn std::error::Error>> {
    wait_for_event(service_loop, events, |event| {
        matches!(event, DownloadEvent::PlanFailed)
    })
}

fn wait_for_event(
    service_loop: &mut NativeServiceLoop,
    events: &DownloadEventStream,
    predicate: impl Fn(&DownloadEvent) -> bool,
) -> Result<Vec<DownloadEvent>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut collected = Vec::new();
    while Instant::now() < deadline {
        service_loop.run_until_idle()?;
        let new_events = events.drain_available();
        let matched = new_events.iter().any(&predicate);
        collected.extend(new_events);
        if matched {
            return Ok(collected);
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    Err(format!("timed out waiting for event; events: {collected:?}").into())
}

const fn event_is_job_paused(event: &DownloadEvent) -> bool {
    matches!(event, DownloadEvent::JobPaused { .. })
}

const fn event_is_job_progress(event: &DownloadEvent) -> bool {
    matches!(event, DownloadEvent::JobProgress { .. })
}

fn download_job(
    id: &str,
    url: String,
    target_path: PathBuf,
    body: &[u8],
) -> Result<DownloadJobSpec, Box<dyn std::error::Error>> {
    download_job_with_id(DownloadJobId::new(id)?, url, target_path, body)
}

fn download_job_with_id(
    id: DownloadJobId,
    url: String,
    target_path: PathBuf,
    body: &[u8],
) -> Result<DownloadJobSpec, Box<dyn std::error::Error>> {
    Ok(DownloadJobSpec {
        id,
        url: DownloadUrl::new(url)?,
        host: Some("127.0.0.1".to_string()),
        target_path,
        expected_size: Some(body.len() as u64),
        checksum: Some(Checksum {
            algorithm: ChecksumAlgorithm::Sha1,
            value: sha1_hex(body),
        }),
        kind: DownloadArtifactKind::Library,
        policy: DownloadJobPolicy::default(),
    })
}

fn write_partial(
    target_path: impl AsRef<Path>,
    job: &DownloadJobSpec,
    bytes: &[u8],
    validator: Option<ResumeValidator>,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage_config = DownloadStorageConfig::default();
    let paths = DownloadStoragePaths::for_target(target_path, &storage_config);
    paths.write_partial_bytes(bytes, &storage_config)?;
    paths.write_partial_metadata(
        &PartialDownloadMetadata::for_job(job, bytes.len() as u64, validator),
        &storage_config,
    )?;
    Ok(())
}

fn resume_enabled_config() -> NativeHttpBackendConfig {
    NativeHttpBackendConfig {
        resume: DownloadResumeConfig {
            mode: DownloadResumeMode::Enabled,
            min_size: 1,
            validator_policy: ResumeValidatorPolicy::RequireMatch,
            ..DownloadResumeConfig::default()
        },
        ..NativeHttpBackendConfig::default()
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn assert_header_contains(request: &str, expected: &str) {
    assert!(
        request
            .lines()
            .any(|line| line.to_ascii_lowercase() == expected),
        "request did not contain header {expected:?}: {request}"
    );
}

fn assert_header_absent(request: &str, unexpected: &str) {
    assert!(
        request
            .lines()
            .all(|line| line.to_ascii_lowercase() != unexpected),
        "request unexpectedly contained header {unexpected:?}: {request}"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    JobQueued,
    JobStarted,
    JobProgress,
    JobCompleted,
    PlanCompleted,
}

fn assert_event_order(events: &[DownloadEvent], expected_order: &[EventKind]) {
    let mut cursor = 0;
    for event in events {
        let Some(expected) = expected_order.get(cursor) else {
            return;
        };
        if event_kind(event) == Some(*expected) {
            cursor += 1;
        }
    }
    assert_eq!(
        cursor,
        expected_order.len(),
        "events did not contain expected order: {events:?}"
    );
}

const fn event_kind(event: &DownloadEvent) -> Option<EventKind> {
    match event {
        DownloadEvent::JobQueued { .. } => Some(EventKind::JobQueued),
        DownloadEvent::JobStarted { .. } => Some(EventKind::JobStarted),
        DownloadEvent::JobProgress { .. } => Some(EventKind::JobProgress),
        DownloadEvent::JobCompleted { .. } => Some(EventKind::JobCompleted),
        DownloadEvent::PlanCompleted => Some(EventKind::PlanCompleted),
        _ => None,
    }
}

struct HttpResponse {
    status: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: HttpBody,
}

impl HttpResponse {
    fn new(status: &'static str, body: &[u8]) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: HttpBody::Complete(body.to_vec()),
        }
    }

    fn ok(body: &[u8]) -> Self {
        Self::new("200 OK", body)
    }

    fn blocked_before_body(status: &'static str, body: &[u8], gate: BodyGate) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: HttpBody::BlockedBeforeBody {
                body: body.to_vec(),
                gate,
            },
        }
    }

    fn blocked_after_chunk(
        status: &'static str,
        first_chunk: &[u8],
        remaining: &[u8],
        gate: BodyGate,
        first_chunk_sent: mpsc::Sender<()>,
    ) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: HttpBody::BlockedAfterChunk {
                first_chunk: first_chunk.to_vec(),
                remaining: remaining.to_vec(),
                gate,
                first_chunk_sent,
            },
        }
    }

    fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }

    const fn body_len(&self) -> usize {
        self.body.len()
    }
}

enum HttpBody {
    Complete(Vec<u8>),
    BlockedBeforeBody {
        body: Vec<u8>,
        gate: BodyGate,
    },
    BlockedAfterChunk {
        first_chunk: Vec<u8>,
        remaining: Vec<u8>,
        gate: BodyGate,
        first_chunk_sent: mpsc::Sender<()>,
    },
}

impl HttpBody {
    const fn len(&self) -> usize {
        match self {
            Self::Complete(body) | Self::BlockedBeforeBody { body, .. } => body.len(),
            Self::BlockedAfterChunk {
                first_chunk,
                remaining,
                ..
            } => first_chunk.len() + remaining.len(),
        }
    }

    fn write_to(self, stream: &mut impl Write) {
        match self {
            Self::Complete(body) => {
                let _ = stream.write_all(&body);
            }
            Self::BlockedBeforeBody { body, gate } => {
                gate.wait();
                let _ = stream.write_all(&body);
            }
            Self::BlockedAfterChunk {
                first_chunk,
                remaining,
                gate,
                first_chunk_sent,
            } => {
                let _ = stream.write_all(&first_chunk);
                let _ = stream.flush();
                let _ = first_chunk_sent.send(());
                gate.wait();
                let _ = stream.write_all(&remaining);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct BodyGate {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl BodyGate {
    fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn release(&self) {
        let (lock, condvar) = &*self.state;
        if let Ok(mut released) = lock.lock() {
            *released = true;
            condvar.notify_all();
        }
    }

    fn wait(&self) {
        let (lock, condvar) = &*self.state;
        let Ok(mut released) = lock.lock() else {
            return;
        };
        while !*released {
            let Ok(next) = condvar.wait(released) else {
                return;
            };
            released = next;
        }
    }
}

struct OneShotSignal {
    sender: mpsc::Sender<()>,
    receiver: mpsc::Receiver<()>,
}

impl OneShotSignal {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    fn sender(&self) -> mpsc::Sender<()> {
        self.sender.clone()
    }

    fn wait(&self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        Ok(self.receiver.recv_timeout(timeout)?)
    }
}

#[derive(Debug)]
struct HttpFixture {
    url: String,
    request_receiver: mpsc::Receiver<String>,
    metrics: Arc<Mutex<HttpMetrics>>,
}

impl HttpFixture {
    fn serve(responses: Vec<HttpResponse>) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let expected_connections = responses.len();
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let metrics = Arc::new(Mutex::new(HttpMetrics::default()));
        let (request_sender, request_receiver) = mpsc::channel();
        thread::spawn({
            let metrics = Arc::clone(&metrics);
            move || {
                for _ in 0..expected_connections {
                    let Ok((mut stream, _peer)) = listener.accept() else {
                        return;
                    };
                    let responses = Arc::clone(&responses);
                    let request_sender = request_sender.clone();
                    let metrics = Arc::clone(&metrics);
                    thread::spawn(move || {
                        handle_http_connection(&mut stream, &responses, &request_sender, &metrics);
                    });
                }
            }
        });
        Ok(Self {
            url: format!("http://{address}/artifact"),
            request_receiver,
            metrics,
        })
    }

    fn serve_none() -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let metrics = Arc::new(Mutex::new(HttpMetrics::default()));
        let (request_sender, request_receiver) = mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut stream, _peer)) = listener.accept() {
                let request = read_http_request(&mut stream);
                let _ = request_sender.send(request);
            }
        });
        Ok(Self {
            url: format!("http://{address}/artifact"),
            request_receiver,
            metrics,
        })
    }

    fn received_request(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.request_receiver.recv_timeout(Duration::from_secs(2))?)
    }

    fn received_requests(&self, count: usize) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut requests = Vec::with_capacity(count);
        for _ in 0..count {
            requests.push(self.received_request()?);
        }
        Ok(requests)
    }

    fn wait_for_requests(
        &self,
        count: usize,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.request_count() >= count {
                return Ok(());
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        Err(format!(
            "timed out waiting for {count} requests; observed {}",
            self.request_count()
        )
        .into())
    }

    fn no_request_received(&self, timeout: Duration) -> Result<bool, Box<dyn std::error::Error>> {
        match self.request_receiver.recv_timeout(timeout) {
            Ok(_) => Ok(false),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    fn request_count(&self) -> usize {
        self.metrics
            .lock()
            .map_or(0, |metrics| metrics.request_count)
    }

    fn max_active_connections(&self) -> usize {
        self.metrics
            .lock()
            .map_or(0, |metrics| metrics.max_active_connections)
    }
}

#[derive(Debug, Default)]
struct HttpMetrics {
    active_connections: usize,
    max_active_connections: usize,
    request_count: usize,
}

fn handle_http_connection(
    stream: &mut (impl Read + Write),
    responses: &Arc<Mutex<VecDeque<HttpResponse>>>,
    request_sender: &mpsc::Sender<String>,
    metrics: &Arc<Mutex<HttpMetrics>>,
) {
    let request = read_http_request(stream);
    if let Ok(mut metrics) = metrics.lock() {
        metrics.request_count += 1;
        metrics.active_connections += 1;
        metrics.max_active_connections = metrics
            .max_active_connections
            .max(metrics.active_connections);
    }
    let _ = request_sender.send(request);
    let response = responses
        .lock()
        .ok()
        .and_then(|mut responses| responses.pop_front());
    if let Some(response) = response {
        write_http_response(stream, response);
    }
    if let Ok(mut metrics) = metrics.lock() {
        metrics.active_connections = metrics.active_connections.saturating_sub(1);
    }
}

fn write_http_response(stream: &mut impl Write, response: HttpResponse) {
    let mut headers = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body_len()
    );
    for (name, value) in response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    let _ = stream.write_all(headers.as_bytes());
    response.body.write_to(stream);
}

fn read_http_request(stream: &mut (impl Read + ?Sized)) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 512];
    while let Ok(read) = stream.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..read) else {
            break;
        };
        bytes.extend_from_slice(chunk);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
