use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

const RELEASES_URL: &str = "https://api.github.com/repos/lynnite/lynncher/releases/latest";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: Option<String>,
    name: Option<String>,
    html_url: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    #[serde(rename = "browser_download_url")]
    browser_download_url: String,
}

fn build_client(proxy_url: Option<&str>) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent("ss14-launcher-rust")
        .timeout(std::time::Duration::from_secs(120));

    if let Some(url) = proxy_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(trimmed) {
                builder = builder.proxy(proxy);
            }
        }
    }

    builder.build().context("building HTTP client")
}

pub fn release_asset_name() -> String {
    #[cfg(target_os = "windows")]
    {
        "lynncher-windows-x86_64.exe".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        if command_exists("dpkg") {
            "lynncher-linux-x86_64.deb".to_string()
        } else {
            "lynncher-linux-x86_64.rpm".to_string()
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        "lynncher-linux-x86_64.deb".to_string()
    }
}

fn command_exists(name: &str) -> bool {
    let probe = if cfg!(target_os = "windows") {
        format!("where {}", name)
    } else {
        format!("command -v {}", name)
    };
    std::process::Command::new("sh")
        .arg("-c")
        .arg(&probe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn fetch_latest_release(proxy_url: Option<&str>) -> Result<GithubRelease> {
    let client = build_client(proxy_url)?;
    let response = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .context("querying GitHub releases")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub releases API responded with {}", response.status());
    }

    response.json().context("parsing GitHub release")
}

/// Fetch the URL of the latest release page on GitHub.
pub fn latest_release_url(proxy_url: Option<&str>) -> Result<Option<String>> {
    let release = fetch_latest_release(proxy_url)?;
    Ok(release.html_url)
}

/// Query the latest release tag from the lynncher GitHub repo.
/// Returns `None` when no tag could be determined.
pub fn check_latest_release(proxy_url: Option<&str>) -> Result<Option<String>> {
    let release = fetch_latest_release(proxy_url)?;
    Ok(release.tag_name.or(release.name))
}

/// Compare a GitHub release tag against the built-in version.
/// Returns `true` if the tag represents a newer release.
pub fn is_newer_tag(tag: &str, current: &str) -> bool {
    let tag = tag.trim().trim_start_matches('v');
    let current = current.trim().trim_start_matches('v');

    let parse = |s: &str| -> Option<Vec<u64>> {
        s.split(['.', '-', '_'])
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
    };

    match (parse(tag), parse(current)) {
        (Some(a), Some(b)) => {
            for (x, y) in a.iter().zip(b.iter()) {
                if x != y {
                    return x > y;
                }
            }
            a.len() > b.len()
        }
        _ => !tag.is_empty() && tag != current,
    }
}

pub fn download_and_apply_update(proxy_url: Option<&str>) -> Result<()> {
    let asset_name = release_asset_name();

    let release = fetch_latest_release(proxy_url)?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| format!("latest release has no {asset_name} asset"))?;

    let client = build_client(proxy_url)?;
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .with_context(|| format!("downloading {asset_name}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "update download responded with {} for {asset_name}",
            response.status()
        );
    }

    let body = response.bytes().context("reading update body")?;

    let work_dir = std::env::temp_dir().join(format!("lynncher-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work_dir);
    fs::create_dir_all(&work_dir).context("creating update work directory")?;

    let staged = work_dir.join(&asset_name);
    fs::write(&staged, &body).with_context(|| format!("writing {}", staged.display()))?;

    let result = apply_downloaded_artifact(&staged);

    let _ = fs::remove_dir_all(&work_dir);

    result
}

fn apply_downloaded_artifact(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let current = std::env::current_exe().context("locating running executable")?;
        let staged = current.with_extension("exe.new");
        fs::copy(path, &staged).context("staging new executable")?;

        if fs::copy(&staged, &current).is_ok() {
            let _ = fs::remove_file(&staged);
            return Ok(());
        }

        anyhow::bail!(
            "download complete; replace {} with {} after closing the launcher",
            current.display(),
            staged.display()
        );
    }

    #[cfg(target_os = "linux")]
    {
        let artifact_path = path.to_path_buf();

        let is_deb = artifact_path
            .extension()
            .map(|e| e == "deb")
            .unwrap_or(false);
        let is_rpm = artifact_path
            .extension()
            .map(|e| e == "rpm")
            .unwrap_or(false);

        if is_deb {
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("exec sudo dpkg -i '{}'", artifact_path.display()))
                .status()
                .context("running dpkg to install the updated package")?;

            if !status.success() {
                anyhow::bail!(
                    "dpkg install failed. Install manually with: sudo dpkg -i {}",
                    artifact_path.display()
                );
            }
            return Ok(());
        }

        if is_rpm {
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("exec sudo rpm -Uvh '{}'", artifact_path.display()))
                .status()
                .context("running rpm to install the updated package")?;

            if !status.success() {
                anyhow::bail!(
                    "rpm install failed. Install manually with: sudo rpm -Uvh {}",
                    artifact_path.display()
                );
            }
            return Ok(());
        }

        anyhow::bail!(
            "unrecognized Linux update artifact {} (expected .deb or .rpm)",
            artifact_path.display()
        )
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        anyhow::bail!("automatic update is not supported on this platform")
    }
}
