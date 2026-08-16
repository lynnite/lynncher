use std::fs;
use std::path::PathBuf;

use std::process::{Child, Command};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use super::accounts;
use super::uri;
use super::{
    display_path, launcher_paths, normalize_base_url, stage_sdl3_native_runtime, LauncherConfig,
    ServerInfo,
};

pub fn launch_game_with_context(
    cfg: &LauncherConfig,
    selected_server: Option<&str>,
    server_info: Option<&ServerInfo>,
) -> Result<Child> {
    if cfg.game_executable.trim().is_empty() {
        anyhow::bail!("Game executable path is empty");
    }

    let executable = PathBuf::from(cfg.game_executable.trim());
    if !executable.exists() {
        anyhow::bail!(
            "Game executable does not exist: {}",
            display_path(&executable)
        );
    }

    let is_managed_dll = executable
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("dll"))
        .unwrap_or(false);

    if is_managed_dll {
        let proxy = if cfg.proxy_enabled && !cfg.proxy_url.trim().is_empty() {
            Some(cfg.proxy_url.trim())
        } else {
            None
        };

        if let Some(exe_dir) = executable.parent() {
            let _ = stage_sdl3_native_runtime(&launcher_paths(), exe_dir, proxy);
        }
    }

    let mut command = if is_managed_dll {
        let mut cmd = Command::new("dotnet");
        cmd.arg(&executable);
        cmd
    } else {
        Command::new(&executable)
    };

    if let Some(parent) = executable.parent() {
        command.current_dir(parent);

        if is_managed_dll {
            #[cfg(target_os = "linux")]
            {
                command.arg("--cvar");
                command.arg("display.windowing_api=sdl3");
            }

            let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
            let mut merged_parts = vec![parent.to_string_lossy().to_string()];

            for (key, value) in std::env::vars() {
                if !key.starts_with("ROBUST_MODULE_") {
                    continue;
                }

                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }

                merged_parts.push(trimmed.to_string());
            }

            let mut merged = merged_parts.join(":");
            if !existing.trim().is_empty() {
                merged.push(':');
                merged.push_str(existing.trim());
            }
            command.env("LD_LIBRARY_PATH", merged);
        }
    }

    if !cfg.connect_uri.trim().is_empty() {
        command.arg(cfg.connect_uri.trim());
    } else if let Some(server) = selected_server {
        if let Some(info) = server_info {
            if let Ok(connect_addr) = uri::derive_connect_address(server, info) {
                command.arg("--launcher");
                command.arg("--connect-address");
                command.arg(connect_addr);
                command.arg("--ss14-address");
                command.arg(server);
            } else {
                command.arg(server);
            }
        } else {
            command.arg(server);
        }
    }

    if !cfg.extra_args.trim().is_empty() {
        let split_args = shell_words::split(cfg.extra_args.trim())
            .context("parsing extra arguments failed")?;
        command.args(split_args);
    }

    if let Some(info) = server_info {
        if let Some(build) = &info.build {
            push_build_cvar(&mut command, "download_url", build.download_url.as_deref());
            push_build_cvar(&mut command, "manifest_url", build.manifest_url.as_deref());
            push_build_cvar(
                &mut command,
                "manifest_download_url",
                build.manifest_download_url.as_deref(),
            );
            push_build_cvar(&mut command, "version", build.version.as_deref());
            push_build_cvar(&mut command, "fork_id", build.fork_id.as_deref());
            push_build_cvar(&mut command, "hash", build.hash.as_deref());
            push_build_cvar(&mut command, "manifest_hash", build.manifest_hash.as_deref());
            push_build_cvar(&mut command, "engine_version", build.engine_version.as_deref());
        }
    }

    if let Some(account) = accounts::active_account_for_auth(cfg, &cfg.auth_server_url) {
        if let Some(info) = server_info {
            if !accounts::auth_mode_disabled(&info.auth) {
                command.env("ROBUST_AUTH_TOKEN", account.token.trim());
                command.env("ROBUST_AUTH_USERID", account.user_id.trim());
                command.env("ROBUST_AUTH_SERVER", normalize_base_url(&account.auth_server));
                if let Some(pub_key) = &info.auth.public_key {
                    if !pub_key.trim().is_empty() {
                        command.env("ROBUST_AUTH_PUBKEY", pub_key.trim());
                    }
                }
            }
        }
    }

    if !is_managed_dll {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&executable)?;
            let mut perms = metadata.permissions();
            let mode = perms.mode();
            if mode & 0o100 == 0 {
                perms.set_mode(mode | 0o755);
                fs::set_permissions(&executable, perms).with_context(|| {
                    format!("setting executable bit on {}", display_path(executable.as_path()))
                })?;
            }
        }
    }

    if is_managed_dll {
        ensure_sdl3_linux_aliases(executable.parent())?;
        ensure_glfw_linux_aliases(executable.parent())?;
        ensure_glfw_module_aliases(executable.parent())?;
    }

    let child = command.spawn().with_context(|| {
        format!(
            "failed to launch executable {}",
            display_path(executable.as_path())
        )
    })?;

    Ok(child)
}

