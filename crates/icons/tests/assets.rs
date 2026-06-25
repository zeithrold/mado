use anyhow::{Context as _, Result};
use gpui::AssetSource;
use mado_icons::{LucideAssets, LucideIcon};

#[test]
fn generated_icon_paths_are_stable() {
    assert_eq!(LucideIcon::Search.path().as_ref(), "icons/search.svg");
}

#[test]
fn generated_assets_can_be_listed_and_loaded() -> Result<()> {
    let assets = LucideAssets::new();
    let icons = assets.list("icons")?;

    assert!(icons.iter().any(|path| path.as_ref() == "search.svg"));

    let search = assets
        .load("icons/search.svg")?
        .context("search icon should be embedded")?;
    let svg = std::str::from_utf8(&search).context("search icon should be utf-8")?;

    assert!(svg.contains("<svg"));
    Ok(())
}

#[test]
fn unknown_assets_are_absent() -> Result<()> {
    assert!(
        LucideAssets::new()
            .load("icons/definitely-not-real.svg")?
            .is_none()
    );
    Ok(())
}
