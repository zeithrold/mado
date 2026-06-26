use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DownloadJobId(String);

impl DownloadJobId {
    pub fn new(value: impl Into<String>) -> Result<Self, DownloadPlanError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DownloadPlanError::EmptyJobId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadUrl(String);

impl DownloadUrl {
    pub fn new(value: impl Into<String>) -> Result<Self, DownloadPlanError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DownloadPlanError::EmptyUrl);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadPlan {
    pub jobs: Vec<DownloadJobSpec>,
}

impl DownloadPlan {
    pub fn new(jobs: Vec<DownloadJobSpec>) -> Result<Self, DownloadPlanError> {
        let mut ids = BTreeSet::new();
        for job in &jobs {
            if !ids.insert(job.id.clone()) {
                return Err(DownloadPlanError::DuplicateJobId { id: job.id.clone() });
            }
        }
        Ok(Self { jobs })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadJobSpec {
    pub id: DownloadJobId,
    pub url: DownloadUrl,
    pub host: Option<String>,
    pub target_path: PathBuf,
    pub expected_size: Option<u64>,
    pub checksum: Option<Checksum>,
    pub kind: DownloadArtifactKind,
    pub policy: DownloadJobPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    pub algorithm: ChecksumAlgorithm,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    Sha1,
    Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadArtifactKind {
    VersionMetadata,
    ClientJar,
    Library,
    Asset,
    Native,
    JavaRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadJobPolicy {
    pub resumable: bool,
    pub retryable: bool,
}

impl Default for DownloadJobPolicy {
    fn default() -> Self {
        Self {
            resumable: true,
            retryable: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadManagerConfig {
    pub concurrency: DownloadConcurrencyConfig,
    pub retry: DownloadRetryConfig,
    pub resume: DownloadResumeConfig,
    pub integrity: DownloadIntegrityConfig,
    pub events: DownloadEventConfig,
    pub storage: DownloadStorageConfig,
    pub timeouts: DownloadTimeoutConfig,
}

impl DownloadManagerConfig {
    pub const fn validate(&self) -> Result<(), DownloadConfigError> {
        if self.concurrency.global_limit == 0 {
            return Err(DownloadConfigError::ZeroGlobalConcurrency);
        }
        if self.concurrency.per_host_limit == 0 {
            return Err(DownloadConfigError::ZeroPerHostConcurrency);
        }
        if self.concurrency.per_host_limit > self.concurrency.global_limit {
            return Err(DownloadConfigError::PerHostExceedsGlobal);
        }
        if self.concurrency.queue_capacity == 0 {
            return Err(DownloadConfigError::ZeroQueueCapacity);
        }
        if self.retry.max_attempts == 0 {
            return Err(DownloadConfigError::ZeroRetryAttempts);
        }
        if self.events.event_buffer == 0 {
            return Err(DownloadConfigError::ZeroEventBuffer);
        }
        if self.storage.temp_suffix.is_empty() {
            return Err(DownloadConfigError::EmptyTempSuffix);
        }
        if self.storage.metadata_suffix.is_empty() {
            return Err(DownloadConfigError::EmptyMetadataSuffix);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadConcurrencyConfig {
    pub global_limit: usize,
    pub per_host_limit: usize,
    pub queue_capacity: usize,
}

impl Default for DownloadConcurrencyConfig {
    fn default() -> Self {
        Self {
            global_limit: 16,
            per_host_limit: 6,
            queue_capacity: 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRetryConfig {
    pub max_attempts: u8,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub jitter: bool,
    pub retry_transient_http: bool,
}

impl Default for DownloadRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(300),
            max_backoff: Duration::from_secs(5),
            jitter: true,
            retry_transient_http: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadResumeConfig {
    pub mode: DownloadResumeMode,
    pub min_size: u64,
    pub validator_policy: ResumeValidatorPolicy,
    pub partial_on_pause: PartialRetentionPolicy,
    pub partial_on_failure: PartialRetentionPolicy,
}

impl Default for DownloadResumeConfig {
    fn default() -> Self {
        Self {
            mode: DownloadResumeMode::Enabled,
            min_size: 1024 * 1024,
            validator_policy: ResumeValidatorPolicy::RequireMatch,
            partial_on_pause: PartialRetentionPolicy::Keep,
            partial_on_failure: PartialRetentionPolicy::Keep,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadResumeMode {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeValidatorPolicy {
    RequireMatch,
    AllowMissingValidator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialRetentionPolicy {
    Keep,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadIntegrityConfig {
    pub require_checksum_when_available: bool,
    pub require_size_when_available: bool,
    pub checksum_mismatch_redownload_once: bool,
}

impl Default for DownloadIntegrityConfig {
    fn default() -> Self {
        Self {
            require_checksum_when_available: true,
            require_size_when_available: true,
            checksum_mismatch_redownload_once: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadEventConfig {
    pub progress_interval: Duration,
    pub min_progress_bytes: u64,
    pub event_buffer: usize,
}

impl Default for DownloadEventConfig {
    fn default() -> Self {
        Self {
            progress_interval: Duration::from_millis(100),
            min_progress_bytes: 64 * 1024,
            event_buffer: 4096,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadStorageConfig {
    pub temp_suffix: String,
    pub metadata_suffix: String,
    pub fsync_on_complete: bool,
    pub atomic_rename: bool,
}

impl Default for DownloadStorageConfig {
    fn default() -> Self {
        Self {
            temp_suffix: ".part".to_string(),
            metadata_suffix: ".part.json".to_string(),
            fsync_on_complete: false,
            atomic_rename: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTimeoutConfig {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub idle_timeout: Duration,
}

impl Default for DownloadTimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_mins(1),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadCommand {
    PauseJob(DownloadJobId),
    ResumeJob(DownloadJobId),
    CancelJob(DownloadJobId),
    RetryJob(DownloadJobId),
    PauseAll,
    ResumeAll,
    CancelAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    JobQueued {
        id: DownloadJobId,
    },
    JobStarted {
        id: DownloadJobId,
        attempt: u8,
    },
    JobPauseRequested {
        id: DownloadJobId,
    },
    JobPaused {
        id: DownloadJobId,
    },
    JobResumed {
        id: DownloadJobId,
    },
    JobCancelRequested {
        id: DownloadJobId,
    },
    JobCancelled {
        id: DownloadJobId,
    },
    JobRetryScheduled {
        id: DownloadJobId,
        attempt: u8,
    },
    JobProgress {
        id: DownloadJobId,
        downloaded: u64,
        total: Option<u64>,
    },
    JobCompleted {
        id: DownloadJobId,
    },
    JobFailed {
        id: DownloadJobId,
        error: String,
    },
    PlanCompleted,
    PlanFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadJobState {
    Pending,
    Running { attempt: u8 },
    Pausing { attempt: u8 },
    Paused,
    Cancelling { attempt: u8 },
    Completed,
    Failed { attempts: u8, error: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerReport {
    Progress {
        id: DownloadJobId,
        downloaded: u64,
        total: Option<u64>,
    },
    Completed {
        id: DownloadJobId,
    },
    Failed {
        id: DownloadJobId,
        error: String,
        retryable: bool,
    },
    Stopped {
        id: DownloadJobId,
        reason: WorkerStopReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStopReason {
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadManagerAction {
    StartJob(DownloadJobSpec),
    StopWorker {
        id: DownloadJobId,
        reason: WorkerStopReason,
    },
}

#[derive(Debug)]
pub struct DownloadManagerState {
    config: DownloadManagerConfig,
    jobs: BTreeMap<DownloadJobId, JobRuntimeState>,
    events: Vec<DownloadEvent>,
}

impl DownloadManagerState {
    pub fn new(
        plan: DownloadPlan,
        config: DownloadManagerConfig,
    ) -> Result<Self, DownloadManagerError> {
        config.validate()?;

        let mut jobs = BTreeMap::new();
        let mut events = Vec::with_capacity(plan.jobs.len());
        for spec in plan.jobs {
            let id = spec.id.clone();
            events.push(DownloadEvent::JobQueued { id: id.clone() });
            jobs.insert(id, JobRuntimeState::new(spec));
        }

        Ok(Self {
            config,
            jobs,
            events,
        })
    }

    pub fn state(&self, id: &DownloadJobId) -> Option<&DownloadJobState> {
        self.jobs.get(id).map(|job| &job.state)
    }

    pub fn drain_events(&mut self) -> Vec<DownloadEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn apply_command(
        &mut self,
        command: DownloadCommand,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        match command {
            DownloadCommand::PauseJob(id) => self.pause_job(&id),
            DownloadCommand::ResumeJob(id) => self.resume_job(&id),
            DownloadCommand::CancelJob(id) => self.cancel_job(&id),
            DownloadCommand::RetryJob(id) => self.retry_job(&id),
            DownloadCommand::PauseAll => self.apply_to_all(Self::pause_job),
            DownloadCommand::ResumeAll => self.apply_to_all(Self::resume_job),
            DownloadCommand::CancelAll => self.apply_to_all(Self::cancel_job),
        }
    }

    pub fn apply_worker_report(
        &mut self,
        report: WorkerReport,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        match report {
            WorkerReport::Progress {
                id,
                downloaded,
                total,
            } => {
                self.ensure_job(&id)?;
                self.events.push(DownloadEvent::JobProgress {
                    id,
                    downloaded,
                    total,
                });
                Ok(Vec::new())
            }
            WorkerReport::Completed { id } => {
                let job = self.ensure_job_mut(&id)?;
                job.state = DownloadJobState::Completed;
                self.events.push(DownloadEvent::JobCompleted { id });
                self.emit_plan_terminal_event();
                Ok(Vec::new())
            }
            WorkerReport::Failed {
                id,
                error,
                retryable,
            } => self.fail_job(id, error, retryable),
            WorkerReport::Stopped { id, reason } => {
                self.ensure_job_mut(&id)?;
                match reason {
                    WorkerStopReason::Paused => {
                        self.ensure_job_mut(&id)?.state = DownloadJobState::Paused;
                        self.events.push(DownloadEvent::JobPaused { id });
                    }
                    WorkerStopReason::Cancelled => {
                        self.ensure_job_mut(&id)?.state = DownloadJobState::Cancelled;
                        self.events.push(DownloadEvent::JobCancelled { id });
                        self.emit_plan_terminal_event();
                    }
                }
                Ok(Vec::new())
            }
        }
    }

    pub fn schedule_ready_jobs(&mut self) -> Vec<DownloadManagerAction> {
        let mut actions = Vec::new();
        while self.running_count() < self.config.concurrency.global_limit {
            let Some(id) = self.next_schedulable_pending_job_id() else {
                break;
            };
            let Some(candidate_spec) = self.jobs.get(&id).map(|job| job.spec.clone()) else {
                break;
            };
            let Some(job) = self.jobs.get_mut(&id) else {
                break;
            };
            job.attempts = job.attempts.saturating_add(1);
            job.state = DownloadJobState::Running {
                attempt: job.attempts,
            };
            self.events.push(DownloadEvent::JobStarted {
                id: id.clone(),
                attempt: job.attempts,
            });
            actions.push(DownloadManagerAction::StartJob(candidate_spec));
        }
        actions
    }

    fn pause_job(
        &mut self,
        id: &DownloadJobId,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        let job = self.ensure_job_mut(id)?;
        match job.state {
            DownloadJobState::Pending => {
                job.state = DownloadJobState::Paused;
                self.events
                    .push(DownloadEvent::JobPaused { id: id.clone() });
                Ok(Vec::new())
            }
            DownloadJobState::Running { attempt } => {
                job.state = DownloadJobState::Pausing { attempt };
                self.events
                    .push(DownloadEvent::JobPauseRequested { id: id.clone() });
                Ok(vec![DownloadManagerAction::StopWorker {
                    id: id.clone(),
                    reason: WorkerStopReason::Paused,
                }])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn resume_job(
        &mut self,
        id: &DownloadJobId,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        let job = self.ensure_job_mut(id)?;
        if matches!(job.state, DownloadJobState::Paused) {
            job.state = DownloadJobState::Pending;
            self.events
                .push(DownloadEvent::JobResumed { id: id.clone() });
        }
        Ok(Vec::new())
    }

    fn cancel_job(
        &mut self,
        id: &DownloadJobId,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        let job = self.ensure_job_mut(id)?;
        match job.state {
            DownloadJobState::Pending
            | DownloadJobState::Paused
            | DownloadJobState::Failed { .. } => {
                job.state = DownloadJobState::Cancelled;
                self.events
                    .push(DownloadEvent::JobCancelled { id: id.clone() });
                self.emit_plan_terminal_event();
                Ok(Vec::new())
            }
            DownloadJobState::Running { attempt } | DownloadJobState::Pausing { attempt } => {
                job.state = DownloadJobState::Cancelling { attempt };
                self.events
                    .push(DownloadEvent::JobCancelRequested { id: id.clone() });
                Ok(vec![DownloadManagerAction::StopWorker {
                    id: id.clone(),
                    reason: WorkerStopReason::Cancelled,
                }])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn retry_job(
        &mut self,
        id: &DownloadJobId,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        let job = self.ensure_job_mut(id)?;
        if matches!(
            job.state,
            DownloadJobState::Failed { .. } | DownloadJobState::Cancelled
        ) {
            let next_attempt = job.attempts.saturating_add(1);
            job.state = DownloadJobState::Pending;
            self.events.push(DownloadEvent::JobRetryScheduled {
                id: id.clone(),
                attempt: next_attempt,
            });
        }
        Ok(Vec::new())
    }

    fn fail_job(
        &mut self,
        id: DownloadJobId,
        error: String,
        retryable: bool,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        let max_attempts = self.config.retry.max_attempts;
        let job = self.ensure_job_mut(&id)?;
        if retryable && job.spec.policy.retryable && job.attempts < max_attempts {
            let next_attempt = job.attempts.saturating_add(1);
            job.state = DownloadJobState::Pending;
            self.events.push(DownloadEvent::JobRetryScheduled {
                id,
                attempt: next_attempt,
            });
            return Ok(Vec::new());
        }

        job.state = DownloadJobState::Failed {
            attempts: job.attempts,
            error: error.clone(),
        };
        self.events.push(DownloadEvent::JobFailed { id, error });
        self.emit_plan_terminal_event();
        Ok(Vec::new())
    }

    fn apply_to_all(
        &mut self,
        f: fn(
            &mut Self,
            &DownloadJobId,
        ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError>,
    ) -> Result<Vec<DownloadManagerAction>, DownloadManagerError> {
        let ids: Vec<_> = self.jobs.keys().cloned().collect();
        let mut actions = Vec::new();
        for id in ids {
            actions.extend(f(self, &id)?);
        }
        Ok(actions)
    }

    fn ensure_job(&self, id: &DownloadJobId) -> Result<&JobRuntimeState, DownloadManagerError> {
        self.jobs
            .get(id)
            .ok_or_else(|| DownloadManagerError::UnknownJob { id: id.clone() })
    }

    fn ensure_job_mut(
        &mut self,
        id: &DownloadJobId,
    ) -> Result<&mut JobRuntimeState, DownloadManagerError> {
        self.jobs
            .get_mut(id)
            .ok_or_else(|| DownloadManagerError::UnknownJob { id: id.clone() })
    }

    fn running_count(&self) -> usize {
        self.jobs
            .values()
            .filter(|job| {
                matches!(
                    job.state,
                    DownloadJobState::Running { .. }
                        | DownloadJobState::Pausing { .. }
                        | DownloadJobState::Cancelling { .. }
                )
            })
            .count()
    }

    fn next_schedulable_pending_job_id(&self) -> Option<DownloadJobId> {
        self.jobs.iter().find_map(|(id, job)| {
            let is_schedulable = matches!(job.state, DownloadJobState::Pending)
                && host_has_capacity(
                    &self.jobs,
                    &job.spec,
                    self.config.concurrency.per_host_limit,
                );
            is_schedulable.then(|| id.clone())
        })
    }

    fn emit_plan_terminal_event(&mut self) {
        if self
            .jobs
            .values()
            .all(|job| matches!(job.state, DownloadJobState::Completed))
        {
            self.events.push(DownloadEvent::PlanCompleted);
            return;
        }

        if self.jobs.values().any(|job| {
            matches!(
                job.state,
                DownloadJobState::Failed { .. } | DownloadJobState::Cancelled
            )
        }) {
            self.events.push(DownloadEvent::PlanFailed);
        }
    }
}

#[derive(Debug)]
struct JobRuntimeState {
    spec: DownloadJobSpec,
    state: DownloadJobState,
    attempts: u8,
}

impl JobRuntimeState {
    const fn new(spec: DownloadJobSpec) -> Self {
        Self {
            spec,
            state: DownloadJobState::Pending,
            attempts: 0,
        }
    }
}

fn host_has_capacity(
    jobs: &BTreeMap<DownloadJobId, JobRuntimeState>,
    candidate: &DownloadJobSpec,
    per_host_limit: usize,
) -> bool {
    let Some(candidate_host) = &candidate.host else {
        return true;
    };
    let running_for_host = jobs
        .values()
        .filter(|job| {
            job.spec.host.as_ref() == Some(candidate_host)
                && matches!(
                    job.state,
                    DownloadJobState::Running { .. }
                        | DownloadJobState::Pausing { .. }
                        | DownloadJobState::Cancelling { .. }
                )
        })
        .count();
    running_for_host < per_host_limit
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadPlanError {
    #[error("download job id must not be empty")]
    EmptyJobId,
    #[error("download URL must not be empty")]
    EmptyUrl,
    #[error("duplicate download job id: {id:?}")]
    DuplicateJobId { id: DownloadJobId },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadConfigError {
    #[error("global download concurrency must be greater than zero")]
    ZeroGlobalConcurrency,
    #[error("per-host download concurrency must be greater than zero")]
    ZeroPerHostConcurrency,
    #[error("per-host download concurrency must not exceed global concurrency")]
    PerHostExceedsGlobal,
    #[error("download queue capacity must be greater than zero")]
    ZeroQueueCapacity,
    #[error("retry attempts must be greater than zero")]
    ZeroRetryAttempts,
    #[error("download event buffer must be greater than zero")]
    ZeroEventBuffer,
    #[error("partial download suffix must not be empty")]
    EmptyTempSuffix,
    #[error("partial metadata suffix must not be empty")]
    EmptyMetadataSuffix,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadManagerError {
    #[error(transparent)]
    InvalidConfig(#[from] DownloadConfigError),
    #[error("unknown download job: {id:?}")]
    UnknownJob { id: DownloadJobId },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_job_ids() -> Result<(), Box<dyn std::error::Error>> {
        let id = job_id("client")?;
        let result = DownloadPlan::new(vec![job(id.clone())?, job(id)?]);

        assert!(matches!(
            result,
            Err(DownloadPlanError::DuplicateJobId { .. })
        ));
        Ok(())
    }

    #[test]
    fn validates_concurrency_limits() {
        let config = DownloadManagerConfig {
            concurrency: DownloadConcurrencyConfig {
                global_limit: 2,
                per_host_limit: 3,
                queue_capacity: 8,
            },
            ..DownloadManagerConfig::default()
        };

        assert_eq!(
            config.validate(),
            Err(DownloadConfigError::PerHostExceedsGlobal)
        );
    }

    #[test]
    fn scheduling_respects_global_limit() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(3)?;
        manager.config.concurrency.global_limit = 2;

        let actions = manager.schedule_ready_jobs();

        assert_eq!(actions.len(), 2);
        assert_eq!(running_states(&manager), 2);
        Ok(())
    }

    #[test]
    fn scheduling_skips_jobs_blocked_by_per_host_limit() -> Result<(), Box<dyn std::error::Error>> {
        let plan = DownloadPlan::new(vec![
            job_with_host(job_id("a-0")?, "a.example")?,
            job_with_host(job_id("a-1")?, "a.example")?,
            job_with_host(job_id("b-0")?, "b.example")?,
        ])?;
        let config = DownloadManagerConfig {
            concurrency: DownloadConcurrencyConfig {
                global_limit: 2,
                per_host_limit: 1,
                queue_capacity: 8,
            },
            ..DownloadManagerConfig::default()
        };
        let mut manager = DownloadManagerState::new(plan, config)?;

        let actions = manager.schedule_ready_jobs();

        assert_eq!(action_ids(&actions), vec![job_id("a-0")?, job_id("b-0")?]);
        assert_eq!(
            manager.state(&job_id("a-1")?),
            Some(&DownloadJobState::Pending)
        );
        Ok(())
    }

    #[test]
    fn pause_running_job_requests_worker_stop_before_paused_report()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(1)?;
        let id = job_id("job-0")?;
        let _actions = manager.schedule_ready_jobs();

        let actions = manager.apply_command(DownloadCommand::PauseJob(id.clone()))?;

        assert_eq!(
            actions,
            vec![DownloadManagerAction::StopWorker {
                id: id.clone(),
                reason: WorkerStopReason::Paused,
            }]
        );
        assert!(matches!(
            manager.state(&id),
            Some(DownloadJobState::Pausing { .. })
        ));

        manager.apply_worker_report(WorkerReport::Stopped {
            id: id.clone(),
            reason: WorkerStopReason::Paused,
        })?;

        assert_eq!(manager.state(&id), Some(&DownloadJobState::Paused));
        Ok(())
    }

    #[test]
    fn cancel_pending_job_is_terminal() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(1)?;
        let id = job_id("job-0")?;

        let actions = manager.apply_command(DownloadCommand::CancelJob(id.clone()))?;

        assert!(actions.is_empty());
        assert_eq!(manager.state(&id), Some(&DownloadJobState::Cancelled));
        assert!(manager.drain_events().contains(&DownloadEvent::PlanFailed));
        Ok(())
    }

    #[test]
    fn retryable_worker_failure_returns_job_to_pending_until_attempts_exhaust()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(1)?;
        let id = job_id("job-0")?;
        let _actions = manager.schedule_ready_jobs();

        manager.apply_worker_report(WorkerReport::Failed {
            id: id.clone(),
            error: "temporary network failure".to_string(),
            retryable: true,
        })?;

        assert_eq!(manager.state(&id), Some(&DownloadJobState::Pending));
        assert!(
            manager
                .drain_events()
                .contains(&DownloadEvent::JobRetryScheduled { id, attempt: 2 })
        );
        Ok(())
    }

    fn manager_with_jobs(count: usize) -> Result<DownloadManagerState, Box<dyn std::error::Error>> {
        let jobs = (0..count)
            .map(|index| job_id(format!("job-{index}")).and_then(job))
            .collect::<Result<Vec<_>, _>>()?;
        let plan = DownloadPlan::new(jobs)?;
        Ok(DownloadManagerState::new(
            plan,
            DownloadManagerConfig::default(),
        )?)
    }

    fn running_states(manager: &DownloadManagerState) -> usize {
        manager
            .jobs
            .values()
            .filter(|job| matches!(job.state, DownloadJobState::Running { .. }))
            .count()
    }

    fn job(id: DownloadJobId) -> Result<DownloadJobSpec, DownloadPlanError> {
        job_with_host(id, "example.invalid")
    }

    fn job_with_host(
        id: DownloadJobId,
        host: impl Into<String>,
    ) -> Result<DownloadJobSpec, DownloadPlanError> {
        Ok(DownloadJobSpec {
            id,
            url: DownloadUrl::new("https://example.invalid/file")?,
            host: Some(host.into()),
            target_path: PathBuf::from("file"),
            expected_size: Some(1),
            checksum: None,
            kind: DownloadArtifactKind::Library,
            policy: DownloadJobPolicy::default(),
        })
    }

    fn job_id(value: impl Into<String>) -> Result<DownloadJobId, DownloadPlanError> {
        DownloadJobId::new(value)
    }

    fn action_ids(actions: &[DownloadManagerAction]) -> Vec<DownloadJobId> {
        actions
            .iter()
            .filter_map(|action| match action {
                DownloadManagerAction::StartJob(spec) => Some(spec.id.clone()),
                DownloadManagerAction::StopWorker { .. } => None,
            })
            .collect()
    }
}
