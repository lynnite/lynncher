use std::fs;
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::AUTHORIZATION;
use reqwest::StatusCode;
use serde::Deserialize;
use walkdir::WalkDir;
use zip::ZipArchive;

use super::http::{
    http_client_for_content, http_client_with_proxy, http_client_with_proxy_and_timeout,
};
use super::uri::{get_server_api_address, resolve_maybe_relative_url};
use super::{display_path, ClientInstall, LauncherPaths, ServerBuildInformation, ServerInfo};

const ROBUST_MANIFEST_URLS: [&str; 2] = [
    "https://robust-builds.cdn.spacestation14.com/manifest.json",
    "https://robust-builds.fallback.cdn.spacestation14.com/manifest.json",
];

const ROBUST_MODULES_MANIFEST_URLS: [&str; 2] = [
    "https://robust-builds.cdn.spacestation14.com/modules.json",
    "https://robust-builds.fallback.cdn.spacestation14.com/modules.json",
];

const ROBUST_NATIVES_SDL3_PACKAGE_ID: &str = "Robust.Natives.Sdl3";
const ROBUST_NATIVES_SDL3_PACKAGE_VERSIONS: [&str; 2] = ["0.1.3-sdl3.4.8", "0.1.2-sdl3.4.0"];

pub fn is_connection_cancelled(err: &anyhow::Error) -> bool {
    err.to_string().contains("download cancelled by user")
}

fn abortable_copy<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    abort: Option<&AtomicBool>,
    label: &str,
) -> Result<u64> {
    let mut buffer = [0u8; 128 * 1024];
    let mut written: u64 = 0;
    loop {
        if let Some(abort) = abort {
            if abort.load(Ordering::SeqCst) {
                anyhow::bail!("download cancelled by user while writing {label}");
            }
        }
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n])?;
        written += n as u64;
    }
    Ok(written)
}

pub fn download_client_for_server_with_proxy_and_tokens(
    paths: &LauncherPaths,
    server_address: &str,
    info: &ServerInfo,
    proxy_url: Option<&str>,
    auth_tokens: &[Option<&str>],
    abort: Option<&AtomicBool>,
) -> Result<ClientInstall> {
    let build = info
        .build
        .as_ref()
        .context("server /info did not provide build information")?;

    let download_url = resolve_client_download_url(server_address, build)?;

    let target_id = build_target_id(server_address, build);
    let install_dir = paths.clients_dir.join(target_id);
    let extracted_dir = install_dir.join("extracted");
    let zip_path = install_dir.join("client.zip");

    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    let client = http_client_for_content(proxy_url)?;

    for header in auth_headers(auth_tokens) {
        let mut request = client.get(&download_url);
        if let Some(header) = &header {
            request = request.header(AUTHORIZATION, header);
        }

        let mut response = request
            .send()
            .with_context(|| format!("requesting client zip from {download_url}"))?;

        if !response.status().is_success() {
            if response.status() == StatusCode::UNAUTHORIZED
                || response.status() == StatusCode::FORBIDDEN
            {
                continue;
            }
            anyhow::bail!(
                "client zip endpoint responded with {} ({download_url})",
                response.status()
            );
        }

        let mut out_file = fs::File::create(&zip_path)
            .with_context(|| format!("creating {}", display_path(&zip_path)))?;
        if let Err(err) = abortable_copy(&mut response, &mut out_file, abort, "client.zip") {
            fs::remove_dir_all(&install_dir).ok();
            return Err(err);
        }
        out_file.flush().context("flushing client zip to disk")?;

        if extracted_dir.exists() {
            fs::remove_dir_all(&extracted_dir)
                .with_context(|| format!("clearing {}", display_path(&extracted_dir)))?;
        }

        fs::create_dir_all(&extracted_dir)
            .with_context(|| format!("creating {}", display_path(&extracted_dir)))?;

        extract_zip(&zip_path, &extracted_dir)?;

        let executable_path = detect_client_executable(&extracted_dir)
            .with_context(|| format!("detecting executable in {}", display_path(&extracted_dir)))?;

        return Ok(ClientInstall {
            install_dir,
            executable_path,
        });
    }

    anyhow::bail!(
        "client zip endpoint rejected authentication ({download_url})"
    )
}

