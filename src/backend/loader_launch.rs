use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use super::display_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsCapCvar {
    DisplayMaxFps,
    FpsCapMax,
    Unsupported,
}

pub fn detect_fps_cap_cvar(module_dirs: &[PathBuf], engine_zip: Option<&Path>) -> FpsCapCvar {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in module_dirs {
        candidates.push(dir.join("Robust.Shared.dll"));
    }
    if let Some(zip) = engine_zip {
        if let Some(parent) = zip.parent() {
            candidates.push(parent.join("extracted").join("Robust.Shared.dll"));
            candidates.push(parent.join("Robust.Shared.dll"));
        }
    }

    let assemblies: Vec<Vec<u8>> = candidates
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .collect();

    if assemblies.is_empty() {
        return FpsCapCvar::Unsupported;
    }

    let declares = |name: &str| assemblies.iter().any(|bytes| dll_contains_cvar(bytes, name));

    if declares("display.max_fps") {
        FpsCapCvar::DisplayMaxFps
    } else if declares("display.fps_cap_max") {
        FpsCapCvar::FpsCapMax
    } else {
        FpsCapCvar::Unsupported
    }
}

fn dll_contains_cvar(bytes: &[u8], cvar: &str) -> bool {
    let ascii = cvar.as_bytes();
    let mut utf16: Vec<u8> = Vec::with_capacity(ascii.len() * 2);
    for &b in ascii {
        utf16.push(b);
        utf16.push(0);
    }
    contains_subslice(bytes, &utf16) || contains_subslice(bytes, ascii)
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(target_os = "linux")]
static FPS_SHIM_BYTES: &[u8] = include_bytes!("../assets/fpscap/liblynncher_fpscap.so");

#[cfg(target_os = "linux")]
fn safe_shim_dir() -> PathBuf {
    let path_has_space = |p: &Path| p.to_string_lossy().contains(char::is_whitespace);

    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
        if cache.is_absolute() && !path_has_space(&cache) {
            return cache.join("lynncher-fpscap");
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if home.is_absolute() {
            let cache = home.join(".cache").join("lynncher-fpscap");
            if !path_has_space(&cache) {
                return cache;
            }
        }
    }

    let tmp = std::env::temp_dir();
    if !path_has_space(&tmp) {
        return tmp.join("lynncher-fpscap");
    }

    PathBuf::from("/tmp/lynncher-fpscap")
}

#[cfg(target_os = "linux")]
pub(crate) fn stage_fps_shim(_hint_dir: &Path) -> Result<PathBuf> {
    let dir = safe_shim_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating fps shim dir {}", display_path(&dir)))?;
    let path = dir.join("liblynncher_fpscap.so");

    if path.to_string_lossy().contains(char::is_whitespace) {
        anyhow::bail!(
            "no space-free directory available to stage the fps shim (got {})",
            display_path(&path)
        );
    }

    let needs_write = match std::fs::read(&path) {
        Ok(existing) => existing != FPS_SHIM_BYTES,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&path, FPS_SHIM_BYTES)
            .with_context(|| format!("writing fps shim {}", display_path(&path)))?;
    }
    Ok(path)
}

fn valid_signature_hex(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let cleaned = cleaned.trim();
    if cleaned.len() % 2 != 0 {
        return String::new();
    }
    if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return String::new();
    }
    cleaned.to_ascii_lowercase()
}

pub struct LoaderLaunchSpec {
    pub engine_zip: PathBuf,
    pub engine_signature: String,
    pub signing_key: PathBuf,
    pub loader_exe: PathBuf,
    pub content_db: PathBuf,
    pub content_version_id: i64,
    pub overlay_zip: Option<PathBuf>,
    pub modules: Vec<(String, PathBuf)>,
    pub connect_uri: Option<String>,
    pub connect_ss14_address: Option<String>,
    pub build_cvars: Vec<(String, String)>,
    pub username: Option<String>,
    pub compat_mode: bool,
    pub extra_args: Vec<String>,
    pub auth_token: Option<String>,
    pub auth_userid: Option<String>,
    pub auth_server: Option<String>,
    pub auth_pubkey: Option<String>,
    pub disable_signing: bool,
    pub forward_cursor_theme: bool,
    pub cursor_theme: Option<String>,
    pub fps_limit: Option<u32>,
    pub fps_shim_dir: Option<PathBuf>,
}

