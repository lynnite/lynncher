use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::Proxy;

use super::HubRequestOptions;

pub(crate) fn http_client_with_proxy(proxy_url: Option<&str>) -> Result<Client> {
    http_client_with_proxy_and_timeout(proxy_url, None)
}

pub(crate) fn http_client_for_content(proxy_url: Option<&str>) -> Result<Client> {
    http_client_with_proxy_and_timeout_and_case(proxy_url, Some(Duration::from_secs(3600)), true)
}

pub(crate) fn http_client_with_proxy_and_timeout(
    proxy_url: Option<&str>,
    timeout: Option<Duration>,
) -> Result<Client> {
    http_client_with_proxy_and_timeout_and_case(proxy_url, timeout, false)
}

pub(crate) fn http_client_with_proxy_and_timeout_and_case(
    proxy_url: Option<&str>,
    timeout: Option<Duration>,
    title_case_headers: bool,
) -> Result<Client> {
    let mut builder = Client::builder().user_agent("ss14-launcher-rust/0.1");

    if title_case_headers {
        builder = builder.http1_title_case_headers();
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn content_client_sends_title_case_protocol_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = stream.flush();
            head
        });

        let client = http_client_for_content(None).unwrap();
        let url = format!("http://{addr}/download");
        let resp = client
            .post(&url)
            .header("X-Robust-Download-Protocol", "1")
            .body(vec![0u8, 0, 0, 0])
            .send()
            .unwrap();
        assert!(resp.status().is_success());

        let raw = server.join().unwrap();
        assert!(
            raw.contains("X-Robust-Download-Protocol: 1"),
            "content client must send the title-cased protocol header;\nraw request:\n{raw}"
        );
        assert!(
            !raw.contains("x-robust-download-protocol: 1"),
            "content client must not send the lowercase protocol header;\nraw request:\n{raw}"
        );
    }

    #[test]
    fn default_client_uses_lowercase_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = stream.flush();
            head
        });

        let client = http_client_with_proxy(None).unwrap();
        let url = format!("http://{addr}/");
        let resp = client
            .get(&url)
            .header("X-Robust-Download-Protocol", "1")
            .send()
            .unwrap();
        assert!(resp.status().is_success());

        let raw = server.join().unwrap();
        assert!(
            raw.contains("x-robust-download-protocol: 1"),
            "default client should keep default (lowercase) header casing;\nraw request:\n{raw}"
        );
    }
}