fn resolve_client_download_url(
    server_address: &str,
    build: &ServerBuildInformation,
) -> Result<String> {
    let api_base = get_server_api_address(server_address)?;

    if let Some(url) = build
        .download_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return resolve_maybe_relative_url(&api_base, url);
    }

    Ok(format!("{api_base}client.zip"))
}

fn auth_headers(auth_tokens: &[Option<&str>]) -> Vec<Option<String>> {
    let mut headers = vec![None];

    let mut seen = HashSet::new();
    for token in auth_tokens {
        let trimmed = token.map(str::trim).unwrap_or("").to_string();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            headers.push(Some(format!("SS14Auth {trimmed}")));
        }
    }

    headers
}

pub fn download_engine_client_for_version(
    paths: &LauncherPaths,
    engine_version: &str,
    proxy_url: Option<&str>,
) -> Result<PathBuf> {
    let version = engine_version.trim();
    if version.is_empty() {
        anyhow::bail!("engine version is empty");
    }

    let rid_candidates = current_rid_candidates();
    let (resolved_version, engine_zip_url, _sig) =
        fetch_engine_url_and_signature(paths, proxy_url, version, &rid_candidates)?;

    let install_dir = paths
        .clients_dir
        .join(format!("engine-{resolved_version}"));
    let extracted_dir = install_dir.join("extracted");
    let zip_path = install_dir.join("engine.zip");

    if extracted_dir.exists() {
        if let Ok(exe) = detect_client_executable(&extracted_dir) {
            return Ok(exe);
        }
    }

    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    let client = http_client_with_proxy(proxy_url)?;
    let mut response = client
        .get(&engine_zip_url)
        .send()
        .with_context(|| format!("requesting engine zip from {engine_zip_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "engine zip endpoint responded with {} ({engine_zip_url})",
            response.status()
        );
    }

    let mut out_file = fs::File::create(&zip_path)
        .with_context(|| format!("creating {}", display_path(&zip_path)))?;
    io::copy(&mut response, &mut out_file).context("writing engine zip to disk")?;
    out_file.flush().context("flushing engine zip to disk")?;

    if extracted_dir.exists() {
        fs::remove_dir_all(&extracted_dir)
            .with_context(|| format!("clearing {}", display_path(&extracted_dir)))?;
    }

    fs::create_dir_all(&extracted_dir)
        .with_context(|| format!("creating {}", display_path(&extracted_dir)))?;

    extract_zip(&zip_path, &extracted_dir)?;

    detect_client_executable(&extracted_dir)
        .with_context(|| format!("detecting executable in {}", display_path(&extracted_dir)))
}