fn push_build_cvar(command: &mut Command, name: &str, value: Option<&str>) {
    let Some(raw) = value else {
        return;
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }

    command.arg("--cvar");
    command.arg(format!("build.{name}={trimmed}"));
}

fn ensure_glfw_linux_aliases(base_dir: Option<&std::path::Path>) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let Some(base_dir) = base_dir else {
            return Ok(());
        };

        let Some(target) = find_glfw_shared_library() else {
            return Ok(());
        };

        let aliases = [
            "glfw3.dll.so",
            "libglfw3.dll.so",
            "glfw3.dll",
            "libglfw3.dll",
        ];

        for alias in aliases {
            let link = base_dir.join(alias);
            if fs::symlink_metadata(&link).is_ok() {
                continue;
            }

            std::os::unix::fs::symlink(&target, &link)
                .with_context(|| format!("creating GLFW alias {}", display_path(&link)))?;
        }
    }

    Ok(())
}

fn ensure_glfw_module_aliases(base_dir: Option<&std::path::Path>) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let Some(base_dir) = base_dir else {
            return Ok(());
        };

        let mut found: Option<PathBuf> = None;

        for (key, value) in std::env::vars() {
            if !key.starts_with("ROBUST_MODULE_") {
                continue;
            }

            let module_path = PathBuf::from(value.trim());
            if !module_path.exists() {
                continue;
            }

            for entry in WalkDir::new(&module_path)
                .follow_links(true)
                .max_depth(6)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if !entry.file_type().is_file() {
                    continue;
                }

                let Some(name) = entry.path().file_name().and_then(|s| s.to_str()) else {
                    continue;
                };

                if name.starts_with("libglfw") || name.starts_with("glfw3") {
                    found = Some(entry.path().to_path_buf());
                    break;
                }
            }

            if found.is_some() {
                break;
            }
        }

        let Some(target) = found else {
            return Ok(());
        };

        let aliases = [
            "glfw3.dll.so",
            "libglfw3.dll.so",
            "glfw3.dll",
            "libglfw3.dll",
        ];

        for alias in aliases {
            let link = base_dir.join(alias);
            if fs::symlink_metadata(&link).is_ok() {
                continue;
            }

            std::os::unix::fs::symlink(&target, &link)
                .with_context(|| format!("creating GLFW module alias {}", display_path(&link)))?;
        }
    }

    Ok(())
}

