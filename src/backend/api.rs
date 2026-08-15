use anyhow::{Context, Result};

use chrono::{DateTime, Utc};

use reqwest::StatusCode;
use serde::Deserialize;

use super::http;
use super::uri;
use super::{
    normalize_base_url, AccountProfile, HubRequestOptions, HubServerEntry, ServerInfo,
};

pub fn fetch_hub_servers_with_options(
    hub_url: &str,
    options: HubRequestOptions,
) -> Result<Vec<HubServerEntry>> {
    let base = normalize_base_url(hub_url);
    let url = format!("{base}api/servers");
    let client = http::hub_http_client(options)?;

    let response = client
        .get(url)
        .send()
        .context("requesting hub server list")?;

    if !response.status().is_success() {
        anyhow::bail!("hub responded with {}", response.status());
    }

    response
        .json::<Vec<HubServerEntry>>()
        .context("parsing hub server list")
}

pub fn fetch_server_info_from_hub_with_options(
    hub_url: &str,
    server_address: &str,
    options: HubRequestOptions,
) -> Result<ServerInfo> {
    let base = normalize_base_url(hub_url);
    let encoded_addr = urlencoding::encode(server_address);
    let url = format!("{base}api/servers/info?url={encoded_addr}");

    let client = http::hub_http_client(options)?;
    let response = client
        .get(url)
        .send()
        .context("requesting server info from hub")?;

    if !response.status().is_success() {
        anyhow::bail!("hub server info endpoint responded with {}", response.status());
    }

    response.json::<ServerInfo>().context("parsing server info")
}

pub fn fetch_server_info_direct_with_proxy(
    server_address: &str,
    proxy_url: Option<&str>,
) -> Result<ServerInfo> {
    let api_base = uri::get_server_api_address(server_address)?;
    let url = format!("{api_base}info");

    let client = http::http_client_with_proxy(proxy_url)?;
    let response = client
        .get(url)
        .send()
        .context("requesting server info directly")?;

    if !response.status().is_success() {
        anyhow::bail!("direct server info endpoint responded with {}", response.status());
    }

    response
        .json::<ServerInfo>()
        .context("parsing direct server info response")
}

pub fn authenticate_account_with_proxy(
    auth_url: &str,
    username: &str,
    password: &str,
    proxy_url: Option<&str>,
) -> Result<AccountProfile> {
    let auth_base = normalize_base_url(auth_url);
    let client = http::http_client_with_proxy(proxy_url)?;
    let url = format!("{auth_base}api/auth/authenticate");

    let payload = serde_json::json!({
        "username": username,
        "password": password
    });

    let response = client
        .post(url)
        .json(&payload)
        .send()
        .context("sending authentication request")?;

    if response.status().is_success() {
        let body = response
            .json::<AuthSuccessResponse>()
            .context("parsing successful auth response")?;

        return Ok(AccountProfile {
            auth_server: auth_base,
            username: body.username,
            user_id: body.user_id,
            token: body.token,
            expire_time: body.expire_time,
        });
    }

    if response.status() == StatusCode::UNAUTHORIZED {
        let text = response.text().unwrap_or_else(|_| String::from("<no response body>"));
        anyhow::bail!("authentication denied: {text}");
    }

    anyhow::bail!("auth server responded with {}", response.status())
}

#[derive(Debug, Deserialize)]
struct AuthSuccessResponse {
    #[serde(rename = "token", alias = "Token")]
    token: String,
    #[serde(rename = "username", alias = "Username")]
    username: String,
    #[serde(rename = "userId", alias = "UserId")]
    user_id: String,
    #[serde(rename = "expireTime", alias = "ExpireTime")]
    expire_time: DateTime<Utc>,
}


