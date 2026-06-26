use crate::{
    DownloadBackend, DownloadCommand, DownloadEvent, DownloadManagerAction, DownloadManagerConfig,
    DownloadManagerError, DownloadManagerState, DownloadPlan, DownloadServiceError, WorkerReport,
};
use std::sync::mpsc::{self, Receiver, SendError, Sender, TryRecvError};

#[derive(Debug)]
pub struct DownloadService<B> {
    pub(crate) manager: DownloadManagerState,
    pub(crate) backend: B,
}

impl<B> DownloadService<B>
where
    B: DownloadBackend,
{
    pub fn new(
        plan: DownloadPlan,
        config: DownloadManagerConfig,
        backend: B,
    ) -> Result<Self, DownloadManagerError> {
        let manager = DownloadManagerState::new(plan, config)?;
        Ok(Self { manager, backend })
    }

    pub const fn manager(&self) -> &DownloadManagerState {
        &self.manager
    }

    pub fn drain_events(&mut self) -> Vec<DownloadEvent> {
        self.manager.drain_events()
    }

    pub fn apply_command(&mut self, command: DownloadCommand) -> Result<(), DownloadServiceError> {
        let actions = self.manager.apply_command(command)?;
        self.apply_actions(actions)?;
        self.schedule_ready_jobs()
    }

    pub fn apply_worker_report(
        &mut self,
        report: WorkerReport,
    ) -> Result<(), DownloadServiceError> {
        let actions = self.manager.apply_worker_report(report)?;
        self.apply_actions(actions)?;
        self.schedule_ready_jobs()
    }

    pub fn schedule_ready_jobs(&mut self) -> Result<(), DownloadServiceError> {
        let actions = self.manager.schedule_ready_jobs();
        self.apply_actions(actions)
    }

    fn apply_actions(
        &mut self,
        actions: Vec<DownloadManagerAction>,
    ) -> Result<(), DownloadServiceError> {
        for action in actions {
            match action {
                DownloadManagerAction::StartJob(job) => self.backend.start_job(job)?,
                DownloadManagerAction::StopWorker { id, reason } => {
                    self.backend.stop_worker(&id, reason)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadServiceInput {
    Command(DownloadCommand),
    WorkerReport(WorkerReport),
}

#[derive(Debug, Clone)]
pub struct DownloadServiceHandle {
    input_sender: Sender<DownloadServiceInput>,
}

impl DownloadServiceHandle {
    pub fn send_command(
        &self,
        command: DownloadCommand,
    ) -> Result<(), SendError<DownloadServiceInput>> {
        self.input_sender
            .send(DownloadServiceInput::Command(command))
    }

    pub fn send_worker_report(
        &self,
        report: WorkerReport,
    ) -> Result<(), SendError<DownloadServiceInput>> {
        self.input_sender
            .send(DownloadServiceInput::WorkerReport(report))
    }
}

#[derive(Debug)]
pub struct DownloadEventStream {
    event_receiver: Receiver<DownloadEvent>,
}

impl DownloadEventStream {
    pub fn try_recv(&self) -> Result<DownloadEvent, TryRecvError> {
        self.event_receiver.try_recv()
    }

    pub fn drain_available(&self) -> Vec<DownloadEvent> {
        let mut events = Vec::new();
        loop {
            match self.event_receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return events,
            }
        }
    }
}

#[derive(Debug)]
pub struct DownloadServiceLoop<B> {
    service: DownloadService<B>,
    input_receiver: Receiver<DownloadServiceInput>,
    event_sender: Sender<DownloadEvent>,
    started: bool,
}

impl<B> DownloadServiceLoop<B>
where
    B: DownloadBackend,
{
    pub fn new(
        plan: DownloadPlan,
        config: DownloadManagerConfig,
        backend: B,
    ) -> Result<(Self, DownloadServiceHandle, DownloadEventStream), DownloadManagerError> {
        let service = DownloadService::new(plan, config, backend)?;
        let (input_sender, input_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let service_loop = Self {
            service,
            input_receiver,
            event_sender,
            started: false,
        };
        let handle = DownloadServiceHandle { input_sender };
        let event_stream = DownloadEventStream { event_receiver };
        Ok((service_loop, handle, event_stream))
    }

    pub const fn service(&self) -> &DownloadService<B> {
        &self.service
    }

    pub const fn is_started(&self) -> bool {
        self.started
    }

    pub fn start(&mut self) -> Result<(), DownloadServiceError> {
        if !self.started {
            self.started = true;
            self.service.schedule_ready_jobs()?;
            self.publish_pending_events()?;
        }
        Ok(())
    }

    pub fn run_until_idle(&mut self) -> Result<usize, DownloadServiceError> {
        let mut processed = 0;
        loop {
            match self.input_receiver.try_recv() {
                Ok(input) => {
                    self.apply_input(input)?;
                    processed += 1;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(processed),
            }
        }
    }

    fn apply_input(&mut self, input: DownloadServiceInput) -> Result<(), DownloadServiceError> {
        match input {
            DownloadServiceInput::Command(command) => self.service.apply_command(command)?,
            DownloadServiceInput::WorkerReport(report) => {
                self.service.apply_worker_report(report)?;
            }
        }
        self.publish_pending_events()
    }

    fn publish_pending_events(&mut self) -> Result<(), DownloadServiceError> {
        for event in self.service.drain_events() {
            self.event_sender
                .send(event)
                .map_err(|_| DownloadServiceError::EventStreamClosed)?;
        }
        Ok(())
    }
}
