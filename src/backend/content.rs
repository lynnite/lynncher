
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use blake2::digest::typenum::U32;
use blake2::{Blake2b, Digest};
use reqwest::blocking::Client;

use super::http::http_client_for_content;
use super::uri::{get_server_api_address, resolve_maybe_relative_url};
use super::{display_path, LauncherPaths, ServerBuildInformation};

const DOWNLOAD_PROTOCOL_VERSION: i32 = 1;

const FLAG_PRE_COMPRESSED: i32 = 1 << 0;

const MANIFEST_HEADER: &str = "Robust Content Manifest 1";

#[derive(Debug)]
struct ManifestEntry {
    hash: String,
    path: String,
}

pub fn download_and_install_content(
    paths: &LauncherPaths,
    server_address: &str,
    build: &ServerBuildInformation,
    proxy_url: Option<&str>,
) -> Result<Option<PathBuf>> {
    let Some(manifest_url) = non_empty(build.manifest_url.as_deref()) else {
        return Ok(None);
    };
    let Some(download_url) = non_empty(build.manifest_download_url.as_deref()) else {
        return Ok(None);
    };
    let Some(expected_hash) = non_empty(build.manifest_hash.as_deref()) else {
        return Ok(None);
    };

    let manifest_url = resolve_url(server_address, Some(manifest_url), "manifest URL")?;
    let download_url = resolve_url(server_address, Some(download_url), "manifest download URL")?;

    let target = build_target_id(build);
    let cache_dir = paths.clients_dir.join("content-cache");
    let install_dir = paths.clients_dir.join(format!("content-{target}"));

    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("creating {}", display_path(&cache_dir)))?;
    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    let client = http_client_for_content(proxy_url)?;

    let entries = fetch_content_manifest(&client, &manifest_url, expected_hash)?;

    let mut to_download: Vec<&ManifestEntry> = Vec::new();
    {
        let mut seen_for_dedup = HashSet::new();
        for entry in &entries {
            if seen_for_dedup.contains(&entry.hash) {
                continue;
            }
            seen_for_dedup.insert(entry.hash.clone());

            let blob_path = blob_path(&cache_dir, &entry.hash);
            if !blob_path.exists() {
                to_download.push(entry);
            }
        }
    }

    if !to_download.is_empty() {
        download_content_blobs(&client, &download_url, &entries, &cache_dir)?;
    }

    for entry in &entries {
        let src = blob_path(&cache_dir, &entry.hash);
        if !src.exists() {
            anyhow::bail!(
                "content blob for '{}' is missing after download ({})",
                entry.path,
                display_path(&src)
            );
        }

        let dest = install_dir.join(&entry.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", display_path(parent)))?;
        }

        fs::copy(&src, &dest)
            .with_context(|| format!("materializing {}", display_path(&dest)))?;
    }

    Ok(Some(install_dir))
}

pub struct ContentFile {
    pub path: String,
    pub hash_hex: String,
}

pub fn download_content_entries(
    paths: &LauncherPaths,
    server_address: &str,
    build: &ServerBuildInformation,
    proxy_url: Option<&str>,
) -> Result<Option<(PathBuf, Vec<ContentFile>, String)>> {
    let Some(manifest_url) = non_empty(build.manifest_url.as_deref()) else {
        return Ok(None);
    };
    let Some(download_url) = non_empty(build.manifest_download_url.as_deref()) else {
        return Ok(None);
    };
    let Some(expected_hash) = non_empty(build.manifest_hash.as_deref()) else {
        return Ok(None);
    };

    let manifest_url = resolve_url(server_address, Some(manifest_url), "manifest URL")?;
    let download_url = resolve_url(server_address, Some(download_url), "manifest download URL")?;

    let cache_dir = paths.clients_dir.join("content-cache");
    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("creating {}", display_path(&cache_dir)))?;

    let client = http_client_for_content(proxy_url)?;
    let entries = fetch_content_manifest(&client, &manifest_url, expected_hash)?;

    let mut to_download: Vec<&ManifestEntry> = Vec::new();
    {
        let mut seen_for_dedup = HashSet::new();
        for entry in &entries {
            if seen_for_dedup.contains(&entry.hash) {
                continue;
            }
            seen_for_dedup.insert(entry.hash.clone());

            let blob_path = blob_path(&cache_dir, &entry.hash);
            if !blob_path.exists() {
                to_download.push(entry);
            }
        }
    }

    if !to_download.is_empty() {
        download_content_blobs(&client, &download_url, &entries, &cache_dir)?;
    }

    let content_files = entries
        .iter()
        .map(|e| ContentFile {
            path: e.path.clone(),
            hash_hex: e.hash.clone(),
        })
        .collect();

    Ok(Some((cache_dir, content_files, expected_hash.to_string())))
}