fn fetch_robust_manifest(paths: &LauncherPaths, proxy_url: Option<&str>) -> Result<RobustBuildManifest> {
    let cache_file = paths.clients_dir.join("robust-manifest.json");
    let mut last_error: Option<anyhow::Error> = None;

    let client = http_client_with_proxy_and_timeout(proxy_url, Some(Duration::from_secs(15)))
        .context("building manifest HTTP client")?;

    for manifest_url in ROBUST_MANIFEST_URLS {
        let response = match client.get(manifest_url).send() {
            Ok(resp) => resp,
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        if !response.status().is_success() {
            last_error = Some(anyhow::anyhow!(
                "robust manifest responded with {} ({manifest_url})",
                response.status()
            ));
            continue;
        }

        let bytes = match response.bytes() {
            Ok(b) => b.to_vec(),
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        let manifest = match serde_json::from_slice::<RobustBuildManifest>(&bytes) {
            Ok(v) => v,
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        if std::fs::create_dir_all(paths.clients_dir.as_path()).is_ok() {
            let _ = fs::write(&cache_file, &bytes);
        }

        return Ok(manifest);
    }

    if let Some(err) = last_error.as_ref() {
        eprintln!("[manifest] all CDNs unreachable ({err:#}); trying disk cache");
    }

    if let Ok(bytes) = fs::read(&cache_file) {
        if let Ok(m) = serde_json::from_slice::<RobustBuildManifest>(&bytes) {
            eprintln!("[manifest] using disk cache ({})", cache_file.display());
            return Ok(m);
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("failed to fetch robust manifest")))
}

fn fetch_engine_url_and_signature(
    paths: &LauncherPaths,
    proxy_url: Option<&str>,
    engine_version: &str,
    rid_candidates: &[&str],
) -> Result<(String, String, String)> {
    let manifest = fetch_robust_manifest(paths, proxy_url)?;

    let Some((resolved_version, version_entry)) = resolve_redirect(&manifest, engine_version)
    else {
        anyhow::bail!("engine version {engine_version} not found in robust manifest");
    };

    for rid in rid_candidates {
        if let Some(platform) = version_entry.platforms.get(*rid) {
            let signature = platform
                .signature
                .as_deref()
                .and_then(normalize_signature)
                .unwrap_or_default();
            return Ok((resolved_version.clone(), platform.url.clone(), signature));
        }
    }

    anyhow::bail!(
        "engine version {engine_version} (resolved to {resolved_version}) has no supported platform for {:?}",
        rid_candidates
    )
}

pub fn download_engine_zip_for_loader(
    paths: &LauncherPaths,
    engine_version: &str,
    proxy_url: Option<&str>,
) -> Result<(PathBuf, String)> {
    let version = engine_version.trim();
    if version.is_empty() {
        anyhow::bail!("engine version is empty");
    }

    let rid_candidates = current_rid_candidates();
    let (resolved_version, engine_zip_url, signature) =
        fetch_engine_url_and_signature(paths, proxy_url, version, &rid_candidates)?;

    let install_dir = paths
        .clients_dir
        .join(format!("engine-{resolved_version}"));
    let zip_path = install_dir.join("engine.zip");

    if !zip_path.exists() {
        let client = http_client_with_proxy(proxy_url)?;
        fs::create_dir_all(&install_dir)
            .with_context(|| format!("creating {}", display_path(&install_dir)))?;

        let mut response = client
            .get(&engine_zip_url)
            .send()
            .with_context(|| format!("requesting engine zip from {engine_zip_url}"))?;

        if !response.status().is_success() {
            anyhow::bail!(
                "engine zip endpoint responded with {} ({engine_zip_url})",
                response.status()
            );
        }

        let mut out_file = fs::File::create(&zip_path)
            .with_context(|| format!("creating {}", display_path(&zip_path)))?;
        io::copy(&mut response, &mut out_file).context("writing engine zip to disk")?;
        out_file.flush().context("flushing engine zip to disk")?;
    }

    Ok((zip_path, signature))
}

pub fn download_engine_module_for_engine_version(
    paths: &LauncherPaths,
    module_name: &str,
    engine_version: &str,
    proxy_url: Option<&str>,
) -> Result<PathBuf> {
    let module_name = module_name.trim();
    let engine_version = engine_version.trim();
    if module_name.is_empty() || engine_version.is_empty() {
        anyhow::bail!("module name or engine version is empty");
    }

    let client = http_client_with_proxy(proxy_url)?;
    let rid_candidates = current_rid_candidates();
    let (module_version, module_zip_url) =
        fetch_module_zip_url(&client, module_name, engine_version, &rid_candidates)?;

    let module_dir = paths
        .clients_dir
        .join("engine-modules")
        .join(module_name)
        .join(&module_version);
    let zip_path = module_dir.join("module.zip");

    if module_dir.exists() {
        let has_files = fs::read_dir(&module_dir)
            .ok()
            .and_then(|mut d| d.next())
            .is_some();
        if has_files {
            return Ok(module_dir);
        }
    }

    fs::create_dir_all(&module_dir)
        .with_context(|| format!("creating {}", display_path(&module_dir)))?;

    let mut response = client
        .get(&module_zip_url)
        .send()
        .with_context(|| format!("requesting module zip from {module_zip_url}"))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "module zip endpoint responded with {} ({module_zip_url})",
            response.status()
        );
    }

    let mut out_file = fs::File::create(&zip_path)
        .with_context(|| format!("creating {}", display_path(&zip_path)))?;
    io::copy(&mut response, &mut out_file).context("writing module zip to disk")?;
    out_file.flush().context("flushing module zip to disk")?;

    extract_zip(&zip_path, &module_dir)?;
    Ok(module_dir)
}

pub fn stage_sdl3_native_runtime(
    paths: &LauncherPaths,
    executable_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<()> {
    let native_dir = stage_natives_package(
        paths,
        ROBUST_NATIVES_SDL3_PACKAGE_ID,
        &ROBUST_NATIVES_SDL3_PACKAGE_VERSIONS,
        proxy_url,
    )?;

    let target = find_native_library(&native_dir, "libSDL3.so.0")
        .or_else(|| find_native_library(&native_dir, "libSDL3.so"))
        .or_else(|| find_native_library(&native_dir, "SDL3.so"))
        .context("locating libSDL3.so.0 in staged SDL3 package")?;

    create_sdl3_aliases(executable_dir, &target)?;
    Ok(())
}

fn extract_zip(zip_path: &Path, extracted_dir: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("opening {}", display_path(zip_path)))?;
    let mut archive = ZipArchive::new(file).context("opening zip archive")?;

    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).context("reading zip entry")?;

        let relative = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => continue,
        };

        let out_path = extracted_dir.join(relative);

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
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("extracting {}", display_path(&out_path)))?;
    }

    Ok(())
}

