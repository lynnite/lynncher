
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::http::http_client_with_proxy;
use super::{display_path, LauncherPaths};

const LAUNCHER_RELEASES_URL: &str =
    "https://api.github.com/repos/space-wizards/SS14.Launcher/releases/latest";

pub const PATCH_FRAMEWORK_ASSEMBLY: &str = "Lynncher.Patches.dll";

pub const PATCH_HARMONY_ASSEMBLY: &str = "0Harmony.dll";

fn stage_patch_framework(loader_dir: &Path) {
    if loader_dir.join(PATCH_FRAMEWORK_ASSEMBLY).is_file()
        && loader_dir.join(PATCH_HARMONY_ASSEMBLY).is_file()
    {
        return;
    }

    let Some(src_dir) = bundled_loader_src_dir() else {
        return;
    };
    let src_loader = src_dir.join("loader");

    for name in [PATCH_FRAMEWORK_ASSEMBLY, PATCH_HARMONY_ASSEMBLY] {
        let src = src_loader.join(name);
        let dst = loader_dir.join(name);
        if src.is_file() && !dst.is_file() {
            let _ = fs::copy(&src, &dst);
        }
    }
}

fn stage_patch_capable_loader(loader_dir: &Path, loader_exe: &Path) {
    if loader_supports_patching(loader_exe) {
        return;
    }

    let Some(src_dir) = bundled_loader_src_dir() else {
        return;
    };
    let src_loader = src_dir.join("loader");

    let bundled_exe = src_loader.join(
        loader_exe
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("SS14.Loader")),
    );
    if !loader_supports_patching(&bundled_exe) {
        return;
    }

    for name in [
        "SS14.Loader.dll",
        "SS14.Loader.deps.json",
        "SS14.Loader.runtimeconfig.json",
    ] {
        let src = src_loader.join(name);
        if src.is_file() {
            let _ = fs::copy(&src, loader_dir.join(name));
        }
    }

    if bundled_exe.is_file() {
        let _ = fs::copy(&bundled_exe, loader_exe);
        fix_loader_permissions(loader_exe);
    }
}

pub struct LoaderInstall {
    pub loader_exe: PathBuf,
    pub signing_key: PathBuf,
}

impl LoaderInstall {
    pub fn supports_patching(&self) -> bool {
        loader_supports_patching(&self.loader_exe)
    }
}

pub fn loader_supports_patching(loader_exe: &Path) -> bool {
    let Some(dir) = loader_exe.parent() else {
        return false;
    };

    if !dir.join(PATCH_FRAMEWORK_ASSEMBLY).is_file() {
        return false;
    }

    let mut candidates: Vec<PathBuf> = vec![loader_exe.to_path_buf()];
    if let Some(name) = loader_exe.file_name() {
        let mut managed_name = name.to_os_string();
        if let Some(stem) = managed_name.to_str().and_then(|n| n.strip_suffix(".exe")) {
            managed_name = stem.into();
        }
        managed_name.push(".dll");
        let managed = dir.join(managed_name);
        if managed != loader_exe {
            candidates.push(managed);
        }
    }

    const NEEDLE: &[u8] =
        b"L\x00y\x00n\x00n\x00c\x00h\x00e\x00r\x00.\x00P\x00a\x00t\x00c\x00h\x00e\x00s\x00";
    const ASCII: &[u8] = b"Lynncher.Patches";

    candidates.iter().any(|path| {
        std::fs::read(path)
            .map(|bytes| byte_contains(&bytes, NEEDLE) || byte_contains(&bytes, ASCII))
            .unwrap_or(false)
    })
}