pub fn download_content_zip(
    paths: &LauncherPaths,
    server_address: &str,
    build: &ServerBuildInformation,
    proxy_url: Option<&str>,
) -> Result<Option<PathBuf>> {
    let Some(download_url) = non_empty(build.download_url.as_deref()) else {
        return Ok(None);
    };
    let download_url = resolve_url(server_address, Some(download_url), "content download URL")?;

    let target = build_target_id(build);
    let install_dir = paths.clients_dir.join(format!("content-{target}"));
    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    if fs::read_dir(&install_dir)
        .ok()
        .and_then(|mut d| d.next())
        .is_some()
    {
        return Ok(Some(install_dir));
    }

    let client = http_client_for_content(proxy_url)?;
    let response = client
        .get(&download_url)
        .send()
        .with_context(|| format!("requesting content zip from {download_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "content zip endpoint responded with {} ({download_url})",
            response.status()
        );
    }

    let body = response.bytes().context("reading content zip body")?;
    extract_zip_to_dir(std::io::Cursor::new(body), &install_dir)?;

    Ok(Some(install_dir))
}

fn extract_zip_to_dir<R: std::io::Read + std::io::Seek>(
    reader: R,
    out_dir: &Path,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).context("opening content zip")?;

    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).context("reading content zip entry")?;

        let relative = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => continue,
        };

        if is_unsafe_relative_path(&relative.to_string_lossy()) {
            anyhow::bail!(
                "content zip contains unsafe path {}",
                relative.to_string_lossy()
            );
        }

        let out_path = out_dir.join(&relative);

        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("creating {}", display_path(&out_path)))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", display_path(parent)))?;
        }

        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("creating {}", display_path(&out_path)))?;
        std::io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("extracting {}", display_path(&out_path)))?;
    }

    Ok(())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn resolve_url(server_address: &str, url: Option<&str>, what: &str) -> Result<String> {
    let Some(raw) = url else {
        anyhow::bail!("server did not provide a {what}");
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("server provided an empty {what}");
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(trimmed.to_string());
    }
    let api_base = get_server_api_address(server_address)?;
    resolve_maybe_relative_url(&api_base, trimmed).with_context(|| format!("resolving {what}"))
}

fn fetch_content_manifest(
    client: &Client,
    manifest_url: &str,
    expected_hash_hex: &str,
) -> Result<Vec<ManifestEntry>> {
    let response = client
        .get(manifest_url)
        .header(reqwest::header::ACCEPT_ENCODING, "zstd")
        .send()
        .with_context(|| format!("requesting content manifest from {manifest_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "content manifest responded with {} ({manifest_url})",
            response.status()
        );
    }

    let is_zstd = response_is_zstd(&response);
    let body = response.bytes().context("reading content manifest body")?;
    let raw = decode_body(&body, is_zstd, "content manifest")?;

    let actual_hash = blake2b_256_hex(&raw);
    if !actual_hash.eq_ignore_ascii_case(expected_hash_hex) {
        anyhow::bail!(
            "content manifest hash mismatch: expected {expected_hash_hex}, got {actual_hash}"
        );
    }

    parse_manifest(&raw)
}

