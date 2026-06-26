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
        let Some(mut control) = self.active_workers.remove(id) else {
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
        if self.config.resume.mode == DownloadResumeMode::Disabled {
            return ResumeDecision::restart();
        }
        let Ok(metadata) = paths.read_partial_metadata() else {
            return ResumeDecision::restart();
        };
        if !metadata_matches_job(&metadata, &self.job) {
            return ResumeDecision::restart();
        }
        let Ok(partial_metadata) = fs::metadata(&paths.partial_path) else {
            return ResumeDecision::restart();
        };
        if partial_metadata.len() != metadata.downloaded {
            return ResumeDecision::restart();
        }
        if metadata.downloaded < self.config.resume.min_size {
            return ResumeDecision::restart();
        }
        let if_range = metadata.validator.as_ref().and_then(|validator| {
            validator
                .etag
                .clone()
                .or_else(|| validator.last_modified.clone())
        });
        if if_range.is_none()
            && self.config.resume.validator_policy == ResumeValidatorPolicy::RequireMatch
        {
            return ResumeDecision::restart();
        }
        ResumeDecision {
            should_resume: true,
            downloaded: metadata.downloaded,
            if_range,
        }
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
