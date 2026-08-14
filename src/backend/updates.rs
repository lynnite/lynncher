use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

const RELEASES_URL: &str = "https://api.github.com/repos/lynnite/lynncher/releases/latest";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: Option<String>,
    name: Option<String>,
    html_url: Option<String>,
}

/// Fetch the URL of the latest release page on GitHub.
pub fn latest_release_url(proxy_url: Option<&str>) -> Result<Option<String>> {
    let mut builder = Client::builder()
        .user_agent("ss14-launcher-rust")
        .timeout(std::time::Duration::from_secs(15));

    if let Some(url) = proxy_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(trimmed) {
                builder = builder.proxy(proxy);
            }
        }
    }

    let client = builder.build().context("building HTTP client")?;
    let response = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .context("querying GitHub releases")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub releases API responded with {}", response.status());
    }

    let release: GithubRelease = response.json().context("parsing GitHub release")?;
    Ok(release.html_url)
}

/// Query the latest release tag from the lynncher GitHub repo.
/// Returns `None` when no tag could be determined.
pub fn check_latest_release(proxy_url: Option<&str>) -> Result<Option<String>> {
    let mut builder = Client::builder()
        .user_agent("ss14-launcher-rust")
        .timeout(std::time::Duration::from_secs(15));

    if let Some(url) = proxy_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(trimmed) {
                builder = builder.proxy(proxy);
            }
        }
    }

    let client = builder.build().context("building HTTP client")?;

    let response = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .context("querying GitHub releases")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub releases API responded with {}", response.status());
    }

    let release: GithubRelease = response.json().context("parsing GitHub release")?;
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