fn parse_manifest(raw: &[u8]) -> Result<Vec<ManifestEntry>> {
    let text = std::str::from_utf8(raw).context("content manifest is not valid UTF-8")?;
    let mut lines = text.lines();

    match lines.next() {
        Some(first) if first == MANIFEST_HEADER => {}
        Some(other) => anyhow::bail!("unknown content manifest header: {other:?}"),
        None => anyhow::bail!("empty content manifest"),
    }

    let mut entries = Vec::new();
    for (idx, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let Some(sep) = line.find(' ') else {
            anyhow::bail!("malformed content manifest line {}: missing path", idx + 2);
        };

        let hash = line[..sep].to_string();
        let path = line[sep + 1..].to_string();

        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            anyhow::bail!(
                "malformed content manifest line {}: bad hash {hash:?}",
                idx + 2
            );
        }

        if is_unsafe_relative_path(&path) {
            anyhow::bail!(
                "content manifest contains unsafe path {path:?} (line {})",
                idx + 2
            );
        }

        entries.push(ManifestEntry {
            hash: hash.to_ascii_lowercase(),
            path,
        });
    }

    Ok(entries)
}

fn is_unsafe_relative_path(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with('\\') {
        return true;
    }

    for comp in path.split(['/', '\\']) {
        if comp == ".." {
            return true;
        }
    }

    false
}

fn download_content_blobs(
    client: &Client,
    download_url: &str,
    entries: &[ManifestEntry],
    cache_dir: &Path,
) -> Result<()> {
    check_download_protocol(client, download_url)?;

    let mut indices: Vec<usize> = Vec::new();
    let mut requested: HashSet<String> = HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        if requested.contains(&entry.hash) {
            continue;
        }
        let blob_path = blob_path(cache_dir, &entry.hash);
        if !blob_path.exists() {
            indices.push(i);
            requested.insert(entry.hash.clone());
        }
    }

    if indices.is_empty() {
        return Ok(());
    }

    let mut body = Vec::with_capacity(indices.len() * 4);
    for idx in &indices {
        body.extend_from_slice(&(*idx as i32).to_le_bytes());
    }

    let response = client
        .post(download_url)
        .header("X-Robust-Download-Protocol", DOWNLOAD_PROTOCOL_VERSION.to_string())
        .header(reqwest::header::ACCEPT_ENCODING, "zstd")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .send()
        .with_context(|| format!("requesting content blobs from {download_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "content download responded with {} ({download_url})",
            response.status()
        );
    }

    let is_zstd = response_is_zstd(&response);
    let stream = response.bytes().context("reading content download body")?;
    let stream = decode_body(&stream, is_zstd, "content download stream")?;

    let pre_compressed = {
        if stream.len() < 4 {
            anyhow::bail!("content download stream is too short for its header");
        }
        let flags = i32::from_le_bytes([stream[0], stream[1], stream[2], stream[3]]);
        (flags & FLAG_PRE_COMPRESSED) != 0
    };

    let file_header_len = if pre_compressed { 8 } else { 4 };
    let mut offset = 4usize;

    for (pos, idx) in indices.iter().enumerate() {
        let entry = &entries[*idx];

        if offset + file_header_len > stream.len() {
            anyhow::bail!("content download stream truncated at blob {pos} (file header)");
        }

        let length =
            i32::from_le_bytes([stream[offset], stream[offset + 1], stream[offset + 2], stream[offset + 3]]) as usize;
        offset += 4;

        let uncompressed: Vec<u8> = if pre_compressed {
            let compressed_len = i32::from_le_bytes([
                stream[offset],
                stream[offset + 1],
                stream[offset + 2],
                stream[offset + 3],
            ]) as usize;
            offset += 4;

            if compressed_len == 0 {
                if offset + length > stream.len() {
                    anyhow::bail!("content download stream truncated at blob {pos} (raw data)");
                }
                let raw = &stream[offset..offset + length];
                offset += length;
                raw.to_vec()
            } else {
                if offset + compressed_len > stream.len() {
                    anyhow::bail!(
                        "content download stream truncated at blob {pos} (compressed data)"
                    );
                }
                let compressed = &stream[offset..offset + compressed_len];
                offset += compressed_len;
                let decoded = zstd::stream::decode_all(std::io::Cursor::new(compressed))
                    .with_context(|| format!("decompressing content blob {pos}"))?;
                if decoded.len() != length {
                    anyhow::bail!(
                        "content blob {pos} had incorrect decompressed size: expected {length}, got {}",
                        decoded.len()
                    );
                }
                decoded
            }
        } else {
            if offset + length > stream.len() {
                anyhow::bail!("content download stream truncated at blob {pos} (data)");
            }
            let raw = &stream[offset..offset + length];
            offset += length;
            raw.to_vec()
        };

        verify_and_store_blob(cache_dir, entry, &uncompressed, pos)?;
    }

    Ok(())
}

