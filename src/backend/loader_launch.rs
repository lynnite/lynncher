
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use super::display_path;

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
    }

    command.spawn().with_context(|| {
        format!(
            "failed to launch loader {}",
            display_path(spec.loader_exe.as_path())
        )
    })
}