fn ensure_sdl3_linux_aliases(base_dir: Option<&std::path::Path>) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let Some(base_dir) = base_dir else {
            return Ok(());
        };

        let Some(target) = find_sdl3_shared_library(Some(base_dir)) else {
            return Ok(());
        };

        let aliases = ["SDL3.so", "libSDL3.so", "SDL3", "libSDL3"];

        for alias in aliases {
            let link = base_dir.join(alias);
            if fs::symlink_metadata(&link).is_ok() {
                continue;
            }

            std::os::unix::fs::symlink(&target, &link)
                .with_context(|| format!("creating SDL3 alias {}", display_path(&link)))?;
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn find_glfw_shared_library() -> Option<PathBuf> {
    let exact = [
        "/usr/lib/x86_64-linux-gnu/libglfw.so.3",
        "/lib/x86_64-linux-gnu/libglfw.so.3",
        "/run/current-system/sw/lib/libglfw.so.3",
        "/usr/lib64/libglfw.so.3",
        "/lib64/libglfw.so.3",
        "/usr/lib/libglfw.so.3",
        "/lib/libglfw.so.3",
    ];

    for p in exact {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/usr/lib"),
        PathBuf::from("/usr/lib64"),
        PathBuf::from("/usr/local/lib"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
        PathBuf::from("/lib/x86_64-linux-gnu"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu"),
        PathBuf::from("/run/current-system/sw/lib"),
        PathBuf::from("/app/lib"),
        PathBuf::from("/app/lib/x86_64-linux-gnu"),
    ];

    if let Ok(ld_library_path) = std::env::var("LD_LIBRARY_PATH") {
        for part in ld_library_path.split(':') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                roots.push(PathBuf::from(trimmed));
            }
        }
    }

    for root in &roots {
        let entries = match fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            let name = match p.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if name.starts_with("libglfw.so") && p.is_file() {
                return Some(p);
            }
        }
    }

    for root in &roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .follow_links(true)
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let name = match entry.path().file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if name.starts_with("libglfw.so") {
                return Some(entry.path().to_path_buf());
            }
        }
    }

    if let Ok(entries) = fs::read_dir("/nix/store") {
        for entry in entries.filter_map(|e| e.ok()) {
            let lib_dir = entry.path().join("lib");
            let read = match fs::read_dir(&lib_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for sub in read.filter_map(|e| e.ok()) {
                let p = sub.path();
                let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };

                if name.starts_with("libglfw.so") && p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn find_sdl3_shared_library(base_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    let mut exact = vec![
        PathBuf::from("/usr/lib/x86_64-linux-gnu/libSDL3.so.0"),
        PathBuf::from("/lib/x86_64-linux-gnu/libSDL3.so.0"),
        PathBuf::from("/run/current-system/sw/lib/libSDL3.so.0"),
        PathBuf::from("/usr/lib64/libSDL3.so.0"),
        PathBuf::from("/lib64/libSDL3.so.0"),
        PathBuf::from("/usr/lib/libSDL3.so.0"),
        PathBuf::from("/lib/libSDL3.so.0"),
    ];

    if let Some(base_dir) = base_dir {
        exact.insert(0, base_dir.join("libSDL3.so.0"));
    }

    for p in exact {
        if p.exists() {
            return Some(p);
        }
    }

    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/usr/lib"),
        PathBuf::from("/usr/lib64"),
        PathBuf::from("/usr/local/lib"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
        PathBuf::from("/lib/x86_64-linux-gnu"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu"),
        PathBuf::from("/run/current-system/sw/lib"),
        PathBuf::from("/app/lib"),
        PathBuf::from("/app/lib/x86_64-linux-gnu"),
    ];

    if let Some(base_dir) = base_dir {
        roots.push(base_dir.to_path_buf());
    }

    if let Ok(ld_library_path) = std::env::var("LD_LIBRARY_PATH") {
        for part in ld_library_path.split(':') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                roots.push(PathBuf::from(trimmed));
            }
        }
    }

    for root in &roots {
        let entries = match fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            let name = match p.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if name.starts_with("libSDL3.so") && p.is_file() {
                return Some(p);
            }
        }
    }

    for root in &roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .follow_links(true)
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let name = match entry.path().file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if name.starts_with("libSDL3.so") {
                return Some(entry.path().to_path_buf());
            }
        }
    }

    None
}