pub fn launch_game_via_loader(spec: &LoaderLaunchSpec) -> Result<std::process::Child> {
    let signature_hex = valid_signature_hex(&spec.engine_signature);
    let disable_signing = spec.disable_signing || signature_hex.is_empty();

    eprintln!(
        "[loader-launch] engine_signature chars={}, signature_hex chars={}, disable_signing={}",
        spec.engine_signature.chars().count(),
        signature_hex.chars().count(),
        disable_signing
    );

    let mut command = Command::new(&spec.loader_exe);
    if let Some(parent) = spec.loader_exe.parent() {
        command.current_dir(parent);
    }

    if spec.forward_cursor_theme {
        super::apply_cursor_env_with_override(&mut command, spec.cursor_theme.as_deref());
    }

    command.arg(spec.engine_zip.as_os_str());
    command.arg(&signature_hex);
    command.arg(spec.signing_key.as_os_str());

    command.env("SS14_LOADER_CONTENT_DB", &spec.content_db);
    command.env(
        "SS14_LOADER_CONTENT_VERSION",
        spec.content_version_id.to_string(),
    );
    if let Some(overlay) = &spec.overlay_zip {
        command.env("SS14_LOADER_OVERLAY_ZIP", overlay);
    }

    for (module_name, module_path) in &spec.modules {
        let safe = module_name.to_uppercase().replace('.', "_");
        command.env(format!("ROBUST_MODULE_{safe}"), module_path);
    }

    if disable_signing {
        command.env("SS14_DISABLE_SIGNING", "true");
    }

    command.env("DOTNET_TieredPGO", "1");
    command.env("DOTNET_ReadyToRun", "0");
    command.env("DOTNET_MULTILEVEL_LOOKUP", "0");

    if let Some(token) = &spec.auth_token {
        command.env("ROBUST_AUTH_TOKEN", token);
    }
    if let Some(uid) = &spec.auth_userid {
        command.env("ROBUST_AUTH_USERID", uid);
    }
    if let Some(srv) = &spec.auth_server {
        command.env("ROBUST_AUTH_SERVER", srv);
    }
    if let Some(key) = &spec.auth_pubkey {
        command.env("ROBUST_AUTH_PUBKEY", key);
    }

    let username = spec
        .username
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("UnknownPlayer");
    command.arg("--username");
    command.arg(username);
    command.arg("--cvar");
    command.arg(format!("display.compat={}", spec.compat_mode));
    command.arg("--cvar");
    command.arg("launch.launcher=true");

    let mut fps_shim_path: Option<PathBuf> = None;
    if let Some(fps) = spec.fps_limit {
        if fps > 0 {
            let module_dirs: Vec<PathBuf> =
                spec.modules.iter().map(|(_, p)| p.clone()).collect();
            match detect_fps_cap_cvar(&module_dirs, Some(&spec.engine_zip)) {
                FpsCapCvar::DisplayMaxFps => {
                    command.arg("--cvar");
                    command.arg("display.vsync=false");
                    command.arg("--cvar");
                    command.arg(format!("display.max_fps={fps}"));
                }
                FpsCapCvar::FpsCapMax => {
                    command.arg("--cvar");
                    command.arg("display.vsync=false");
                    command.arg("--cvar");
                    command.arg(format!("display.fps_cap_max={fps}"));
                }
                FpsCapCvar::Unsupported => {
                    #[cfg(target_os = "linux")]
                    {
                        command.arg("--cvar");
                        command.arg("display.vsync=false");
                        match spec.fps_shim_dir.as_deref() {
                            Some(dir) => match stage_fps_shim(dir) {
                                Ok(path) => {
                                    command.env("LYNNCHER_FPS_CAP", fps.to_string());
                                    command.env(
                                        "LYNNCHER_FPS_LOG",
                                        path.with_file_name("fpscap.log"),
                                    );
                                    fps_shim_path = Some(path);
                                    eprintln!(
                                        "[loader-launch] engine lacks a numeric FPS cap; \
                                         applying external frame cap of {fps} fps via LD_PRELOAD shim"
                                    );
                                }
                                Err(err) => {
                                    eprintln!(
                                        "[loader-launch] failed to stage fps shim: {err:#}"
                                    );
                                }
                            },
                            None => {
                                eprintln!(
                                    "[loader-launch] engine lacks a numeric FPS cap and no \
                                     shim dir was provided; cap {fps} not applied"
                                );
                            }
                        }
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        eprintln!(
                            "[loader-launch] engine declares no numeric FPS cap cvar \
                             (display.max_fps/display.fps_cap_max); a cap of {fps} cannot be applied"
                        );
                    }
                }
            }
        }
    }

    for (name, value) in &spec.build_cvars {
        command.arg("--cvar");
        command.arg(format!("build.{name}={value}"));
    }

    if let Some(uri) = &spec.connect_uri {
        command.arg("--launcher");
        command.arg("--connect-address");
        command.arg(uri);
    }
    if let Some(addr) = &spec.connect_ss14_address {
        command.arg("--ss14-address");
        command.arg(addr);
    }
    for arg in &spec.extra_args {
        command.arg(arg);
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = spec.loader_exe.parent() {
            let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
            let mut parts = vec![parent.to_string_lossy().to_string()];
            for (_, module_path) in &spec.modules {
                parts.push(module_path.to_string_lossy().to_string());
            }
            let mut merged = parts.join(":");
            if !existing.trim().is_empty() {
                merged.push(':');
                merged.push_str(existing.trim());
            }
            command.env("LD_LIBRARY_PATH", merged);
        }

        if let Some(shim) = &fps_shim_path {
            let existing = std::env::var("LD_PRELOAD").unwrap_or_default();
            let mut preload = shim.to_string_lossy().to_string();
            if !existing.trim().is_empty() {
                preload.push(':');
                preload.push_str(existing.trim());
            }
            command.env("LD_PRELOAD", preload);
        }
    }

    #[cfg(not(target_os = "linux"))]
    let _ = &fps_shim_path;

    command.spawn().with_context(|| {
        format!(
            "failed to launch loader {}",
            display_path(spec.loader_exe.as_path())
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for c in s.encode_utf16() {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out
    }

    #[test]
    fn detects_display_max_fps_utf16() {
        let mut bytes = b"header".to_vec();
        bytes.extend_from_slice(&utf16le("display.max_fps"));
        bytes.extend_from_slice(b"trailer");
        assert!(dll_contains_cvar(&bytes, "display.max_fps"));
    }

    #[test]
    fn detects_display_max_fps_ascii() {
        let mut bytes = b"junk".to_vec();
        bytes.extend_from_slice(b"display.max_fps");
        assert!(dll_contains_cvar(&bytes, "display.max_fps"));
    }

    #[test]
    fn does_not_confuse_fps_cap_max() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&utf16le("display.fps_cap_max"));
        assert!(!dll_contains_cvar(&bytes, "display.max_fps"));
        assert!(dll_contains_cvar(&bytes, "display.fps_cap_max"));
    }

    #[test]
    fn absent_cvar_not_detected() {
        let mut bytes = b"junk".to_vec();
        bytes.extend_from_slice(&utf16le("display.vsync"));
        assert!(!dll_contains_cvar(&bytes, "display.max_fps"));
        assert!(!dll_contains_cvar(&bytes, "display.fps_cap_max"));
    }

    #[test]
    fn empty_needle_is_false() {
        assert!(!contains_subslice(b"anything", &[]));
    }
}