fn stage_natives_package(
    paths: &LauncherPaths,
    package_id: &str,
    package_versions: &[&str],
    proxy_url: Option<&str>,
) -> Result<PathBuf> {
    let install_dir = paths
        .clients_dir
        .join("native-packages")
        .join(package_id);

    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    for version in package_versions {
        let version = version.trim();
        if version.is_empty() {
            continue;
        }

        let extracted_dir = install_dir.join(version).join("extracted");
        if extracted_dir.exists() {
            if find_native_library(&extracted_dir, "libSDL3.so.0").is_some()
                || find_native_library(&extracted_dir, "libSDL3.so").is_some()
                || find_native_library(&extracted_dir, "SDL3.so").is_some()
            {
                return Ok(extracted_dir);
            }
        }

        let zip_path = install_dir.join(version).join("package.nupkg");
        if let Some(parent) = zip_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", display_path(parent)))?;
        }

        let package_url = format!(
            "https://www.nuget.org/api/v2/package/{package_id}/{version}"
        );
        let client = http_client_with_proxy(proxy_url)?;
        let mut response = client
            .get(&package_url)
            .send()
            .with_context(|| format!("requesting native package from {package_url}"))?;

        if !response.status().is_success() {
            continue;
        }

        let mut out_file = fs::File::create(&zip_path)
            .with_context(|| format!("creating {}", display_path(&zip_path)))?;
        io::copy(&mut response, &mut out_file)
            .with_context(|| format!("writing {}", display_path(&zip_path)))?;
        out_file.flush().context("flushing native package to disk")?;

        if extracted_dir.exists() {
            fs::remove_dir_all(&extracted_dir)
                .with_context(|| format!("clearing {}", display_path(&extracted_dir)))?;
        }
        fs::create_dir_all(&extracted_dir)
            .with_context(|| format!("creating {}", display_path(&extracted_dir)))?;

        extract_zip(&zip_path, &extracted_dir)?;

        if find_native_library(&extracted_dir, "libSDL3.so.0").is_some()
            || find_native_library(&extracted_dir, "libSDL3.so").is_some()
            || find_native_library(&extracted_dir, "SDL3.so").is_some()
        {
            return Ok(extracted_dir);
        }
    }

    anyhow::bail!("failed to stage {package_id} from any known version")
}

fn find_native_library(root: &Path, library_name: &str) -> Option<PathBuf> {
    let mut preferred: Option<PathBuf> = None;
    let mut fallback: Option<PathBuf> = None;
    let host_rid = host_rid();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let Some(name) = entry.path().file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        if name == library_name {
            let path = entry.path().to_path_buf();
            if preferred.is_none() && path.to_string_lossy().contains(host_rid) {
                preferred = Some(path);
            } else if fallback.is_none() {
                fallback = Some(path);
            }
        }
    }

    preferred.or(fallback)
}

fn host_rid() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win-x64",
        ("windows", "aarch64") => "win-arm64",
        ("macos", "x86_64") => "osx-x64",
        ("macos", "aarch64") => "osx-arm64",
        _ => "linux-x64",
    }
}

