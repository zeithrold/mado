use crate::{DownloadBackendError, DownloadJobId, DownloadJobSpec, WorkerStopReason};

pub trait DownloadBackend {
    fn start_job(&mut self, job: DownloadJobSpec) -> Result<(), DownloadBackendError>;

    fn stop_worker(
        &mut self,
        id: &DownloadJobId,
        reason: WorkerStopReason,
    ) -> Result<(), DownloadBackendError>;
}
