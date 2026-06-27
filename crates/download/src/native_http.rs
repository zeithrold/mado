use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::{
    ArtifactVerifier, DownloadBackend, DownloadBackendError, DownloadIntegrityConfig,
    DownloadJobId, DownloadJobSpec, DownloadResumeConfig, DownloadResumeMode,
    DownloadServiceHandle, DownloadStorageConfig, DownloadStoragePaths, DownloadTimeoutConfig,
    ExistingArtifactDecision, PartialDownloadMetadata, PartialRetentionPolicy, ResumeValidator,
    ResumeValidatorPolicy, WorkerReport, WorkerStopReason,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeHttpBackendConfig {
    pub storage: DownloadStorageConfig,
    pub integrity: DownloadIntegrityConfig,
    pub resume: DownloadResumeConfig,
    pub timeouts: DownloadTimeoutConfig,
}

#[derive(Debug)]
pub struct NativeHttpBackend {
    runtime_handle: Handle,
    client: reqwest::Client,
    report_handle: DownloadServiceHandle,
    config: NativeHttpBackendConfig,
    active_workers: BTreeMap<DownloadJobId, NativeHttpWorkerControl>,
}

impl NativeHttpBackend {
    pub fn new(
        runtime_handle: Handle,
        report_handle: DownloadServiceHandle,
        config: NativeHttpBackendConfig,
    ) -> Result<Self, DownloadBackendError> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.timeouts.connect_timeout)
            .timeout(config.timeouts.request_timeout)
            .build()
            .map_err(|source| DownloadBackendError::Setup {
                message: format!("failed to build reqwest client: {source}"),
            })?;
        Ok(Self {
            runtime_handle,
            client,
            report_handle,
            config,
            active_workers: BTreeMap::new(),
        })
    }

    pub fn active_worker_count(&self) -> usize {
        self.active_workers.len()
    }
}

impl DownloadBackend for NativeHttpBackend {
    fn start_job(&mut self, job: DownloadJobSpec) -> Result<(), DownloadBackendError> {
        self.reap_finished_workers();
        if self.active_workers.contains_key(&job.id) {
            return Err(DownloadBackendError::StartJob {
                id: job.id,
                message: "worker is already active".to_string(),
            });
        }

        let (stop_sender, stop_receiver) = oneshot::channel();
        let worker = NativeHttpWorker {
            job: job.clone(),
            client: self.client.clone(),
            report_handle: self.report_handle.clone(),
            config: self.config.clone(),
            stop_receiver,
        };
        let join_handle = self.runtime_handle.spawn(worker.run());
        self.active_workers.insert(
            job.id,
            NativeHttpWorkerControl {
                stop_sender: Some(stop_sender),
                join_handle,
            },
        );
        Ok(())
    }

    fn stop_worker(
        &mut self,
        id: &DownloadJobId,
        reason: WorkerStopReason,
    ) -> Result<(), DownloadBackendError> {
        self.reap_finished_workers();
        let Some(control) = self.active_workers.get_mut(id) else {
            return Ok(());
        };
        let Some(stop_sender) = control.stop_sender.take() else {
            return Ok(());
        };
        stop_sender
            .send(reason)
            .map_err(|_| DownloadBackendError::StopWorker {
                id: id.clone(),
                message: "worker already stopped".to_string(),
            })
    }
}

impl NativeHttpBackend {
    fn reap_finished_workers(&mut self) {
        self.active_workers
            .retain(|_, control| !control.join_handle.is_finished());
    }
}

#[derive(Debug)]
struct NativeHttpWorkerControl {
    stop_sender: Option<oneshot::Sender<WorkerStopReason>>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl Drop for NativeHttpWorkerControl {
    fn drop(&mut self) {
        self.join_handle.abort();
    }
}

#[derive(Debug)]
struct NativeHttpWorker {
    job: DownloadJobSpec,
    client: reqwest::Client,
    report_handle: DownloadServiceHandle,
    config: NativeHttpBackendConfig,
    stop_receiver: oneshot::Receiver<WorkerStopReason>,
}

impl NativeHttpWorker {
    async fn run(mut self) {
        let result = self.download().await;
        match result {
            Ok(WorkerOutcome::Completed) => {
                self.report(WorkerReport::Completed {
                    id: self.job.id.clone(),
                });
            }
            Ok(WorkerOutcome::Stopped(reason)) => {
                self.apply_partial_retention(reason);
                self.report(WorkerReport::Stopped {
                    id: self.job.id.clone(),
                    reason,
                });
            }
            Err(error) => {
                self.apply_failure_retention();
                self.report(WorkerReport::Failed {
                    id: self.job.id.clone(),
                    error: error.message,
                    retryable: error.retryable,
                });
            }
        }
    }

