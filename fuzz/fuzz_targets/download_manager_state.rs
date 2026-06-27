#![no_main]

use std::collections::BTreeMap;
use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use mado_download::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadCommand,
    DownloadConcurrencyConfig, DownloadEvent, DownloadEventConfig, DownloadJobId,
    DownloadJobPolicy, DownloadJobSpec, DownloadJobState, DownloadManagerAction,
    DownloadManagerConfig, DownloadManagerError, DownloadPlan, DownloadRetryConfig, DownloadUrl,
    PlanTerminalState, WorkerReport, WorkerStopReason,
};

const MAX_JOBS: usize = 16;
const MAX_STEPS: usize = 128;
const HOSTS: [Option<&str>; 5] = [
    Some("assets.example.test"),
    Some("libraries.example.test"),
    Some("meta.example.test"),
    None,
    Some("shared.example.test"),
];

fuzz_target!(|input: &[u8]| {
    let mut reader = FuzzReader::new(input);
    let jobs = build_jobs(&mut reader);
    exercise_plan_validation(&mut reader, &jobs);

    let Some(config) = build_valid_config(&mut reader) else {
        return;
    };
    let Ok(plan) = DownloadPlan::new(jobs.clone()) else {
        return;
    };
    let Ok(mut manager) = mado_download::DownloadManagerState::new(plan, config.clone()) else {
        return;
    };
    let ids = jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
    let hosts = jobs
        .iter()
        .map(|job| (job.id.clone(), job.host.clone()))
        .collect::<BTreeMap<_, _>>();

    assert_events_match_terminal_invariants(&mut manager, &ids);
    for _ in 0..reader.bounded_usize(MAX_STEPS + 1) {
        match reader.bounded_usize(13) {
            0 => {
                let actions = manager.schedule_ready_jobs();
                assert_start_actions(&manager, &actions);
            }
            1 => apply_command(
                &mut manager,
                DownloadCommand::PauseJob(select_id(&mut reader, &ids)),
            ),
            2 => apply_command(
                &mut manager,
                DownloadCommand::ResumeJob(select_id(&mut reader, &ids)),
            ),
            3 => apply_command(
                &mut manager,
                DownloadCommand::CancelJob(select_id(&mut reader, &ids)),
            ),
            4 => apply_command(
                &mut manager,
                DownloadCommand::RetryJob(select_id(&mut reader, &ids)),
            ),
            5 => apply_command(&mut manager, DownloadCommand::PauseAll),
            6 => apply_command(&mut manager, DownloadCommand::ResumeAll),
            7 => apply_command(&mut manager, DownloadCommand::CancelAll),
            8 => apply_report(
                &mut manager,
                WorkerReport::Progress {
                    id: select_id(&mut reader, &ids),
                    downloaded: reader.next_u64(),
                    total: reader.bool().then(|| reader.next_u64()),
                },
            ),
            9 => apply_report(
                &mut manager,
                WorkerReport::Completed {
                    id: select_id(&mut reader, &ids),
                },
            ),
            10 => apply_report(
                &mut manager,
                WorkerReport::Failed {
                    id: select_id(&mut reader, &ids),
                    error: short_string(&mut reader, "error", 24),
                    retryable: reader.bool(),
                },
            ),
            11 => apply_report(
                &mut manager,
                WorkerReport::Stopped {
                    id: select_id(&mut reader, &ids),
                    reason: WorkerStopReason::Paused,
                },
            ),
            12 => apply_report(
                &mut manager,
                WorkerReport::Stopped {
                    id: select_id(&mut reader, &ids),
                    reason: WorkerStopReason::Cancelled,
                },
            ),
            _ => unreachable!(),
        }
        assert_capacity_invariants(&manager, &ids, &hosts, &config);
        assert_events_match_terminal_invariants(&mut manager, &ids);
    }
});

fn exercise_plan_validation(reader: &mut FuzzReader<'_>, jobs: &[DownloadJobSpec]) {
    if jobs.is_empty() || !reader.bool() {
        return;
    }

    let mut duplicated = jobs.to_vec();
    duplicated.push(jobs[reader.bounded_usize(jobs.len())].clone());
    assert!(matches!(
        DownloadPlan::new(duplicated),
        Err(mado_download::DownloadPlanError::DuplicateJobId { .. })
    ));
}

