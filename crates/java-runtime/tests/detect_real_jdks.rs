use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use mado_java_runtime::{JavaArchitectureKind, JavaRuntimeInfo, JavaVendorKind, detect_java_home};
use serde_json::Value;
use tar::Archive;

#[derive(Debug, Clone, Copy)]
enum FixtureVendor {
    Temurin,
    Zulu,
    Liberica,
}

#[derive(Debug, Clone, Copy)]
struct JdkFixture {
    version: u16,
    vendor: FixtureVendor,
}

#[test]
fn detects_representative_real_jdks() -> Result<(), Box<dyn std::error::Error>> {
    if !should_run_real_jdk_fixture_tests() {
        write_integration_skip_message()?;
        return Ok(());
    }

    let fixtures = [
        JdkFixture {
            version: 8,
            vendor: FixtureVendor::Temurin,
        },
        JdkFixture {
            version: 17,
            vendor: FixtureVendor::Zulu,
        },
        JdkFixture {
            version: 21,
            vendor: FixtureVendor::Liberica,
        },
        JdkFixture {
            version: 25,
            vendor: FixtureVendor::Temurin,
        },
    ];

    for fixture in fixtures {
        match detect_fixture(fixture)? {
            Some(info) => assert_fixture(fixture, &info),
            None => write_skip_message(fixture)?,
        }
    }

    Ok(())
}

fn detect_fixture(
    fixture: JdkFixture,
) -> Result<Option<JavaRuntimeInfo>, Box<dyn std::error::Error>> {
    let Some(url) = fixture.download_url()? else {
        return Ok(None);
    };

    let cache_dir = workspace_root()
        .join("target")
        .join("jdk-fixtures")
        .join(format!("{}-{}", fixture.vendor.label(), fixture.version));
    let marker = cache_dir.join(".complete");
    if !marker.exists() {
        if cache_dir.exists() {
            fs::remove_dir_all(&cache_dir)?;
        }
        fs::create_dir_all(&cache_dir)?;
        if !download_and_extract_tar_gz(&url, &cache_dir)? {
            fs::remove_dir_all(&cache_dir)?;
            return Ok(None);
        }
        fs::write(marker, url)?;
    }

    let Some(java_home) = find_java_home(&cache_dir) else {
        return Err(format!(
            "downloaded fixture does not contain a JDK home: {}",
            cache_dir.display()
        )
        .into());
    };

    Ok(Some(detect_java_home(java_home)?))
}

fn assert_fixture(fixture: JdkFixture, info: &JavaRuntimeInfo) {
    assert_eq!(info.version.major, fixture.version);
    assert_eq!(info.vendor.kind, fixture.vendor.expected_kind());
    assert_eq!(info.architecture.kind, host_architecture_kind());
}

impl JdkFixture {
    fn download_url(self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        match self.vendor {
            FixtureVendor::Temurin => Ok(temurin_url(self.version)),
            FixtureVendor::Zulu => zulu_url(self.version),
            FixtureVendor::Liberica => liberica_url(self.version),
        }
    }
}

impl FixtureVendor {
    const fn label(self) -> &'static str {
        match self {
            Self::Temurin => "temurin",
            Self::Zulu => "zulu",
            Self::Liberica => "liberica",
        }
    }

    const fn expected_kind(self) -> JavaVendorKind {
        match self {
            Self::Temurin => JavaVendorKind::Temurin,
            Self::Zulu => JavaVendorKind::Zulu,
            Self::Liberica => JavaVendorKind::Liberica,
        }
    }
}

fn temurin_url(version: u16) -> Option<String> {
    let os = adoptium_os()?;
    let arch = adoptium_arch()?;

    Some(format!(
        "https://api.adoptium.net/v3/binary/latest/{version}/ga/{os}/{arch}/jdk/hotspot/normal/eclipse?project=jdk"
    ))
}