    async fn download(&mut self) -> Result<WorkerOutcome, NativeHttpWorkerError> {
        let Some((paths, verifier, resume)) = self.prepare_download()? else {
            return Ok(WorkerOutcome::Completed);
        };
        let response = self.send_request(&paths, &resume).await?;
        if let Some(reason) = self.stream_response(&paths, response, &resume).await? {
            return Ok(WorkerOutcome::Stopped(reason));
        }
        self.finalize_download(&paths, &verifier)?;
        Ok(WorkerOutcome::Completed)
    }

    fn prepare_download(
        &self,
    ) -> Result<
        Option<(DownloadStoragePaths, ArtifactVerifier, ResumeDecision)>,
        NativeHttpWorkerError,
    > {
        let paths = DownloadStoragePaths::for_job(&self.job, &self.config.storage);
        let verifier = ArtifactVerifier::new(self.config.integrity.clone());
        match verifier.classify_existing_job_target(&self.job) {
            ExistingArtifactDecision::Ready(_) => return Ok(None),
            ExistingArtifactDecision::Missing { .. }
            | ExistingArtifactDecision::NeedsRedownload { .. } => {}
            ExistingArtifactDecision::Failed { error } => {
                return Err(NativeHttpWorkerError::permanent(format!(
                    "existing target verification failed: {error}"
                )));
            }
        }
        paths
            .ensure_parent_dirs()
            .map_err(|source| NativeHttpWorkerError::permanent(source.to_string()))?;
        let resume = self.resume_decision(&paths);
        if !resume.should_resume {
            remove_file_if_exists(&paths.partial_path)
                .map_err(|source| NativeHttpWorkerError::permanent(source.to_string()))?;
            paths
                .remove_partial_metadata_if_exists()
                .map_err(|source| NativeHttpWorkerError::permanent(source.to_string()))?;
        }
        Ok(Some((paths, verifier, resume)))
    }

    async fn send_request(
        &self,
        paths: &DownloadStoragePaths,
        resume: &ResumeDecision,
    ) -> Result<reqwest::Response, NativeHttpWorkerError> {
        let mut request = self.client.get(self.job.url.as_str());
        if resume.should_resume && resume.downloaded > 0 {
            request = request.header(
                reqwest::header::RANGE,
                format!("bytes={}-", resume.downloaded),
            );
            if let Some(if_range) = &resume.if_range {
                request = request.header(reqwest::header::IF_RANGE, if_range);
            }
        }
        let response = request.send().await.map_err(|source| {
            NativeHttpWorkerError::retryable(format!("HTTP request failed: {source}"))
        })?;
        if !response.status().is_success() {
            return Err(NativeHttpWorkerError {
                message: format!("HTTP request returned {}", response.status()),
                retryable: response.status().is_server_error(),
            });
        }

        let resumed = resume.should_resume
            && resume.downloaded > 0
            && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if resume.should_resume && resume.downloaded > 0 && !resumed {
            remove_file_if_exists(&paths.partial_path)
                .map_err(|source| NativeHttpWorkerError::permanent(source.to_string()))?;
        }
        Ok(response)
    }

