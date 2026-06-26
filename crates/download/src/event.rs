use crate::{DownloadJobId, DownloadJobSpec};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanTerminalState {
    Completed,
    Failed,
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
