use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use mado_download::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadEvent, DownloadEventStream,
    DownloadJobId, DownloadJobPolicy, DownloadJobSpec, DownloadManagerConfig, DownloadPlan,
    DownloadResumeConfig, DownloadResumeMode, DownloadServiceLoop, DownloadStorageConfig,
    DownloadStoragePaths, DownloadUrl, NativeHttpBackend, NativeHttpBackendConfig,
    PartialDownloadMetadata, ResumeValidatorPolicy,
};
use sha1::{Digest, Sha1};
use tokio::runtime::{Builder, Runtime};

#[test]
fn native_http_backend_downloads_and_promotes_partial() -> Result<(), Box<dyn std::error::Error>> {
    let body = b"hello native http";
    let fixture = HttpFixture::serve_once(HttpResponse {
        status: "200 OK",
        headers: vec![("ETag", "\"fresh\"")],
        body: body.to_vec(),
    })?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("libraries/example.jar");
    let job = download_job("library", fixture.url.clone(), target_path.clone(), body)?;
    let plan = DownloadPlan::new(vec![job])?;
    let manager_config = DownloadManagerConfig::default();
    let backend_config = NativeHttpBackendConfig::default();
    let runtime = tokio_runtime()?;
    let runtime_handle = runtime.handle().clone();
    let (mut service_loop, _handle, events) =
        DownloadServiceLoop::try_with_backend_factory(plan, manager_config, |handle| {
            Ok(NativeHttpBackend::new(
                runtime_handle,
                handle,
                backend_config,
            )?)
        })?;

    service_loop.start()?;
    let events = wait_for_plan_completed(&mut service_loop, &events)?;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadEvent::JobProgress { .. }))
    );
    assert_eq!(std::fs::read(&target_path)?, body);
    assert!(
        !DownloadStoragePaths::for_target(&target_path, &DownloadStorageConfig::default())
            .partial_metadata_path
            .exists()
    );
    assert!(
        fixture
            .received_request()?
            .starts_with("GET /artifact HTTP/1.1")
    );
    Ok(())
}

#[test]
fn native_http_backend_completes_existing_ready_target_without_http()
-> Result<(), Box<dyn std::error::Error>> {
    let body = b"already here";
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("client.jar");
    std::fs::write(&target_path, body)?;
    let fixture = HttpFixture::serve_none()?;
    let job = download_job("client", fixture.url.clone(), target_path, body)?;
    let plan = DownloadPlan::new(vec![job])?;
    let runtime = tokio_runtime()?;
    let runtime_handle = runtime.handle().clone();
    let (mut service_loop, _handle, events) = DownloadServiceLoop::try_with_backend_factory(
        plan,
        DownloadManagerConfig::default(),
        |handle| {
            Ok(NativeHttpBackend::new(
                runtime_handle,
                handle,
                NativeHttpBackendConfig::default(),
            )?)
        },
    )?;

    service_loop.start()?;
    let events = wait_for_plan_completed(&mut service_loop, &events)?;

    assert!(events.contains(&DownloadEvent::PlanCompleted));
    assert!(fixture.no_request_received(Duration::from_millis(50))?);
    Ok(())
}