    async fn stream_response(
        &mut self,
        paths: &DownloadStoragePaths,
        response: reqwest::Response,
        resume: &ResumeDecision,
    ) -> Result<Option<WorkerStopReason>, NativeHttpWorkerError> {
        let resumed = resume.should_resume
            && resume.downloaded > 0
            && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let total = response.content_length().map(|length| {
            if resumed {
                length.saturating_add(resume.downloaded)
            } else {
                length
            }
        });
        let validator = response_validator(&response);
        let mut downloaded = if resumed { resume.downloaded } else { 0 };
        let mut file = open_partial_file(&paths.partial_path, resumed)
            .await
            .map_err(|source| {
                NativeHttpWorkerError::retryable(format!(
                    "failed to open partial artifact: {source}"
                ))
            })?;
        let mut stream = response.bytes_stream();

        loop {
            let chunk = tokio::select! {
                stop = &mut self.stop_receiver => {
                    return Ok(Some(stop.unwrap_or(WorkerStopReason::Cancelled)));
                }
                next = stream.next() => {
                    let Some(chunk) = next else {
                        break;
                    };
                    chunk.map_err(|source| {
                        NativeHttpWorkerError::retryable(format!("failed to read HTTP response: {source}"))
                    })?
                }
            };
            tokio::select! {
                stop = &mut self.stop_receiver => {
                    return Ok(Some(stop.unwrap_or(WorkerStopReason::Cancelled)));
                }
                write_result = write_chunk(&mut file, &chunk) => {
                    write_result.map_err(|source| {
                        NativeHttpWorkerError::retryable(format!("failed to write partial artifact: {source}"))
                    })?;
                }
            }
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            let partial_metadata =
                PartialDownloadMetadata::for_job(&self.job, downloaded, validator.clone());
            paths
                .write_partial_metadata(&partial_metadata, &self.config.storage)
                .map_err(|source| NativeHttpWorkerError::retryable(source.to_string()))?;
            self.report(WorkerReport::Progress {
                id: self.job.id.clone(),
                downloaded,
                total,
            });
        }
        file.sync_all().await.map_err(|source| {
            NativeHttpWorkerError::retryable(format!("failed to fsync partial artifact: {source}"))
        })?;
        drop(file);
        Ok(None)
    }

    fn finalize_download(
        &self,
        paths: &DownloadStoragePaths,
        verifier: &ArtifactVerifier,
    ) -> Result<(), NativeHttpWorkerError> {
        verifier
            .verify_path(&self.job, &paths.partial_path)
            .map_err(|source| {
                NativeHttpWorkerError::permanent(format!("partial verification failed: {source}"))
            })?;
        paths
            .promote_partial_to_target(&self.config.storage)
            .map_err(|source| NativeHttpWorkerError::permanent(source.to_string()))?;
        verifier.verify_job_target(&self.job).map_err(|source| {
            NativeHttpWorkerError::permanent(format!("target verification failed: {source}"))
        })?;
        paths
            .remove_partial_metadata_if_exists()
            .map_err(|source| NativeHttpWorkerError::permanent(source.to_string()))?;
        Ok(())
    }

    fn resume_decision(&self, paths: &DownloadStoragePaths) -> ResumeDecision {
        resume_decision_for_job(&self.job, &self.config.resume, paths)
    }

    fn apply_partial_retention(&self, reason: WorkerStopReason) {
        let paths = DownloadStoragePaths::for_job(&self.job, &self.config.storage);
        let policy = match reason {
            WorkerStopReason::Paused => self.config.resume.partial_on_pause,
            WorkerStopReason::Cancelled => self.config.resume.partial_on_failure,
        };
        if policy == PartialRetentionPolicy::Delete {
            let _ = remove_file_if_exists(&paths.partial_path);
            let _ = paths.remove_partial_metadata_if_exists();
        }
    }

    fn apply_failure_retention(&self) {
        if self.config.resume.partial_on_failure == PartialRetentionPolicy::Delete {
            let paths = DownloadStoragePaths::for_job(&self.job, &self.config.storage);
            let _ = remove_file_if_exists(&paths.partial_path);
            let _ = paths.remove_partial_metadata_if_exists();
        }
    }