fn zulu_url(version: u16) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some(os) = azul_os() else {
        return Ok(None);
    };
    let Some(arch) = azul_arch() else {
        return Ok(None);
    };
    let url = format!(
        "https://api.azul.com/metadata/v1/zulu/packages/?java_version={version}&os={os}&arch={arch}&archive_type=tar.gz&java_package_type=jdk&latest=true&release_status=ga"
    );
    let Some(json) = get_json(&url)? else {
        return Ok(None);
    };
    let Some(download_url) = json
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("download_url"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };

    Ok(Some(download_url.to_string()))
}

fn liberica_url(version: u16) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some(os) = bellsoft_os() else {
        return Ok(None);
    };
    let Some(arch) = bellsoft_arch() else {
        return Ok(None);
    };
    let url = format!(
        "https://api.bell-sw.com/v1/liberica/releases?version-feature={version}&os={os}&arch={arch}&package-type=jdk&bundle-type=jdk&bitness=64&release-type=ga&output=json"
    );
    let Some(json) = get_json(&url)? else {
        return Ok(None);
    };
    let Some(download_url) = json
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("downloadUrl").or_else(|| item.get("download_url")))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };

    Ok(Some(download_url.to_string()))
}

fn get_json(url: &str) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    let response = match ureq::get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut reader = response.into_reader();
    let mut body = String::new();
    std::io::Read::read_to_string(&mut reader, &mut body)?;
    Ok(Some(serde_json::from_str(&body)?))
}

fn download_and_extract_tar_gz(
    url: &str,
    destination: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let response = match ureq::get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let reader = response.into_reader();
    let decoder = GzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    archive.unpack(destination)?;
    Ok(true)
}

fn find_java_home(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if path.join("release").is_file() && path.join("bin").join(java_binary_name()).is_file() {
            return Some(path);
        }

        let entries = fs::read_dir(&path).ok()?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            }
        }
    }

    None
}

const fn java_binary_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn should_run_real_jdk_fixture_tests() -> bool {
    std::env::var_os("CI").is_some()
        || std::env::var("MADO_RUN_JDK_FIXTURES").is_ok_and(|value| value == "1")
}

fn write_integration_skip_message() -> Result<(), Box<dyn std::error::Error>> {
    writeln!(
        std::io::stderr(),
        "skipping real JDK fixture downloads outside CI; set MADO_RUN_JDK_FIXTURES=1 to run locally"
    )?;
    Ok(())
}

fn write_skip_message(fixture: JdkFixture) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(
        std::io::stderr(),
        "skipping JDK {} {} fixture for unsupported host {} {}",
        fixture.version,
        fixture.vendor.label(),
        std::env::consts::OS,
        std::env::consts::ARCH
    )?;
    Ok(())
}

fn host_architecture_kind() -> JavaArchitectureKind {
    match std::env::consts::ARCH {
        "x86_64" => JavaArchitectureKind::X86_64,
        "aarch64" => JavaArchitectureKind::Aarch64,
        "x86" => JavaArchitectureKind::X86,
        "arm" => JavaArchitectureKind::Arm,
        _ => JavaArchitectureKind::Unknown,
    }
}

fn adoptium_os() -> Option<&'static str> {
    match std::env::consts::OS {
        "macos" => Some("mac"),
        "linux" => Some("linux"),
        "windows" => Some("windows"),
        _ => None,
    }
}

fn adoptium_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x64"),
        "aarch64" => Some("aarch64"),
        "x86" => Some("x86"),
        _ => None,
    }
}

fn azul_os() -> Option<&'static str> {
    match std::env::consts::OS {
        "macos" => Some("macos"),
        "linux" => Some("linux"),
        "windows" => Some("windows"),
        _ => None,
    }
}

fn azul_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x64"),
        "aarch64" => Some("arm64"),
        "x86" => Some("x86"),
        _ => None,
    }
}

fn bellsoft_os() -> Option<&'static str> {
    match std::env::consts::OS {
        "macos" => Some("macos"),
        "linux" => Some("linux"),
        "windows" => Some("windows"),
        _ => None,
    }
}

fn bellsoft_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x86"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}
