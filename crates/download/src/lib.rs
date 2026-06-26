mod backend;
mod config;
mod error;
mod event;
mod manager;
mod plan;
mod service;
mod storage;
mod verify;

pub use backend::DownloadBackend;
pub use config::{
    DownloadConcurrencyConfig, DownloadEventConfig, DownloadIntegrityConfig, DownloadManagerConfig,
    DownloadResumeConfig, DownloadResumeMode, DownloadRetryConfig, DownloadStorageConfig,
    DownloadTimeoutConfig, PartialRetentionPolicy, ResumeValidatorPolicy,
};
pub use error::{
    ArtifactVerifyError, DownloadBackendError, DownloadConfigError, DownloadManagerError,
    DownloadPlanError, DownloadServiceError,
};
pub use event::{
    DownloadCommand, DownloadEvent, DownloadJobState, DownloadManagerAction, PlanTerminalState,
    WorkerReport, WorkerStopReason,
};
pub use manager::DownloadManagerState;
pub use plan::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadJobId, DownloadJobPolicy,
    DownloadJobSpec, DownloadPlan, DownloadUrl,
};
pub use service::{
    DownloadEventStream, DownloadService, DownloadServiceHandle, DownloadServiceInput,
    DownloadServiceLoop,
};
pub use storage::{DownloadStoragePaths, PartialDownloadMetadata, ResumeValidator};
pub use verify::{ArtifactVerification, ArtifactVerifier};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

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

    #[test]
    fn terminal_plan_failed_event_is_emitted_once() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(1)?;
        let id = job_id("job-0")?;
        let _initial_events = manager.drain_events();

        manager.apply_command(DownloadCommand::CancelJob(id.clone()))?;
        manager.apply_command(DownloadCommand::CancelJob(id))?;

        let events = manager.drain_events();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, DownloadEvent::PlanFailed))
                .count(),
            1
        );
        assert_eq!(manager.terminal_state(), Some(PlanTerminalState::Failed));
        Ok(())
    }

    #[test]
    fn completed_job_ignores_late_commands_and_worker_reports()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(1)?;
        let id = job_id("job-0")?;
        let _actions = manager.schedule_ready_jobs();
        manager.apply_worker_report(WorkerReport::Completed { id: id.clone() })?;
        let _events = manager.drain_events();

        manager.apply_command(DownloadCommand::PauseJob(id.clone()))?;
        manager.apply_command(DownloadCommand::CancelJob(id.clone()))?;
        manager.apply_command(DownloadCommand::RetryJob(id.clone()))?;
        manager.apply_worker_report(WorkerReport::Failed {
            id: id.clone(),
            error: "late failure".to_string(),
            retryable: false,
        })?;
        manager.apply_worker_report(WorkerReport::Stopped {
            id: id.clone(),
            reason: WorkerStopReason::Cancelled,
        })?;

        assert_eq!(manager.state(&id), Some(&DownloadJobState::Completed));
        assert!(manager.drain_events().is_empty());
        Ok(())
    }

    #[test]
    fn retry_after_terminal_failure_opens_a_new_terminal_phase()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = manager_with_jobs(1)?;
        let id = job_id("job-0")?;
        let _actions = manager.schedule_ready_jobs();

        manager.apply_worker_report(WorkerReport::Failed {
            id: id.clone(),
            error: "permanent failure".to_string(),
            retryable: false,
        })?;
        assert_eq!(manager.terminal_state(), Some(PlanTerminalState::Failed));

        manager.apply_command(DownloadCommand::RetryJob(id.clone()))?;
        assert_eq!(manager.terminal_state(), None);

        let _actions = manager.schedule_ready_jobs();
        manager.apply_worker_report(WorkerReport::Completed { id })?;

        assert_eq!(manager.terminal_state(), Some(PlanTerminalState::Completed));
        assert!(
            manager
                .drain_events()
                .contains(&DownloadEvent::PlanCompleted)
        );
        Ok(())
    }

    #[test]
    fn storage_paths_append_configured_suffixes_to_target_path() {
        let config = DownloadStorageConfig::default();
        let paths = DownloadStoragePaths::for_target("libraries/example.jar", &config);

        assert_eq!(paths.target_path, PathBuf::from("libraries/example.jar"));
        assert_eq!(
            paths.partial_path,
            PathBuf::from("libraries/example.jar.part")
        );
        assert_eq!(
            paths.partial_metadata_path,
            PathBuf::from("libraries/example.jar.part.json")
        );
    }

    #[test]
    fn artifact_verifier_accepts_matching_size_and_sha1() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = tempfile::tempdir()?;
        let path = temp_dir.path().join("artifact.bin");
        fs::write(&path, b"hello")?;
        let mut spec = job(job_id("artifact")?)?;
        spec.target_path = path.clone();
        spec.expected_size = Some(5);
        spec.checksum = Some(Checksum {
            algorithm: ChecksumAlgorithm::Sha1,
            value: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".to_string(),
        });
        let verifier = ArtifactVerifier::new(DownloadIntegrityConfig::default());

        let verification = verifier.verify_job_target(&spec)?;

        assert_eq!(verification.path, path);
        assert_eq!(verification.size, 5);
        Ok(())
    }

    #[test]
    fn artifact_verifier_rejects_checksum_mismatch() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let path = temp_dir.path().join("artifact.bin");
        fs::write(&path, b"hello")?;
        let mut spec = job(job_id("artifact")?)?;
        spec.target_path = path.clone();
        spec.expected_size = None;
        spec.checksum = Some(Checksum {
            algorithm: ChecksumAlgorithm::Sha256,
            value: "definitely-not-the-sha256".to_string(),
        });
        let verifier = ArtifactVerifier::new(DownloadIntegrityConfig::default());

        let result = verifier.verify_job_target(&spec);

        assert!(matches!(
            result,
            Err(ArtifactVerifyError::ChecksumMismatch { path: mismatch_path, .. })
                if mismatch_path == path
        ));
        Ok(())
    }

    #[test]
    fn service_dispatches_manager_actions_to_backend() -> Result<(), Box<dyn std::error::Error>> {
        let id = job_id("job-0")?;
        let plan = DownloadPlan::new(vec![job(id.clone())?])?;
        let backend = RecordingBackend::default();
        let mut service = DownloadService::new(plan, DownloadManagerConfig::default(), backend)?;

        service.schedule_ready_jobs()?;
        service.apply_command(DownloadCommand::PauseJob(id.clone()))?;

        assert_eq!(service.backend.started, vec![id.clone()]);
        assert_eq!(
            service.backend.stopped,
            vec![(id, WorkerStopReason::Paused)]
        );
        Ok(())
    }

    #[test]
    fn service_loop_publishes_started_and_completed_events_to_stream()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = job_id("job-0")?;
        let plan = DownloadPlan::new(vec![job(id.clone())?])?;
        let backend = RecordingBackend::default();
        let (mut service_loop, handle, events) =
            DownloadServiceLoop::new(plan, DownloadManagerConfig::default(), backend)?;

        service_loop.start()?;
        assert_eq!(
            events.drain_available(),
            vec![
                DownloadEvent::JobQueued { id: id.clone() },
                DownloadEvent::JobStarted {
                    id: id.clone(),
                    attempt: 1,
                },
            ]
        );

        handle.send_worker_report(WorkerReport::Completed { id: id.clone() })?;
        assert_eq!(service_loop.run_until_idle()?, 1);

        assert_eq!(
            events.drain_available(),
            vec![
                DownloadEvent::JobCompleted { id },
                DownloadEvent::PlanCompleted,
            ]
        );
        Ok(())
    }

    #[test]
    fn service_loop_preserves_input_order_in_single_mailbox()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = job_id("job-0")?;
        let plan = DownloadPlan::new(vec![job(id.clone())?])?;
        let backend = RecordingBackend::default();
        let (mut service_loop, handle, events) =
            DownloadServiceLoop::new(plan, DownloadManagerConfig::default(), backend)?;
        service_loop.start()?;
        let _initial_events = events.drain_available();

        handle.send_command(DownloadCommand::PauseJob(id.clone()))?;
        handle.send_worker_report(WorkerReport::Stopped {
            id: id.clone(),
            reason: WorkerStopReason::Paused,
        })?;

        assert_eq!(service_loop.run_until_idle()?, 2);
        assert_eq!(service_loop.service().backend.stopped.len(), 1);
        assert_eq!(
            service_loop.service().manager.state(&id),
            Some(&DownloadJobState::Paused)
        );
        assert_eq!(
            events.drain_available(),
            vec![
                DownloadEvent::JobPauseRequested { id: id.clone() },
                DownloadEvent::JobPaused { id },
            ]
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

    #[derive(Default)]
    struct RecordingBackend {
        started: Vec<DownloadJobId>,
        stopped: Vec<(DownloadJobId, WorkerStopReason)>,
    }

    impl DownloadBackend for RecordingBackend {
        fn start_job(&mut self, job: DownloadJobSpec) -> Result<(), DownloadBackendError> {
            self.started.push(job.id);
            Ok(())
        }

        fn stop_worker(
            &mut self,
            id: &DownloadJobId,
            reason: WorkerStopReason,
        ) -> Result<(), DownloadBackendError> {
            self.stopped.push((id.clone(), reason));
            Ok(())
        }
    }
}