    fn report(&self, report: WorkerReport) {
        let _ = self.report_handle.send_worker_report(report);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResumeDecision {
    should_resume: bool,
    downloaded: u64,
    if_range: Option<String>,
}

impl ResumeDecision {
    const fn restart() -> Self {
        Self {
            should_resume: false,
            downloaded: 0,
            if_range: None,
        }
    }
}

fn resume_decision_for_job(
    job: &DownloadJobSpec,
    resume_config: &DownloadResumeConfig,
    paths: &DownloadStoragePaths,
) -> ResumeDecision {
    if resume_config.mode == DownloadResumeMode::Disabled {
        return ResumeDecision::restart();
    }
    let Ok(metadata) = paths.read_partial_metadata() else {
        return ResumeDecision::restart();
    };
    if !metadata_matches_job(&metadata, job) {
        return ResumeDecision::restart();
    }
    let Ok(partial_metadata) = fs::metadata(&paths.partial_path) else {
        return ResumeDecision::restart();
    };
    if partial_metadata.len() != metadata.downloaded {
        return ResumeDecision::restart();
    }
    if metadata.downloaded < resume_config.min_size {
        return ResumeDecision::restart();
    }
    let if_range = metadata.validator.as_ref().and_then(|validator| {
        validator
            .etag
            .clone()
            .or_else(|| validator.last_modified.clone())
    });
    if if_range.is_none() && resume_config.validator_policy == ResumeValidatorPolicy::RequireMatch {
        return ResumeDecision::restart();
    }
    ResumeDecision {
        should_resume: true,
        downloaded: metadata.downloaded,
        if_range,
    }
}

#[cfg(fuzzing)]
pub mod fuzzing {
    use std::path::Path;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FuzzResumeDecision {
        pub should_resume: bool,
        pub downloaded: u64,
        pub if_range: Option<String>,
    }

    pub fn resume_decision_for_inputs(
        job: &DownloadJobSpec,
        config: &NativeHttpBackendConfig,
        root: &Path,
        partial_bytes: &[u8],
        metadata_bytes: &[u8],
    ) -> FuzzResumeDecision {
        let paths = DownloadStoragePaths::for_job(job, &config.storage);
        let paths = DownloadStoragePaths {
            target_path: root.join(&paths.target_path),
            partial_path: root.join(&paths.partial_path),
            partial_metadata_path: root.join(&paths.partial_metadata_path),
        };
        paths
            .ensure_parent_dirs()
            .expect("fuzz resume directories should be creatable");
        fs::write(&paths.partial_path, partial_bytes)
            .expect("fuzz partial artifact should be writable");
        fs::write(&paths.partial_metadata_path, metadata_bytes)
            .expect("fuzz partial metadata should be writable");

        let decision = super::resume_decision_for_job(job, &config.resume, &paths);
        FuzzResumeDecision {
            should_resume: decision.should_resume,
            downloaded: decision.downloaded,
            if_range: decision.if_range,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerOutcome {
    Completed,
    Stopped(WorkerStopReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeHttpWorkerError {
    message: String,
    retryable: bool,
}

impl NativeHttpWorkerError {
    const fn retryable(message: String) -> Self {
        Self {
            message,
            retryable: true,
        }
    }

    const fn permanent(message: String) -> Self {
        Self {
            message,
            retryable: false,
        }
    }
}

fn metadata_matches_job(metadata: &PartialDownloadMetadata, job: &DownloadJobSpec) -> bool {
    let same_id = metadata.job_id == job.id;
    let same_url = metadata.url == job.url;
    let same_target = metadata.target_path == job.target_path;
    let same_size = metadata.expected_size == job.expected_size;
    let same_checksum = metadata.checksum == job.checksum;
    same_id && same_url && same_target && same_size && same_checksum
}

fn response_validator(response: &reqwest::Response) -> Option<ResumeValidator> {
    let headers = response.headers();
    let etag = headers
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let last_modified = headers
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    if etag.is_none() && last_modified.is_none() {
        return None;
    }
    Some(ResumeValidator {
        etag,
        last_modified,
    })
}

async fn open_partial_file(path: &Path, append: bool) -> io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(path).await
}

async fn write_chunk(file: &mut tokio::fs::File, chunk: &[u8]) -> io::Result<()> {
    use tokio::io::AsyncWriteExt as _;

    file.write_all(chunk).await
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(source),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;

    use sha1::{Digest, Sha1};
    use tokio::sync::oneshot;

    use super::*;
    use crate::{
        Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadJobPolicy,
        DownloadManagerConfig, DownloadPlan, DownloadServiceLoop, DownloadUrl,
    };

    #[test]
    fn resume_decision_restarts_when_resume_is_disabled() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(
            job,
            NativeHttpBackendConfig {
                resume: DownloadResumeConfig {
                    mode: DownloadResumeMode::Disabled,
                    min_size: 1,
                    ..DownloadResumeConfig::default()
                },
                ..NativeHttpBackendConfig::default()
            },
        )?;

        let decision = worker.resume_decision(&fixture.paths);

        assert_eq!(decision, ResumeDecision::restart());
        Ok(())
    }

    #[test]
    fn resume_decision_restarts_when_metadata_does_not_match_job()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        let mut mismatched_job = job.clone();
        mismatched_job.url = DownloadUrl::new("http://127.0.0.1/other")?;
        fixture.write_partial(&mismatched_job, b"part", Some(etag_validator()))?;
        let worker = test_worker(job, resume_enabled_config())?;

        let decision = worker.resume_decision(&fixture.paths);

        assert_eq!(decision, ResumeDecision::restart());
        Ok(())
    }

    #[test]
    fn resume_decision_restarts_when_partial_length_does_not_match_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture
            .paths
            .write_partial_bytes(b"part", &fixture.storage)?;
        fixture.paths.write_partial_metadata(
            &PartialDownloadMetadata::for_job(&job, 9, Some(etag_validator())),
            &fixture.storage,
        )?;
        let worker = test_worker(job, resume_enabled_config())?;

        let decision = worker.resume_decision(&fixture.paths);

        assert_eq!(decision, ResumeDecision::restart());
        Ok(())
    }

    #[test]
    fn resume_decision_restarts_when_partial_is_below_min_size()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(
            job,
            NativeHttpBackendConfig {
                resume: DownloadResumeConfig {
                    min_size: 5,
                    ..DownloadResumeConfig::default()
                },
                ..NativeHttpBackendConfig::default()
            },
        )?;

        let decision = worker.resume_decision(&fixture.paths);

        assert_eq!(decision, ResumeDecision::restart());
        Ok(())
    }

    #[test]
    fn resume_decision_restarts_when_validator_is_required_but_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", None)?;
        let worker = test_worker(job, resume_enabled_config())?;

        let decision = worker.resume_decision(&fixture.paths);

        assert_eq!(decision, ResumeDecision::restart());
        Ok(())
    }

    #[test]
    fn resume_decision_uses_etag_for_if_range_when_partial_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(job, resume_enabled_config())?;

        let decision = worker.resume_decision(&fixture.paths);

        assert_eq!(
            decision,
            ResumeDecision {
                should_resume: true,
                downloaded: 4,
                if_range: Some("\"resume\"".to_string()),
            }
        );
        Ok(())
    }