fn verify_and_store_blob(
    cache_dir: &Path,
    entry: &ManifestEntry,
    data: &[u8],
    pos: usize,
) -> Result<()> {
    let actual = blake2b_256_hex(data);
    if !actual.eq_ignore_ascii_case(&entry.hash) {
        anyhow::bail!(
            "content blob {pos} ('{}') hash mismatch: expected {}, got {actual}",
            entry.path,
            entry.hash
        );
    }

    let dest = blob_path(cache_dir, &entry.hash);
    fs::write(&dest, data).with_context(|| format!("writing {}", display_path(&dest)))?;
    Ok(())
}

fn blob_path(cache_dir: &Path, hash: &str) -> PathBuf {
    cache_dir.join(hash)
}

fn blake2b_256_hex(data: &[u8]) -> String {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(data);
    let out = hasher.finalize();
    let out: &[u8] = &out;
    let mut hex = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

pub fn hash_bytes_hex(data: &[u8]) -> String {
    blake2b_256_hex(data)
}

fn response_is_zstd(response: &reqwest::blocking::Response) -> bool {
    response
        .headers()
        .get_all(reqwest::header::CONTENT_ENCODING)
        .iter()
        .any(|v| v.to_str().map(|s| s.eq_ignore_ascii_case("zstd")).unwrap_or(false))
}

fn decode_body(body: &[u8], is_zstd: bool, what: &str) -> Result<Vec<u8>> {
    if !is_zstd {
        return Ok(body.to_vec());
    }

    zstd::stream::decode_all(std::io::Cursor::new(body))
        .with_context(|| format!("decompressing {what}"))
}

fn check_download_protocol(client: &Client, download_url: &str) -> Result<()> {
    let response = client
        .request(reqwest::Method::OPTIONS, download_url)
        .send()
        .with_context(|| format!("probing content download endpoint {download_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "content download endpoint OPTIONS responded with {} ({download_url})",
            response.status()
        );
    }

    let min = header_int(&response, "X-Robust-Download-Min-Protocol")?;
    let max = header_int(&response, "X-Robust-Download-Max-Protocol")?;

    if min > DOWNLOAD_PROTOCOL_VERSION || max < DOWNLOAD_PROTOCOL_VERSION {
        anyhow::bail!(
            "server does not support download protocol {} (supports {min}..={max})",
            DOWNLOAD_PROTOCOL_VERSION
        );
    }

    Ok(())
}

fn header_int(response: &reqwest::blocking::Response, name: &str) -> Result<i32> {
    let value = response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow!("missing expected header {name} from content download endpoint"))?;

    value
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid {name}: {value:?}"))
}

fn build_target_id(build: &ServerBuildInformation) -> String {
    let fork = build.fork_id.as_deref().unwrap_or("unknown-fork");
    let version = build.version.as_deref().unwrap_or("unknown-version");

    let mut id = format!("{fork}-{version}");
    id = id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if id == "unknown-fork-unknown-version" {
        return "unknown".to_string();
    }

    id
}