fn create_sdl3_aliases(base_dir: &Path, target: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let aliases = ["SDL3.so", "libSDL3.so", "SDL3", "libSDL3"];

        for alias in aliases {
            let link = base_dir.join(alias);
            if fs::symlink_metadata(&link).is_ok() {
                continue;
            }

            std::os::unix::fs::symlink(target, &link)
                .with_context(|| format!("creating SDL3 alias {}", display_path(&link)))?;
        }
    }

    Ok(())
}

fn detect_client_executable(extracted_dir: &Path) -> Result<PathBuf> {
    let candidates = [
        "Robust.Client.dll",
        "Robust.Client",
        "Robust.Client.exe",
    ];

    for candidate in candidates {
        for entry in WalkDir::new(extracted_dir).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }

            let file_name = entry.file_name().to_string_lossy();
            if file_name.eq_ignore_ascii_case(candidate) {
                return Ok(entry.path().to_path_buf());
            }
        }
    }

    anyhow::bail!("no known client executable found after extraction")
}

fn normalize_signature(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let cleaned = cleaned.trim();
    if cleaned.len() % 2 != 0 {
        return None;
    }
    if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(cleaned.to_ascii_lowercase())
}

fn current_rid_candidates() -> Vec<&'static str> {    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => vec!["linux-x64"],
        ("linux", "aarch64") => vec!["linux-arm64", "linux-x64"],
        ("windows", "x86_64") => vec!["win-x64"],
        ("windows", "aarch64") => vec!["win-arm64", "win-x64"],
        ("macos", "x86_64") => vec!["osx-x64"],
        ("macos", "aarch64") => vec!["osx-arm64", "osx-x64"],
        _ => vec!["linux-x64", "win-x64", "osx-x64"],
    }
}