    #[test]
    fn metadata_matches_job_rejects_changed_checksum() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let mut job = test_job(temp_dir.path().join("artifact.jar"), b"complete")?;
        let metadata = PartialDownloadMetadata::for_job(&job, 4, Some(etag_validator()));
        job.checksum = Some(Checksum {
            algorithm: ChecksumAlgorithm::Sha1,
            value: "different".to_string(),
        });

        assert!(!metadata_matches_job(&metadata, &job));
        Ok(())
    }

    #[test]
    fn failure_retention_delete_removes_partial_and_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(
            job,
            NativeHttpBackendConfig {
                resume: DownloadResumeConfig {
                    partial_on_failure: PartialRetentionPolicy::Delete,
                    ..DownloadResumeConfig::default()
                },
                ..NativeHttpBackendConfig::default()
            },
        )?;

        worker.apply_failure_retention();

        assert!(!fixture.paths.partial_path.exists());
        assert!(!fixture.paths.partial_metadata_path.exists());
        Ok(())
    }

    #[test]
    fn pause_retention_keep_leaves_partial_and_metadata() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(job, NativeHttpBackendConfig::default())?;

        worker.apply_partial_retention(WorkerStopReason::Paused);

        assert!(fixture.paths.partial_path.exists());
        assert!(fixture.paths.partial_metadata_path.exists());
        Ok(())
    }

    #[test]
    fn cancel_retention_delete_removes_partial_and_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"complete")?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(
            job,
            NativeHttpBackendConfig {
                resume: DownloadResumeConfig {
                    partial_on_failure: PartialRetentionPolicy::Delete,
                    ..DownloadResumeConfig::default()
                },
                ..NativeHttpBackendConfig::default()
            },
        )?;

        worker.apply_partial_retention(WorkerStopReason::Cancelled);

        assert!(!fixture.paths.partial_path.exists());
        assert!(!fixture.paths.partial_metadata_path.exists());
        Ok(())
    }

    #[test]
    fn finalize_download_fails_without_promoting_checksum_mismatch()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ResumeFixture::new()?;
        let job = test_job(fixture.target_path(), b"expected")?;
        fixture
            .paths
            .write_partial_bytes(b"wrong", &fixture.storage)?;
        let verifier = ArtifactVerifier::new(DownloadIntegrityConfig::default());
        let worker = test_worker(job, NativeHttpBackendConfig::default())?;

        let result = worker.finalize_download(&fixture.paths, &verifier);

        assert!(matches!(
            result,
            Err(NativeHttpWorkerError {
                retryable: false,
                ..
            })
        ));
        assert!(!fixture.paths.target_path.exists());
        assert!(fixture.paths.partial_path.exists());
        Ok(())
    }

    #[test]
    fn send_request_classifies_client_error_as_permanent() -> Result<(), Box<dyn std::error::Error>>
    {
        let response = HttpFixture::serve_once("404 Not Found", b"missing")?;
        let runtime = tokio_runtime()?;
        let fixture = ResumeFixture::new()?;
        let mut job = test_job(fixture.target_path(), b"expected")?;
        job.url = DownloadUrl::new(response.url)?;
        let worker = test_worker(job, NativeHttpBackendConfig::default())?;

        let result =
            runtime.block_on(worker.send_request(&fixture.paths, &ResumeDecision::restart()));

        assert!(matches!(
            result,
            Err(NativeHttpWorkerError {
                retryable: false,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn send_request_classifies_server_error_as_retryable() -> Result<(), Box<dyn std::error::Error>>
    {
        let response = HttpFixture::serve_once("503 Service Unavailable", b"try later")?;
        let runtime = tokio_runtime()?;
        let fixture = ResumeFixture::new()?;
        let mut job = test_job(fixture.target_path(), b"expected")?;
        job.url = DownloadUrl::new(response.url)?;
        let worker = test_worker(job, NativeHttpBackendConfig::default())?;

        let result =
            runtime.block_on(worker.send_request(&fixture.paths, &ResumeDecision::restart()));

        assert!(matches!(
            result,
            Err(NativeHttpWorkerError {
                retryable: true,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn send_request_restarts_when_resume_response_is_not_partial()
    -> Result<(), Box<dyn std::error::Error>> {
        let response = HttpFixture::serve_once("200 OK", b"complete")?;
        let runtime = tokio_runtime()?;
        let fixture = ResumeFixture::new()?;
        let mut job = test_job(fixture.target_path(), b"complete")?;
        job.url = DownloadUrl::new(response.url.clone())?;
        fixture.write_partial(&job, b"part", Some(etag_validator()))?;
        let worker = test_worker(job, resume_enabled_config())?;
        let resume = ResumeDecision {
            should_resume: true,
            downloaded: 4,
            if_range: Some("\"resume\"".to_string()),
        };

        let result = runtime.block_on(worker.send_request(&fixture.paths, &resume));

        assert!(result.is_ok());
        assert!(!fixture.paths.partial_path.exists());
        assert!(
            response
                .request()?
                .to_ascii_lowercase()
                .contains("range: bytes=4-")
        );
        Ok(())
    }

    fn test_worker(
        job: DownloadJobSpec,
        config: NativeHttpBackendConfig,
    ) -> Result<NativeHttpWorker, Box<dyn std::error::Error>> {
        let (_stop_sender, stop_receiver) = oneshot::channel();
        Ok(NativeHttpWorker {
            job,
            client: reqwest::Client::new(),
            report_handle: service_handle()?,
            config,
            stop_receiver,
        })
    }

    fn service_handle() -> Result<DownloadServiceHandle, Box<dyn std::error::Error>> {
        let (_service_loop, handle, _events) = DownloadServiceLoop::with_backend_factory(
            DownloadPlan::new(Vec::new())?,
            DownloadManagerConfig::default(),
            |_| NoopBackend,
        )?;
        Ok(handle)
    }

    fn test_job(
        target_path: impl AsRef<Path>,
        body: &[u8],
    ) -> Result<DownloadJobSpec, Box<dyn std::error::Error>> {
        Ok(DownloadJobSpec {
            id: DownloadJobId::new("artifact")?,
            url: DownloadUrl::new("http://127.0.0.1/artifact")?,
            host: Some("127.0.0.1".to_string()),
            target_path: target_path.as_ref().to_path_buf(),
            expected_size: Some(body.len() as u64),
            checksum: Some(Checksum {
                algorithm: ChecksumAlgorithm::Sha1,
                value: sha1_hex(body),
            }),
            kind: DownloadArtifactKind::Library,
            policy: DownloadJobPolicy::default(),
        })
    }

    fn resume_enabled_config() -> NativeHttpBackendConfig {
        NativeHttpBackendConfig {
            resume: DownloadResumeConfig {
                min_size: 1,
                validator_policy: ResumeValidatorPolicy::RequireMatch,
                ..DownloadResumeConfig::default()
            },
            ..NativeHttpBackendConfig::default()
        }
    }

    fn etag_validator() -> ResumeValidator {
        ResumeValidator {
            etag: Some("\"resume\"".to_string()),
            last_modified: None,
        }
    }

    fn sha1_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn tokio_runtime() -> Result<tokio::runtime::Runtime, Box<dyn std::error::Error>> {
        Ok(tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?)
    }

    #[derive(Debug)]
    struct ResumeFixture {
        _temp_dir: tempfile::TempDir,
        storage: DownloadStorageConfig,
        paths: DownloadStoragePaths,
    }

    impl ResumeFixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let temp_dir = tempfile::tempdir()?;
            let storage = DownloadStorageConfig::default();
            let paths =
                DownloadStoragePaths::for_target(temp_dir.path().join("artifact.jar"), &storage);
            Ok(Self {
                _temp_dir: temp_dir,
                storage,
                paths,
            })
        }

        fn target_path(&self) -> PathBuf {
            self.paths.target_path.clone()
        }

        fn write_partial(
            &self,
            job: &DownloadJobSpec,
            bytes: &[u8],
            validator: Option<ResumeValidator>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.paths.write_partial_bytes(bytes, &self.storage)?;
            self.paths.write_partial_metadata(
                &PartialDownloadMetadata::for_job(job, bytes.len() as u64, validator),
                &self.storage,
            )?;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct HttpFixture {
        url: String,
        request_receiver: mpsc::Receiver<String>,
    }

    impl HttpFixture {
        fn serve_once(
            status: &'static str,
            body: &'static [u8],
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let (request_sender, request_receiver) = mpsc::channel();
            thread::spawn(move || {
                let Ok((mut stream, _peer)) = listener.accept() else {
                    return;
                };
                let request = read_http_request(&mut stream);
                let _ = request_sender.send(request);
                let headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(body);
            });
            Ok(Self {
                url: format!("http://{address}/artifact"),
                request_receiver,
            })
        }

        fn request(&self) -> Result<String, Box<dyn std::error::Error>> {
            Ok(self.request_receiver.recv()?)
        }
    }

    fn read_http_request(stream: &mut impl Read) -> String {
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

    #[derive(Debug)]
    struct NoopBackend;

    impl DownloadBackend for NoopBackend {
        fn start_job(&mut self, _job: DownloadJobSpec) -> Result<(), DownloadBackendError> {
            Ok(())
        }

        fn stop_worker(
            &mut self,
            _id: &DownloadJobId,
            _reason: WorkerStopReason,
        ) -> Result<(), DownloadBackendError> {
            Ok(())
        }
    }
}