pub fn merge_content_into(content_dir: &Path, engine_dir: &Path) -> Result<()> {
    fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
        if src.is_dir() {
            fs::create_dir_all(dst)
                .with_context(|| format!("creating {}", display_path(dst)))?;
            for entry in fs::read_dir(src).with_context(|| format!("reading {}", display_path(src)))? {
                let entry = entry.with_context(|| format!("reading {}", display_path(src)))?;
                let child_src = entry.path();
                let child_dst = dst.join(entry.file_name());
                copy_tree(&child_src, &child_dst)?;
            }
        } else if src.is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", display_path(parent)))?;
            }
            fs::copy(src, dst).with_context(|| format!("copying {}", display_path(dst)))?;
        }
        Ok(())
    }

    copy_tree(content_dir, engine_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake2b_256_matches_known_vectors() {
        assert_eq!(
            blake2b_256_hex(b""),
            "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"
        );
        assert_eq!(
            blake2b_256_hex(b"abc"),
            "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319"
        );
    }

    #[test]
    fn parse_manifest_accepts_valid_entries() {
        let raw = format!(
            "{header}\n{hash1} {path1}\n{hash2} {path2}\n",
            header = MANIFEST_HEADER,
            hash1 = "a".repeat(64),
            path1 = "Resources/Textures/foo.png",
            hash2 = "b".repeat(64),
            path2 = "Content.Client.dll"
        );
        let out = parse_manifest(raw.as_bytes());
        assert!(out.is_ok(), "expected valid manifest to parse");
        let entries = out.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "Resources/Textures/foo.png");
        assert_eq!(entries[1].hash.len(), 64);
    }

    #[test]
    fn parse_manifest_rejects_bad_header() {
        let err = parse_manifest(b"Wrong Header\n").unwrap_err();
        assert!(format!("{err:#}").contains("unknown content manifest header"));
    }

    #[test]
    fn parse_manifest_rejects_unsafe_paths() {
        let raw = format!("{MANIFEST_HEADER}\n{}\n", format!("{} ../evil", "c".repeat(64)));
        assert!(parse_manifest(raw.as_bytes()).is_err());

        let raw2 = format!("{MANIFEST_HEADER}\n{} /etc/passwd\n", "d".repeat(64));
        assert!(parse_manifest(raw2.as_bytes()).is_err());
    }

    #[test]
    fn unsafe_path_detection() {
        assert!(is_unsafe_relative_path("../escape"));
        assert!(is_unsafe_relative_path("/abs"));
        assert!(is_unsafe_relative_path("a/../../b"));
        assert!(!is_unsafe_relative_path("Resources/foo.bar"));
        assert!(!is_unsafe_relative_path("foo"));
    }

    #[test]
    fn build_target_id_sanitizes() {
        let build = ServerBuildInformation {
            download_url: None,
            manifest_url: None,
            manifest_download_url: None,
            engine_version: None,
            version: Some("1.2.3".into()),
            fork_id: Some("Spicy/Menu".into()),
            hash: None,
            manifest_hash: None,
            acz: None,
        };
        let id = build_target_id(&build);
        assert_eq!(id, "Spicy_Menu-1.2.3");
    }

    #[test]
    fn non_empty_filters_blank() {
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some("")), None);
        assert_eq!(non_empty(Some("   ")), None);
        assert_eq!(non_empty(Some(" abc ")), Some("abc"));
    }

    #[test]
    fn extract_zip_to_dir_writes_files() {
        use std::io::Write;

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("Resources/foo.txt", opts).unwrap();
            w.write_all(b"hello").unwrap();
            w.finish().unwrap();
        }
        buf.set_position(0);

        let out_dir = std::env::temp_dir().join(format!("content-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out_dir);
        extract_zip_to_dir(buf, &out_dir).unwrap();
        assert_eq!(fs::read_to_string(out_dir.join("Resources/foo.txt")).unwrap(), "hello");
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn manifest_requires_all_three_fields() {
        let build = ServerBuildInformation {
            download_url: None,
            manifest_url: Some("http://x/manifest".into()),
            manifest_download_url: Some("http://x/download".into()),
            engine_version: None,
            version: Some("1.0".into()),
            fork_id: Some("fork".into()),
            hash: None,
            manifest_hash: None,
            acz: None,
        };
        assert_eq!(non_empty(build.manifest_hash.as_deref()), None);
        assert_eq!(non_empty(build.manifest_url.as_deref()), Some("http://x/manifest"));
    }
}
