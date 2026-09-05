use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::Proxy;

use super::HubRequestOptions;

pub(crate) fn http_client_with_proxy(proxy_url: Option<&str>) -> Result<Client> {
    http_client_with_proxy_and_timeout(proxy_url, None)
}

pub(crate) fn http_client_for_content(proxy_url: Option<&str>) -> Result<Client> {
    http_client_with_proxy_and_timeout(proxy_url, Some(Duration::from_secs(3600)))
}

pub(crate) fn http_client_with_proxy_and_timeout(
    proxy_url: Option<&str>,
    timeout: Option<Duration>,
) -> Result<Client> {
    let mut builder = Client::builder().user_agent("ss14-launcher-rust/0.1");

    let explicit_proxy = proxy_url
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    match explicit_proxy {
        Some(url) => {
            eprintln!("[http] building client with explicit proxy: {url}");
            let proxy = Proxy::all(&url).with_context(|| format!("invalid proxy URL: {url}"))?;
            builder = builder.proxy(proxy);
        }
        None => {
        builder = builder.no_proxy();
        }
    }

    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
        builder = builder.connect_timeout(Duration::from_secs(60));
    }

    builder.build().context("building HTTP client")
}

pub(crate) fn hub_http_client(options: HubRequestOptions) -> Result<Client> {
    http_client_with_proxy(options.proxy_url.as_deref())
}