fn build_jobs(reader: &mut FuzzReader<'_>) -> Vec<DownloadJobSpec> {
    let count = 1 + reader.bounded_usize(MAX_JOBS);
    (0..count).map(|index| build_job(reader, index)).collect()
}

fn build_job(reader: &mut FuzzReader<'_>, index: usize) -> DownloadJobSpec {
    let id = DownloadJobId::new(format!("job-{index}")).expect("generated job ids are non-empty");
    let url = DownloadUrl::new(format!("https://example.test/artifacts/{index}"))
        .expect("generated urls are non-empty");
    let host = HOSTS[reader.bounded_usize(HOSTS.len())].map(str::to_string);
    let checksum = reader.bool().then(|| Checksum {
        algorithm: if reader.bool() {
            ChecksumAlgorithm::Sha1
        } else {
            ChecksumAlgorithm::Sha256
        },
        value: short_string(reader, "checksum", 40),
    });

    DownloadJobSpec {
        id,
        url,
        host,
        target_path: PathBuf::from(format!("target/fuzz-download/job-{index}.bin")),
        expected_size: reader.bool().then(|| reader.next_u64()),
        checksum,
        kind: match reader.bounded_usize(6) {
            0 => DownloadArtifactKind::VersionMetadata,
            1 => DownloadArtifactKind::ClientJar,
            2 => DownloadArtifactKind::Library,
            3 => DownloadArtifactKind::Asset,
            4 => DownloadArtifactKind::Native,
            5 => DownloadArtifactKind::JavaRuntime,
            _ => unreachable!(),
        },
        policy: DownloadJobPolicy {
            resumable: reader.bool(),
            retryable: reader.bool(),
        },
    }
}

fn build_valid_config(reader: &mut FuzzReader<'_>) -> Option<DownloadManagerConfig> {
    let global_limit = 1 + reader.bounded_usize(MAX_JOBS);
    let per_host_limit = 1 + reader.bounded_usize(global_limit);
    let config = DownloadManagerConfig {
        concurrency: DownloadConcurrencyConfig {
            global_limit,
            per_host_limit,
            queue_capacity: 1 + reader.bounded_usize(256),
        },
        retry: DownloadRetryConfig {
            max_attempts: 1 + reader.next_u8().wrapping_rem(8),
            ..DownloadRetryConfig::default()
        },
        events: DownloadEventConfig {
            event_buffer: 1 + reader.bounded_usize(256),
            ..DownloadEventConfig::default()
        },
        ..DownloadManagerConfig::default()
    };

    if reader.bool() {
        let mut invalid = config.clone();
        match reader.bounded_usize(4) {
            0 => invalid.concurrency.global_limit = 0,
            1 => invalid.concurrency.per_host_limit = invalid.concurrency.global_limit + 1,
            2 => invalid.concurrency.queue_capacity = 0,
            3 => invalid.retry.max_attempts = 0,
            _ => unreachable!(),
        }
        assert!(invalid.validate().is_err());
    }

    config.validate().ok().map(|()| config)
}

fn apply_command(manager: &mut mado_download::DownloadManagerState, command: DownloadCommand) {
    match manager.apply_command(command) {
        Ok(actions) => assert_stop_actions(manager, &actions),
        Err(error) => assert_unknown_job(error),
    }
}

fn apply_report(manager: &mut mado_download::DownloadManagerState, report: WorkerReport) {
    if let Err(error) = manager.apply_worker_report(report) {
        assert_unknown_job(error);
    }
}

fn assert_unknown_job(error: DownloadManagerError) {
    assert!(matches!(error, DownloadManagerError::UnknownJob { .. }));
}

fn assert_start_actions(
    manager: &mado_download::DownloadManagerState,
    actions: &[DownloadManagerAction],
) {
    for action in actions {
        match action {
            DownloadManagerAction::StartJob(spec) => {
                assert!(matches!(
                    manager.state(&spec.id),
                    Some(DownloadJobState::Running { .. })
                ));
            }
            DownloadManagerAction::StopWorker { .. } => {
                panic!("scheduling must not emit stop actions");
            }
        }
    }
}

