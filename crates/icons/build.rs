use std::fmt::Write as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use flate2::read::GzDecoder;
use serde_json::Value;
use sha1::{Digest, Sha1};

#[path = "src/icon_name.rs"]
mod icon_name;

use icon_name::unique_variant_name;

const LUCIDE_STATIC_VERSION: &str = "1.21.0";
const LUCIDE_STATIC_SHASUM: &str = "08812bde7238e206466ba07226e2316f2ab599fe";

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LUCIDE_STATIC_CACHE_DIR");
    println!("cargo:rerun-if-env-changed=LUCIDE_STATIC_REGISTRY");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let cache_dir = std::env::var_os("LUCIDE_STATIC_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or(workspace_root()?.join("target").join("lucide-static-cache"));
    let package_dir = ensure_lucide_static(&cache_dir)?;
    let icons = collect_icons(&package_dir)?;

    if icons.is_empty() {
        bail!("lucide-static package did not contain any SVG icons");
    }

    fs::write(out_dir.join("lucide_icons.rs"), generate_source(&icons))
        .context("writing generated lucide icon source")?;

    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            anyhow!(
                "could not resolve workspace root from {}",
                manifest_dir.display()
            )
        })
}

fn ensure_lucide_static(cache_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir).context("creating lucide-static cache directory")?;

    let version_dir = cache_dir.join(LUCIDE_STATIC_VERSION);
    let tarball_path = version_dir.join("lucide-static.tgz");
    let extract_dir = version_dir.join("package");
    fs::create_dir_all(&version_dir).context("creating lucide-static version cache")?;

    let cached_tarball_is_valid =
        tarball_path.exists() && sha1_hex(&fs::read(&tarball_path)?) == LUCIDE_STATIC_SHASUM;

    if !cached_tarball_is_valid {
        download_lucide_tarball(&tarball_path)?;
    }

    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir).context("clearing lucide-static extract cache")?;
    }
    fs::create_dir_all(&extract_dir).context("creating lucide-static extract cache")?;

    let tarball = fs::File::open(&tarball_path).context("opening lucide-static tarball")?;
    let decoder = GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(&extract_dir)
        .context("extracting lucide-static tarball")?;

    Ok(extract_dir)
}

fn download_lucide_tarball(tarball_path: &Path) -> Result<()> {
    let registry = std::env::var("LUCIDE_STATIC_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
    let metadata_url = format!(
        "{}/lucide-static/{}",
        registry.trim_end_matches('/'),
        LUCIDE_STATIC_VERSION
    );
    let metadata = ureq::get(&metadata_url)
        .call()
        .with_context(|| format!("fetching {metadata_url}"))?
        .into_string()
        .context("reading lucide-static metadata")?;
    let metadata: Value =
        serde_json::from_str(&metadata).context("parsing lucide-static metadata")?;
    let dist = metadata
        .get("dist")
        .ok_or_else(|| anyhow!("lucide-static metadata missing dist"))?;
    let shasum = dist
        .get("shasum")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("lucide-static metadata missing dist.shasum"))?;
    let tarball_url = dist
        .get("tarball")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("lucide-static metadata missing dist.tarball"))?;

    if shasum != LUCIDE_STATIC_SHASUM {
        bail!(
            "lucide-static registry SHA1 changed for version {LUCIDE_STATIC_VERSION}: expected {LUCIDE_STATIC_SHASUM}, got {shasum}"
        );
    }

    let mut response = ureq::get(tarball_url)
        .call()
        .with_context(|| format!("downloading {tarball_url}"))?
        .into_reader();
    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .context("reading lucide-static tarball")?;
    let actual = sha1_hex(&bytes);
    if actual != shasum {
        bail!("lucide-static tarball SHA1 mismatch: expected {shasum}, got {actual}");
    }
    fs::write(tarball_path, bytes).context("writing lucide-static tarball cache")?;

    Ok(())
}

fn collect_icons(package_dir: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut icons = BTreeMap::new();
    collect_icons_recursive(package_dir, &mut icons)?;
    Ok(icons)
}

