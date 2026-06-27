use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use mado_download::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadConcurrencyConfig, DownloadEvent,
    DownloadEventStream, DownloadJobId, DownloadJobPolicy, DownloadJobSpec, DownloadManagerConfig,
    DownloadPlan, DownloadServiceLoop, DownloadUrl, NativeHttpBackend, NativeHttpBackendConfig,
};
use serde::Deserialize;
use tokio::runtime::{Builder, Runtime};

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FIXED_VERSION_ID: &str = "1.20.1";
const REAL_INTEGRATION_ENV: &str = "MADO_RUN_REAL_INTEGRATION";
const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";

#[test]
#[ignore = "requires real Mojang/Piston and Minecraft asset CDN network access"]
fn downloads_ten_real_minecraft_assets_concurrently() -> Result<(), Box<dyn std::error::Error>> {
    run_real_network_case(10)
}

#[test]
#[ignore = "requires real Mojang/Piston and Minecraft asset CDN network access"]
fn downloads_one_hundred_real_minecraft_assets_concurrently()
-> Result<(), Box<dyn std::error::Error>> {
    run_real_network_case(100)
}

fn run_real_network_case(job_count: usize) -> Result<(), Box<dyn std::error::Error>> {
    if !real_network_enabled() {
        write_integration_skip_message();
        return Ok(());
    }

    let runtime = tokio_runtime()?;
    let assets = runtime.block_on(load_small_assets(job_count))?;
    let temp_dir = tempfile::tempdir()?;
    let jobs = assets
        .iter()
        .enumerate()
        .map(|(index, asset)| asset_download_job(index, asset, temp_dir.path().join(&asset.hash)))
        .collect::<Result<Vec<_>, _>>()?;
    let manager_config = DownloadManagerConfig {
        concurrency: DownloadConcurrencyConfig {
            global_limit: job_count,
            per_host_limit: job_count,
            queue_capacity: job_count.saturating_mul(2),
        },
        ..DownloadManagerConfig::default()
    };
    let runtime_handle = runtime.handle().clone();
    let (mut service_loop, _handle, events) = DownloadServiceLoop::try_with_backend_factory(
        DownloadPlan::new(jobs)?,
        manager_config,
        |handle| {
            Ok(NativeHttpBackend::new(
                runtime_handle,
                handle,
                NativeHttpBackendConfig::default(),
            )?)
        },
    )?;

    service_loop.start()?;
    let events = wait_for_plan_completed(&mut service_loop, &events, Duration::from_mins(2))?;

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, DownloadEvent::JobCompleted { .. }))
            .count(),
        job_count
    );
    for asset in assets {
        let target_path = temp_dir.path().join(&asset.hash);
        assert_eq!(std::fs::metadata(&target_path)?.len(), asset.size);
        assert!(!target_path.with_extension("part.json").exists());
    }
    Ok(())
}

async fn load_small_assets(
    count: usize,
) -> Result<Vec<MinecraftAsset>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let manifest: VersionManifest = fetch_json(&client, VERSION_MANIFEST_URL).await?;
    let version = manifest
        .versions
        .iter()
        .find(|version| version.id == FIXED_VERSION_ID)
        .ok_or_else(|| format!("version {FIXED_VERSION_ID} was not present in manifest"))?;
    let metadata: VersionMetadata = fetch_json(&client, &version.url).await?;
    let asset_index: AssetIndex = fetch_json(&client, &metadata.asset_index.url).await?;
    let mut unique_assets = BTreeMap::new();
    for asset in asset_index.objects.into_values() {
        unique_assets.entry(asset.hash.clone()).or_insert(asset);
    }
    let mut assets = unique_assets.into_values().collect::<Vec<_>>();
    assets.retain(|asset| asset.size > 0 && asset.size <= 16 * 1024);
    assets.sort_by(|left, right| {
        left.size
            .cmp(&right.size)
            .then_with(|| left.hash.cmp(&right.hash))
    });
    if assets.len() < count {
        return Err(format!(
            "asset index only had {} small assets, needed {count}",
            assets.len()
        )
        .into());
    }
    assets.truncate(count);
    Ok(assets)
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let response = client.get(url).send().await?.error_for_status()?;
    let body = response.text().await?;
    Ok(serde_json::from_str(&body)?)
}

fn asset_download_job(
    index: usize,
    asset: &MinecraftAsset,
    target_path: PathBuf,
) -> Result<DownloadJobSpec, Box<dyn std::error::Error>> {
    let prefix = asset
        .hash
        .get(..2)
        .ok_or_else(|| format!("asset hash is too short: {}", asset.hash))?;
    Ok(DownloadJobSpec {
        id: DownloadJobId::new(format!("asset-{index}"))?,
        url: DownloadUrl::new(format!("{ASSET_BASE_URL}/{prefix}/{}", asset.hash))?,
        host: Some("resources.download.minecraft.net".to_string()),
        target_path,
        expected_size: Some(asset.size),
        checksum: Some(Checksum {
            algorithm: ChecksumAlgorithm::Sha1,
            value: asset.hash.clone(),
        }),
        kind: DownloadArtifactKind::Asset,
        policy: DownloadJobPolicy::default(),
    })
}

type NativeServiceLoop = DownloadServiceLoop<NativeHttpBackend>;

fn tokio_runtime() -> Result<Runtime, Box<dyn std::error::Error>> {
    Ok(Builder::new_multi_thread()
        .enable_all()
        .thread_name("mado-download-real-network-test")
        .build()?)
}

fn wait_for_plan_completed(
    service_loop: &mut NativeServiceLoop,
    events: &DownloadEventStream,
    timeout: Duration,
) -> Result<Vec<DownloadEvent>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    let mut collected = Vec::new();
    while Instant::now() < deadline {
        service_loop.run_until_idle()?;
        let new_events = events.drain_available();
        let failure = new_events
            .iter()
            .find(|event| matches!(event, DownloadEvent::PlanFailed))
            .cloned();
        if let Some(failure) = failure {
            collected.extend(new_events);
            return Err(format!(
                "real network download plan failed: {failure:?}; events: {collected:?}"
            )
            .into());
        }
        let completed = new_events
            .iter()
            .any(|event| matches!(event, DownloadEvent::PlanCompleted));
        collected.extend(new_events);
        if completed {
            return Ok(collected);
        }
        thread::park_timeout(Duration::from_millis(10));
    }
    Err(format!("timed out waiting for real network PlanCompleted; events: {collected:?}").into())
}

fn real_network_enabled() -> bool {
    std::env::var(REAL_INTEGRATION_ENV).is_ok_and(|value| value == "1")
}

#[expect(
    clippy::print_stderr,
    reason = "ignored real-network tests should explain their gate"
)]
fn write_integration_skip_message() {
    eprintln!(
        "skipping real Minecraft asset downloads; set {REAL_INTEGRATION_ENV}=1 and run ignored tests"
    );
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionSummary>,
}

#[derive(Debug, Deserialize)]
struct VersionSummary {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct VersionMetadata {
    #[serde(rename = "assetIndex")]
    asset_index: AssetIndexReference,
}

#[derive(Debug, Deserialize)]
struct AssetIndexReference {
    url: String,
}

#[derive(Debug, Deserialize)]
struct AssetIndex {
    objects: BTreeMap<String, MinecraftAsset>,
}

#[derive(Debug, Deserialize)]
struct MinecraftAsset {
    hash: String,
    size: u64,
}