fn assert_stop_actions(
    manager: &mado_download::DownloadManagerState,
    actions: &[DownloadManagerAction],
) {
    for action in actions {
        match action {
            DownloadManagerAction::StartJob(_) => {
                panic!("commands must not emit start actions");
            }
            DownloadManagerAction::StopWorker { id, .. } => {
                assert!(is_active_state(manager.state(id)));
            }
        }
    }
}

fn assert_capacity_invariants(
    manager: &mado_download::DownloadManagerState,
    ids: &[DownloadJobId],
    hosts: &BTreeMap<DownloadJobId, Option<String>>,
    config: &DownloadManagerConfig,
) {
    let active_ids = ids
        .iter()
        .filter(|id| is_active_state(manager.state(id)))
        .collect::<Vec<_>>();
    assert!(active_ids.len() <= config.concurrency.global_limit);

    let mut active_by_host = BTreeMap::<&str, usize>::new();
    for id in active_ids {
        if let Some(Some(host)) = hosts.get(id) {
            *active_by_host.entry(host.as_str()).or_default() += 1;
        }
    }
    for active in active_by_host.values() {
        assert!(*active <= config.concurrency.per_host_limit);
    }
}

fn assert_events_match_terminal_invariants(
    manager: &mut mado_download::DownloadManagerState,
    ids: &[DownloadJobId],
) {
    let events = manager.drain_events();
    let terminal_events = events
        .iter()
        .filter(|event| matches!(event, DownloadEvent::PlanCompleted | DownloadEvent::PlanFailed))
        .collect::<Vec<_>>();
    assert!(terminal_events.len() <= 1);

    for event in terminal_events {
        match event {
            DownloadEvent::PlanCompleted => {
                assert_eq!(manager.terminal_state(), Some(PlanTerminalState::Completed));
                assert!(ids
                    .iter()
                    .all(|id| matches!(manager.state(id), Some(DownloadJobState::Completed))));
            }
            DownloadEvent::PlanFailed => {
                assert_eq!(manager.terminal_state(), Some(PlanTerminalState::Failed));
                assert!(ids.iter().any(|id| matches!(
                    manager.state(id),
                    Some(DownloadJobState::Failed { .. } | DownloadJobState::Cancelled)
                )));
            }
            _ => unreachable!(),
        }
    }
}

fn is_active_state(state: Option<&DownloadJobState>) -> bool {
    matches!(
        state,
        Some(
            DownloadJobState::Running { .. }
                | DownloadJobState::Pausing { .. }
                | DownloadJobState::Cancelling { .. }
        )
    )
}

fn select_id(reader: &mut FuzzReader<'_>, ids: &[DownloadJobId]) -> DownloadJobId {
    if reader.bool() {
        return DownloadJobId::new(format!("unknown-{}", reader.next_u8()))
            .expect("generated unknown ids are non-empty");
    }
    ids[reader.bounded_usize(ids.len())].clone()
}

fn short_string(reader: &mut FuzzReader<'_>, prefix: &str, max_extra: usize) -> String {
    let len = reader.bounded_usize(max_extra + 1);
    let mut value = String::from(prefix);
    for _ in 0..len {
        let byte = b'a' + reader.next_u8().wrapping_rem(26);
        value.push(char::from(byte));
    }
    value
}

#[derive(Debug, Clone, Copy)]
struct FuzzReader<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> FuzzReader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, cursor: 0 }
    }

    fn next_u8(&mut self) -> u8 {
        let byte = self.input.get(self.cursor).copied().unwrap_or(0);
        self.cursor = self.cursor.saturating_add(1);
        byte
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0_u8; 8];
        for byte in &mut bytes {
            *byte = self.next_u8();
        }
        u64::from_le_bytes(bytes)
    }

    fn bool(&mut self) -> bool {
        self.next_u8() % 2 == 1
    }

    fn bounded_usize(&mut self, upper_exclusive: usize) -> usize {
        debug_assert!(upper_exclusive > 0);
        usize::from(self.next_u8()) % upper_exclusive
    }
}
