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

    if let Some(url) = proxy_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            let proxy = Proxy::all(trimmed)
                .with_context(|| format!("invalid proxy URL: {trimmed}"))?;
            builder = builder.proxy(proxy);
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