fn byte_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
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
        stage_patch_capable_loader(&loader_dir, &loader_exe);
        stage_patch_framework(&loader_dir);
        return Ok(LoaderInstall {
            loader_exe,
            signing_key,
        });
    }

    fs::create_dir_all(&install_dir)
        .with_context(|| format!("creating {}", display_path(&install_dir)))?;

    if let Some(bundled) = bundled_loader_src_dir() {
        if copy_bundled_loader(&bundled, &loader_dir, &signing_key).is_ok()
            && loader_exe.exists()
            && signing_key.exists()
        {
            fix_loader_permissions(&loader_exe);
            stage_patch_framework(&loader_dir);
            return Ok(LoaderInstall {
                loader_exe,
                signing_key,
            });
        }
    }

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

    stage_patch_framework(&loader_dir);

    Ok(LoaderInstall {
        loader_exe,
        signing_key,
    })
}

/// Locate the SS14.Loader binaries bundled with this repository, if present.
/// Returns the directory containing the loader files (mirroring the layout of
/// the official release zip, i.e. a `bin/` folder plus a `signing_key` file).
fn bundled_loader_src_dir() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("loader/bin_x64"),
        manifest_dir.join("loader"),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.join("loader/bin_x64")))
            .unwrap_or_default(),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.join("loader")))
            .unwrap_or_default(),
    ];

    for dir in candidates {
        let loader_dir = dir.join("loader");
        let key = dir.join("signing_key");

        let loader_present = if cfg!(target_os = "windows") {
            loader_dir.join("SS14.Loader.exe").exists()
        } else {
            loader_dir.join("SS14.Loader").exists()
        };

        if loader_present && key.exists() {
            return Some(dir);
        }
    }

    None
}

fn copy_bundled_loader(
    src_dir: &Path,
    loader_dir: &Path,
    signing_key: &Path,
) -> std::io::Result<()> {
    if loader_dir.exists() {
        fs::remove_dir_all(loader_dir)?;
    }
    fs::create_dir_all(loader_dir)?;

    let src_loader = src_dir.join("loader");
    for entry in fs::read_dir(&src_loader)? {
        let entry = entry?;
        let target = loader_dir.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }

    fs::copy(src_dir.join("signing_key"), signing_key)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn fix_loader_permissions(loader_exe: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(loader_exe) {
            let mode = meta.permissions().mode();
            if mode & 0o100 == 0 {
                let mut perms = meta.permissions();
                perms.set_mode(mode | 0o755);
                let _ = fs::set_permissions(loader_exe, perms);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("lynncher-loader-test-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_loader(dir: &Path, embed_name: bool) -> PathBuf {
        let exe = dir.join("SS14.Loader");
        let mut bytes = b"MZ".to_vec();
        if embed_name {
            for c in "Lynncher.Patches".encode_utf16() {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        std::fs::write(&exe, bytes).unwrap();
        exe
    }

    #[test]
    fn patching_unsupported_without_framework_dll() {
        let dir = unique_dir("nodll");
        let exe = fake_loader(&dir, true);
        assert!(!loader_supports_patching(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_unsupported_when_loader_lacks_embedded_name() {
        let dir = unique_dir("stock");
        let exe = fake_loader(&dir, false);
        std::fs::write(dir.join(PATCH_FRAMEWORK_ASSEMBLY), b"x").unwrap();
        assert!(!loader_supports_patching(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_supported_when_both_present() {
        let dir = unique_dir("both");
        let exe = fake_loader(&dir, true);
        std::fs::write(dir.join(PATCH_FRAMEWORK_ASSEMBLY), b"x").unwrap();
        assert!(loader_supports_patching(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_supported_when_name_is_in_managed_dll() {
        let dir = unique_dir("manageddll");
        let exe = fake_loader(&dir, false);

        let mut dll_bytes = b"MZ".to_vec();
        for c in "Lynncher.Patches".encode_utf16() {
            dll_bytes.extend_from_slice(&c.to_le_bytes());
        }
        std::fs::write(dir.join("SS14.Loader.dll"), dll_bytes).unwrap();
        std::fs::write(dir.join(PATCH_FRAMEWORK_ASSEMBLY), b"x").unwrap();

        assert!(loader_supports_patching(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_unsupported_for_missing_loader() {
        let dir = unique_dir("missing");
        let exe = dir.join("does-not-exist");
        assert!(!loader_supports_patching(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
