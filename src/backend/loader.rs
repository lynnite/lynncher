
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::http::http_client_with_proxy;
use super::{display_path, LauncherPaths};

const LAUNCHER_RELEASES_URL: &str =
    "https://api.github.com/repos/space-wizards/SS14.Launcher/releases/latest";

pub struct LoaderInstall {
    pub loader_dir: PathBuf,
    pub loader_exe: PathBuf,
    pub signing_key: PathBuf,
}

pub fn ensure_loader_installed(paths: &LauncherPaths, proxy_url: Option<&str>) -> Result<LoaderInstall> {
    let install_dir = paths.clients_dir.join("loader");
    let loader_dir = install_dir.join("bin");
    let signing_key = install_dir.join("signing_key");

    let loader_exe = if cfg!(target_os = "windows") {
        loader_dir.join("SS14.Loader.exe")
    } else {
        loader_dir.join("SS14.Loader")
    };

    if loader_exe.exists() && signing_key.exists() {
        return Ok(LoaderInstall {
            loader_dir: loader_dir.clone(),
            loader_exe,
            signing_key,
        });
    }

    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    let client = http_client_with_proxy(proxy_url)?;
    let release = fetch_latest_release(&client)?;

    let asset_name = release_asset_name();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| format!("SS14.Launcher release has no {asset_name} asset"))?;
    let asset_url = asset.browser_download_url.clone();

    let temp_zip = install_dir.join("launcher-release.zip");
    let mut response = client
        .get(&asset_url)
        .header(reqwest::header::USER_AGENT, "ss14-launcher-rust")
        .send()
        .with_context(|| format!("requesting launcher release from {asset_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "launcher release download responded with {} ({asset_url})",
            response.status()
        );
    }

    {
        let mut out = fs::File::create(&temp_zip)
            .with_context(|| format!("creating {}", display_path(&temp_zip)))?;
        io::copy(&mut response, &mut out)
            .context("writing launcher release zip to disk")?;
    }

    if loader_dir.exists() {
        fs::remove_dir_all(&loader_dir)
            .with_context(|| format!("clearing {}", display_path(&loader_dir)))?;
    }
    fs::create_dir_all(&loader_dir)
        .with_context(|| format!("creating {}", display_path(&loader_dir)))?;

    extract_loader_parts(&temp_zip, &loader_dir, &signing_key)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&loader_exe) {
            let mode = meta.permissions().mode();
            if mode & 0o100 == 0 {
                let mut perms = meta.permissions();
                perms.set_mode(mode | 0o755);
                let _ = fs::set_permissions(&loader_exe, perms);
            }
        }
    }

    let _ = fs::remove_file(&temp_zip);

    if !loader_exe.exists() || !signing_key.exists() {
        anyhow::bail!(
            "extracting loader produced no {} (loader={}, key={})",
            asset_name,
            loader_exe.exists(),
            signing_key.exists()
        );
    }

    Ok(LoaderInstall {
        loader_dir,
        loader_exe,
        signing_key,
    })
}

fn extract_loader_parts(zip_path: &Path, loader_dir: &Path, signing_key: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("opening {}", display_path(zip_path)))?;
    let mut archive = zip::ZipArchive::new(file).context("opening launcher release zip")?;

    let mut found_key = false;
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).context("reading release zip entry")?;
        let name_str = entry.name().replace('\\', "/");

        if let Some(rest) = name_str
            .strip_prefix("bin_x64/loader/")
            .or_else(|| name_str.strip_prefix("bin_arm64/loader/"))
        {
            if rest.is_empty() {
                continue;
            }
            let target = loader_dir.join(rest);
            if entry.is_dir() {
                fs::create_dir_all(&target)
                    .with_context(|| format!("creating {}", display_path(&target)))?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", display_path(parent)))?;
                }
                let mut out = fs::File::create(&target)
                    .with_context(|| format!("creating {}", display_path(&target)))?;
                io::copy(&mut entry, &mut out)
                    .with_context(|| format!("extracting {}", display_path(&target)))?;
            }
            continue;
        }

        if name_str == "bin_x64/signing_key" || name_str == "bin_arm64/signing_key" {
            let mut out = fs::File::create(signing_key)
                .with_context(|| format!("creating {}", display_path(signing_key)))?;
            io::copy(&mut entry, &mut out)
                .with_context(|| format!("extracting {}", display_path(signing_key)))?;
            found_key = true;
        }
    }

    if !found_key {
        anyhow::bail!("launcher release zip did not contain a signing_key");
    }

    Ok(())
}

fn release_asset_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "SS14.Launcher_Windows.zip"
    }
    #[cfg(target_os = "macos")]
    {
        "SS14.Launcher_macOS.zip"
    }
    #[cfg(target_os = "linux")]
    {
        "SS14.Launcher_Linux.zip"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "SS14.Launcher_Linux.zip"
    }
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    #[serde(rename = "browser_download_url")]
    browser_download_url: String,
}

fn fetch_latest_release(client: &reqwest::blocking::Client) -> Result<GithubRelease> {
    let response = client
        .get(LAUNCHER_RELEASES_URL)
        .header(reqwest::header::USER_AGENT, "ss14-launcher-rust")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .with_context(|| format!("requesting latest release from {LAUNCHER_RELEASES_URL}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub releases API responded with {} ({LAUNCHER_RELEASES_URL})",
            response.status()
        );
    }

    response
        .json::<GithubRelease>()
        .context("parsing GitHub release JSON")
}