#[test]
fn native_http_backend_resumes_matching_partial_download() -> Result<(), Box<dyn std::error::Error>>
{
    let complete = b"resume me please";
    let prefix = b"resume ";
    let suffix = b"me please";
    let fixture = HttpFixture::serve_once(HttpResponse {
        status: "206 Partial Content",
        headers: vec![("ETag", "\"resume\""), ("Content-Range", "bytes 7-15/16")],
        body: suffix.to_vec(),
    })?;
    let temp_dir = tempfile::tempdir()?;
    let target_path = temp_dir.path().join("assets/object");
    let mut job = download_job("asset", fixture.url.clone(), target_path.clone(), complete)?;
    job.expected_size = Some(complete.len() as u64);
    let storage_config = DownloadStorageConfig::default();
    let paths = DownloadStoragePaths::for_target(&target_path, &storage_config);
    paths.write_partial_bytes(prefix, &storage_config)?;
    paths.write_partial_metadata(
        &PartialDownloadMetadata::for_job(
            &job,
            prefix.len() as u64,
            Some(mado_download::ResumeValidator {
                etag: Some("\"resume\"".to_string()),
                last_modified: None,
            }),
        ),
        &storage_config,
    )?;
    let plan = DownloadPlan::new(vec![job])?;
    let backend_config = NativeHttpBackendConfig {
        resume: DownloadResumeConfig {
            mode: DownloadResumeMode::Enabled,
            min_size: 1,
            validator_policy: ResumeValidatorPolicy::RequireMatch,
            ..DownloadResumeConfig::default()
        },
        ..NativeHttpBackendConfig::default()
    };
    let runtime = tokio_runtime()?;
    let runtime_handle = runtime.handle().clone();
    let (mut service_loop, _handle, events) = DownloadServiceLoop::try_with_backend_factory(
        plan,
        DownloadManagerConfig::default(),
        |handle| {
            Ok(NativeHttpBackend::new(
                runtime_handle,
                handle,
                backend_config,
            )?)
        },
    )?;

    service_loop.start()?;
    let _events = wait_for_plan_completed(&mut service_loop, &events)?;
    let request = fixture.received_request()?;

    assert!(request.contains("range: bytes=7-") || request.contains("Range: bytes=7-"));
    assert_eq!(std::fs::read(&target_path)?, complete);
    Ok(())
}

fn tokio_runtime() -> Result<Runtime, Box<dyn std::error::Error>> {
    Ok(Builder::new_multi_thread()
        .enable_all()
        .thread_name("mado-download-http-test")
        .build()?)
}

fn wait_for_plan_completed<B: mado_download::DownloadBackend>(
    service_loop: &mut DownloadServiceLoop<B>,
    events: &DownloadEventStream,
) -> Result<Vec<DownloadEvent>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut collected = Vec::new();
    while Instant::now() < deadline {
        service_loop.run_until_idle()?;
        collected.extend(events.drain_available());
        if collected.contains(&DownloadEvent::PlanCompleted) {
            return Ok(collected);
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err(format!("timed out waiting for PlanCompleted; events: {collected:?}").into())
}

fn download_job(
    id: &str,
    url: String,
    target_path: PathBuf,
    body: &[u8],
) -> Result<DownloadJobSpec, Box<dyn std::error::Error>> {
    Ok(DownloadJobSpec {
        id: DownloadJobId::new(id)?,
        url: DownloadUrl::new(url)?,
        host: Some("127.0.0.1".to_string()),
        target_path,
        expected_size: Some(body.len() as u64),
        checksum: Some(Checksum {
            algorithm: ChecksumAlgorithm::Sha1,
            value: sha1_hex(body),
        }),
        kind: DownloadArtifactKind::Library,
        policy: DownloadJobPolicy::default(),
    })
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[derive(Debug)]
struct HttpResponse {
    status: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct HttpFixture {
    url: String,
    request_receiver: mpsc::Receiver<String>,
}

impl HttpFixture {
    fn serve_once(response: HttpResponse) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (request_sender, request_receiver) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _peer)) = listener.accept() else {
                return;
            };
            let request = read_http_request(&mut stream);
            let _ = request_sender.send(request);
            let mut headers = format!(
                "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                response.status,
                response.body.len()
            );
            for (name, value) in response.headers {
                headers.push_str(name);
                headers.push_str(": ");
                headers.push_str(value);
                headers.push_str("\r\n");
            }
            headers.push_str("\r\n");
            let _ = stream.write_all(headers.as_bytes());
            let _ = stream.write_all(&response.body);
        });
        Ok(Self {
            url: format!("http://{address}/artifact"),
            request_receiver,
        })
    }

    fn serve_none() -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (request_sender, request_receiver) = mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut stream, _peer)) = listener.accept() {
                let request = read_http_request(&mut stream);
                let _ = request_sender.send(request);
            }
        });
        Ok(Self {
            url: format!("http://{address}/artifact"),
            request_receiver,
        })
    }

    fn received_request(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.request_receiver.recv_timeout(Duration::from_secs(2))?)
    }

    fn no_request_received(&self, timeout: Duration) -> Result<bool, Box<dyn std::error::Error>> {
        match self.request_receiver.recv_timeout(timeout) {
            Ok(_) => Ok(false),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }
}

fn read_http_request(stream: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 512];
    while let Ok(read) = stream.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..read) else {
            break;
        };
        bytes.extend_from_slice(chunk);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
