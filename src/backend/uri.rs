use anyhow::{Context, Result};

use super::{ServerBuildInformation, ServerInfo, SS14_DEFAULT_PORT};

pub(crate) fn parse_ss14_uri(address: &str) -> Result<reqwest::Url> {
    let full = if address.contains("://") {
        address.to_string()
    } else {
        format!("ss14://{address}")
    };

    let parsed = reqwest::Url::parse(&full).context("invalid server address")?;
    if parsed.scheme() != "ss14" && parsed.scheme() != "ss14s" {
        anyhow::bail!("server address scheme must be ss14 or ss14s");
    }

    if parsed.host().is_none() {
        anyhow::bail!("server address has no host");
    }

    Ok(parsed)
}

pub(crate) fn get_server_api_address(server_address: &str) -> Result<String> {
    let parsed = parse_ss14_uri(server_address)?;

    let host = parsed.host_str().context("server URI has no host")?;
    let (http_scheme, port) = if parsed.scheme() == "ss14" {
        ("http", parsed.port().or(Some(SS14_DEFAULT_PORT)))
    } else {
        ("https", parsed.port())
    };

    let base = match port {
        Some(p) => format!("{http_scheme}://{host}:{p}"),
        None => format!("{http_scheme}://{host}"),
    };

    let mut path = parsed.path().trim_start_matches('/').to_string();
    if !path.is_empty() && !path.ends_with('/') {
        path.push('/');
    }

    if path.is_empty() {
        Ok(format!("{base}/"))
    } else {
        Ok(format!("{base}/{path}"))
    }
}

pub(crate) fn resolve_maybe_relative_url(base: &str, maybe_relative: &str) -> Result<String> {
    if maybe_relative.starts_with("http://") || maybe_relative.starts_with("https://") {
        return Ok(maybe_relative.to_string());
    }

    let base_url = reqwest::Url::parse(base).context("invalid base URL while resolving relative URL")?;
    base_url
        .join(maybe_relative)
        .map(|u| u.to_string())
        .context("failed to resolve relative download URL")
}

pub fn apply_acz_inferred_urls(
    server_address: &str,
    build: &ServerBuildInformation,
) -> Result<ServerBuildInformation> {
    let mut out = build.clone();

    let acz = build.acz.unwrap_or(false) || non_empty(build.download_url.as_deref()).is_none();

    if acz {
        let api_base = get_server_api_address(server_address)?;

        if non_empty(build.download_url.as_deref()).is_none() {
            out.download_url = Some(format!("{api_base}client.zip"));
        }

        if non_empty(out.manifest_url.as_deref()).is_none() {
            out.manifest_url = Some(format!("{api_base}manifest.txt"));
        }
        if non_empty(out.manifest_download_url.as_deref()).is_none() {
            out.manifest_download_url = Some(format!("{api_base}download"));
        }
    }

    Ok(out)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

pub fn derive_connect_address(server_address: &str, info: &ServerInfo) -> Result<String> {
    if let Some(explicit) = &info.connect_address {
        if !explicit.trim().is_empty() {
            return Ok(explicit.trim().to_string());
        }
    }

    let uri = parse_ss14_uri(server_address)?;
    let host = uri.host().context("server URI has no host")?;
    let port = uri.port().unwrap_or_else(|| {
        if uri.scheme() == "ss14" {
            SS14_DEFAULT_PORT
        } else {
            443
        }
    });

    Ok(format!("udp://{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acz_inference_fills_missing_urls() {
        let build = ServerBuildInformation {
            download_url: None,
            manifest_url: None,
            manifest_download_url: None,
            engine_version: Some("263.0.0".into()),
            version: Some("1.0".into()),
            fork_id: Some("WizardsDen".into()),
            hash: None,
            manifest_hash: Some("abc123".into()),
            acz: Some(true),
        };

        let out = apply_acz_inferred_urls("ss14://127.0.0.1:1212", &build).unwrap();
        assert_eq!(out.download_url.as_deref(), Some("http://127.0.0.1:1212/client.zip"));
        assert_eq!(out.manifest_url.as_deref(), Some("http://127.0.0.1:1212/manifest.txt"));
        assert_eq!(
            out.manifest_download_url.as_deref(),
            Some("http://127.0.0.1:1212/download")
        );
        assert_eq!(out.engine_version.as_deref(), Some("263.0.0"));
        assert_eq!(out.manifest_hash.as_deref(), Some("abc123"));
    }

    #[test]
    fn acz_inference_preserves_existing_urls() {
        let build = ServerBuildInformation {
            download_url: Some("http://cdn.example.com/client.zip".into()),
            manifest_url: Some("http://cdn.example.com/manifest.txt".into()),
            manifest_download_url: Some("http://cdn.example.com/download".into()),
            engine_version: Some("263.0.0".into()),
            version: Some("1.0".into()),
            fork_id: Some("WizardsDen".into()),
            hash: None,
            manifest_hash: None,
            acz: Some(true),
        };

        let out = apply_acz_inferred_urls("ss14://127.0.0.1:1212", &build).unwrap();
        assert_eq!(
            out.download_url.as_deref(),
            Some("http://cdn.example.com/client.zip")
        );
        assert_eq!(
            out.manifest_url.as_deref(),
            Some("http://cdn.example.com/manifest.txt")
        );
    }

    #[test]
    fn non_acz_with_download_url_not_modified() {
        let build = ServerBuildInformation {
            download_url: Some("http://cdn.example.com/client.zip".into()),
            manifest_url: None,
            manifest_download_url: None,
            engine_version: Some("263.0.0".into()),
            version: Some("1.0".into()),
            fork_id: Some("WizardsDen".into()),
            hash: None,
            manifest_hash: None,
            acz: Some(false),
        };

        let out = apply_acz_inferred_urls("ss14://127.0.0.1:1212", &build).unwrap();
        assert_eq!(
            out.download_url.as_deref(),
            Some("http://cdn.example.com/client.zip")
        );
        assert_eq!(out.manifest_url, None);
        assert_eq!(out.manifest_download_url, None);
    }

    #[test]
    fn api_address_handles_path() {
        let base = get_server_api_address("ss14://host:1212/xyz").unwrap();
        assert_eq!(base, "http://host:1212/xyz/");
        assert_eq!(
            get_server_api_address("ss14://host:1212").unwrap(),
            "http://host:1212/"
        );
    }
}
