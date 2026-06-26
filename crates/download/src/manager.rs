use std::collections::BTreeMap;

use crate::{
    DownloadCommand, DownloadEvent, DownloadJobId, DownloadJobSpec, DownloadJobState,
    DownloadManagerAction, DownloadManagerConfig, DownloadManagerError, DownloadPlan,
    PlanTerminalState, WorkerReport, WorkerStopReason,
};

#[derive(Debug)]
pub struct DownloadManagerState {
    pub(crate) config: DownloadManagerConfig,
    pub(crate) jobs: BTreeMap<DownloadJobId, JobRuntimeState>,
    events: Vec<DownloadEvent>,
    terminal_state: Option<PlanTerminalState>,
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
            terminal_state: None,
        })
    }

    pub const fn terminal_state(&self) -> Option<PlanTerminalState> {
        self.terminal_state
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
                if self.job_accepts_worker_report(&id)? {
                    self.events.push(DownloadEvent::JobProgress {
                        id,
                        downloaded,
                        total,
                    });
                }
                Ok(Vec::new())
            }
            WorkerReport::Completed { id } => {
                if self.job_accepts_worker_report(&id)? {
                    let job = self.ensure_job_mut(&id)?;
                    job.state = DownloadJobState::Completed;
                    self.events.push(DownloadEvent::JobCompleted { id });
                    self.emit_plan_terminal_event();
                }
                Ok(Vec::new())
            }
            WorkerReport::Failed {
                id,
                error,
                retryable,
            } => self.fail_job(id, error, retryable),
            WorkerReport::Stopped { id, reason } => {
                match reason {
                    WorkerStopReason::Paused => {
                        if matches!(self.state(&id), Some(DownloadJobState::Pausing { .. })) {
                            self.ensure_job_mut(&id)?.state = DownloadJobState::Paused;
                            self.events.push(DownloadEvent::JobPaused { id });
                        } else {
                            self.ensure_job(&id)?;
                        }
                    }
                    WorkerStopReason::Cancelled => {
                        if matches!(self.state(&id), Some(DownloadJobState::Cancelling { .. })) {
                            self.ensure_job_mut(&id)?.state = DownloadJobState::Cancelled;
                            self.events.push(DownloadEvent::JobCancelled { id });
                            self.emit_plan_terminal_event();
                        } else {
                            self.ensure_job(&id)?;
                        }
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
            self.terminal_state = None;
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
        if !matches!(
            job.state,
            DownloadJobState::Running { .. }
                | DownloadJobState::Pausing { .. }
                | DownloadJobState::Cancelling { .. }
        ) {
            return Ok(Vec::new());
        }

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

    fn job_accepts_worker_report(&self, id: &DownloadJobId) -> Result<bool, DownloadManagerError> {
        let job = self.ensure_job(id)?;
        Ok(matches!(
            job.state,
            DownloadJobState::Running { .. }
                | DownloadJobState::Pausing { .. }
                | DownloadJobState::Cancelling { .. }
        ))
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
        if self.terminal_state.is_some() {
            return;
        }

        if self
            .jobs
            .values()
            .all(|job| matches!(job.state, DownloadJobState::Completed))
        {
            self.terminal_state = Some(PlanTerminalState::Completed);
            self.events.push(DownloadEvent::PlanCompleted);
            return;
        }

        if self.jobs.values().any(|job| {
            matches!(
                job.state,
                DownloadJobState::Failed { .. } | DownloadJobState::Cancelled
            )
        }) {
            self.terminal_state = Some(PlanTerminalState::Failed);
            self.events.push(DownloadEvent::PlanFailed);
        }
    }
}

#[derive(Debug)]
pub struct JobRuntimeState {
    pub(crate) spec: DownloadJobSpec,
    pub(crate) state: DownloadJobState,
    pub(crate) attempts: u8,
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