fn collect_icons_recursive(dir: &Path, icons: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_icons_recursive(&path, icons)?;
            continue;
        }

        let is_svg = path.extension().and_then(|ext| ext.to_str()) == Some("svg");
        let in_icons_dir = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("icons");

        if is_svg && in_icons_dir {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow!("invalid icon filename: {}", path.display()))?
                .to_string();
            icons.insert(file_name, path);
        }
    }
    Ok(())
}

fn generate_source(icons: &BTreeMap<String, PathBuf>) -> String {
    let mut used_variants = BTreeSet::new();
    let mut enum_variants = String::new();
    let mut path_arms = String::new();
    let mut asset_entries = String::new();
    let mut list_entries = String::new();

    for (file_name, path) in icons {
        let stem = file_name.trim_end_matches(".svg");
        let variant = unique_variant_name(stem, &mut used_variants);
        let icon_path = format!("icons/{file_name}");
        let bytes_path = path.display();

        let _ = writeln!(enum_variants, "    {variant},");
        let _ = writeln!(path_arms, "            Self::{variant} => \"{icon_path}\",");
        let _ = writeln!(
            asset_entries,
            "    (\"{icon_path}\", include_bytes!(r#\"{bytes_path}\"#)),\n"
        );
        let _ = writeln!(list_entries, "    \"{file_name}\",");
    }

    format!(
        r#"use std::borrow::Cow;

use anyhow::Result;
use gpui::{{AssetSource, SharedString}};
use gpui_component::IconNamed;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LucideIcon {{
{enum_variants}}}

impl LucideIcon {{
    pub fn path(self) -> SharedString {{
        <Self as IconNamed>::path(self)
    }}
}}

impl IconNamed for LucideIcon {{
    #[expect(clippy::too_many_lines, reason = "generated match includes one arm per icon")]
    fn path(self) -> SharedString {{
        match self {{
{path_arms}        }}
        .into()
    }}
}}

#[derive(Clone, Copy, Debug, Default)]
pub struct LucideAssets;

impl LucideAssets {{
    pub const fn new() -> Self {{
        Self
    }}
}}

impl AssetSource for LucideAssets {{
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {{
        Ok(LUCIDE_ASSETS
            .iter()
            .find(|(asset_path, _)| *asset_path == path)
            .map(|(_, bytes)| Cow::Borrowed(&bytes[..])))
    }}

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {{
        if path == "icons" || path == "icons/" {{
            Ok(LUCIDE_ICON_FILES.iter().map(|name| (*name).into()).collect())
        }} else {{
            Ok(Vec::new())
        }}
    }}
}}

const LUCIDE_ASSETS: &[(&str, &[u8])] = &[
{asset_entries}];

const LUCIDE_ICON_FILES: &[&str] = &[
{list_entries}];

#[cfg(test)]
mod tests {{
    use super::*;
    use anyhow::Context as _;

    #[test]
    fn exposes_expected_icon_path() {{
        assert_eq!(LucideIcon::Search.path().as_ref(), "icons/search.svg");
    }}

    #[test]
    fn loads_embedded_asset() -> Result<()> {{
        let bytes = LucideAssets::new()
            .load("icons/search.svg")?
            .context("search icon should exist")?;
        let svg = std::str::from_utf8(&bytes).context("icon should be utf-8 SVG")?;
        assert!(svg.contains("<svg"));
        Ok(())
    }}

    #[test]
    fn missing_asset_returns_none() -> Result<()> {{
        assert!(LucideAssets::new().load("icons/not-real.svg")?.is_none());
        Ok(())
    }}
}}
"#
    )
}

fn sha1_hex(bytes: impl AsRef<[u8]>) -> String {
    hex_lower(Sha1::digest(bytes.as_ref()).as_slice())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(hex_char(byte >> 4));
        out.push(hex_char(byte & 0x0f));
    }
    out
}

fn hex_char(nibble: u8) -> char {
    char::from(if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + (nibble - 10)
    })
}