fn fetch_module_zip_url(
    client: &reqwest::blocking::Client,
    module_name: &str,
    engine_version: &str,
    rid_candidates: &[&str],
) -> Result<(String, String)> {
    let mut last_error: Option<anyhow::Error> = None;

    for manifest_url in ROBUST_MODULES_MANIFEST_URLS {
        let response = match client.get(manifest_url).send() {
            Ok(resp) => resp,
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        if !response.status().is_success() {
            last_error = Some(anyhow::anyhow!(
                "robust modules manifest responded with {} ({manifest_url})",
                response.status()
            ));
            continue;
        }

        let manifest = match response.json::<RobustModulesManifest>() {
            Ok(v) => v,
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        let Some(module_entry) = manifest.modules.get(module_name) else {
            last_error = Some(anyhow::anyhow!("module {module_name} not found in manifest"));
            continue;
        };

        let mut best: Option<(Vec<u32>, String, String)> = None;
        let engine_cmp = version_cmp_key(engine_version);

        for (version, version_entry) in &module_entry.versions {
            let mut selected_url: Option<String> = None;
            for rid in rid_candidates {
                if let Some(platform) = version_entry.platforms.get(*rid) {
                    selected_url = Some(platform.url.clone());
                    break;
                }
            }
            let Some(url) = selected_url else {
                continue;
            };

            let version_key = version_cmp_key(version);
            if let (Some(ev), Some(vk)) = (&engine_cmp, &version_key) {
                if vk > ev {
                    continue;
                }
            }

            let key_for_sort = version_key.unwrap_or_else(|| vec![0]);
            match &best {
                Some((best_key, _, _)) if key_for_sort <= *best_key => {}
                _ => best = Some((key_for_sort, version.clone(), url)),
            }
        }

        if let Some((_, version, url)) = best {
            return Ok((version, url));
        }

        last_error = Some(anyhow::anyhow!(
            "no compatible {module_name} version found for engine {engine_version} and {:?}",
            rid_candidates
        ));
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("failed to resolve module URL")))
}

fn version_cmp_key(raw: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    let mut any = false;

    for part in raw.split('.') {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            break;
        }

        if let Ok(v) = digits.parse::<u32>() {
            any = true;
            out.push(v);
        } else {
            break;
        }
    }

    if any { Some(out) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rid() -> Vec<&'static str> {
        current_rid_candidates()
    }

    #[test]
    fn resolve_redirect_follows_chain() {
        let manifest = RobustBuildManifest(
            [
                (
                    "v1".to_string(),
                    RobustBuildVersionEntry {
                        redirect: Some("v2".to_string()),
                        platforms: Default::default(),
                    },
                ),
                (
                    "v2".to_string(),
                    RobustBuildVersionEntry {
                        redirect: Some("v3".to_string()),
                        platforms: Default::default(),
                    },
                ),
                (
                    "v3".to_string(),
                    RobustBuildVersionEntry {
                        redirect: None,
                        platforms: Default::default(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        );

        let (resolved, _) = resolve_redirect(&manifest, "v1").expect("chain resolves");
        assert_eq!(resolved, "v3");
    }

    #[test]
    fn resolve_redirect_self_stops() {
        let manifest = RobustBuildManifest(
            [(
                "self".to_string(),
                RobustBuildVersionEntry {
                    redirect: Some("self".to_string()),
                    platforms: Default::default(),
                },
            )]
            .into_iter()
            .collect(),
        );
        let (resolved, _) = resolve_redirect(&manifest, "self").expect("self resolves");
        assert_eq!(resolved, "self");
    }

    #[test]
    fn resolve_redirect_missing_returns_none() {
        let manifest = RobustBuildManifest(Default::default());
        assert!(resolve_redirect(&manifest, "nope").is_none());
    }

    #[test]
    fn manifest_cache_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ss14-manifest-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let paths = LauncherPaths {
            user_data_dir: dir.clone(),
            local_data_dir: dir.clone(),
            logs_dir: dir.join("logs"),
            clients_dir: dir.clone(),
            extensions_dir: dir.join("extensions"),
            config_path: dir.join("config.toml"),
        };

        let cache = paths.clients_dir.join("robust-manifest.json");
        let rid_name = rid()[0];
        let synthetic = serde_json::json!({
            "myserver-fork": {
                "redirect": null,
                "platforms": {
                    rid_name: {
                        "url": "https://example.com/engine.zip",
                        "sig": "ab"
                    }
                }
            }
        });
        fs::write(&cache, serde_json::to_vec(&synthetic).unwrap()).unwrap();

        let bytes = fs::read(&cache).unwrap();
        let manifest: RobustBuildManifest = serde_json::from_slice(&bytes).unwrap();
        let (resolved, _) = resolve_redirect(&manifest, "myserver-fork").unwrap();
        assert_eq!(resolved, "myserver-fork");

        let _ = fs::remove_dir_all(&dir);
    }
}

#[derive(Debug, Deserialize)]
struct RobustBuildManifest(std::collections::HashMap<String, RobustBuildVersionEntry>);

#[derive(Debug, Deserialize)]
struct RobustBuildVersionEntry {
    #[serde(default)]
    redirect: Option<String>,
    platforms: std::collections::HashMap<String, RobustBuildPlatformEntry>,
}

fn resolve_redirect<'a>(
    manifest: &'a RobustBuildManifest,
    version: &str,
) -> Option<(String, &'a RobustBuildVersionEntry)> {
    let mut current = version.to_string();
    for _ in 0..16 {
        let entry = manifest.0.get(&current)?;
        match entry.redirect.as_deref() {
            None => return Some((current, entry)),
            Some(next) if next.is_empty() || next == current => return Some((current, entry)),
            Some(next) => current = next.to_string(),
        }
    }
    manifest
        .0
        .get(&current)
        .map(|entry| (current, entry))
}

#[derive(Debug, Deserialize)]
struct RobustBuildPlatformEntry {
    url: String,
    #[serde(default, rename = "sig")]
    signature: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RobustModulesManifest {
    modules: std::collections::HashMap<String, RobustModuleEntry>,
}

#[derive(Debug, Deserialize)]
struct RobustModuleEntry {
    versions: std::collections::HashMap<String, RobustModuleVersionEntry>,
}

#[derive(Debug, Deserialize)]
struct RobustModuleVersionEntry {
    platforms: std::collections::HashMap<String, RobustBuildPlatformEntry>,
}

fn build_target_id(server_address: &str, build: &ServerBuildInformation) -> String {
    let fork = build
        .fork_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("unknown-fork");
    let version = build
        .version
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("unknown-version");

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
        let fallback = server_address
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        return fallback;
    }

    id
}
