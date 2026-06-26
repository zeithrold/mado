use std::time::Duration;

use crate::DownloadConfigError;

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
