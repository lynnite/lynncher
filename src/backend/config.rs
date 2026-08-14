use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{app_data_name, display_path, LauncherConfig, LauncherPaths, APP_VENDOR_DIR, CONFIG_FILE_NAME};

pub fn launcher_paths() -> LauncherPaths {
    let user_data_base = user_data_base_dir();
    let user_data_dir = user_data_base.join(APP_VENDOR_DIR).join(app_data_name());

    let local_data_dir = local_data_base_dir().join(APP_VENDOR_DIR).join(app_data_name());
    let logs_dir = user_data_dir.join("logs");
    let clients_dir = user_data_dir.join("clients");
    let extensions_dir = user_data_dir.join("extensions");
    let config_path = local_data_dir.join(CONFIG_FILE_NAME);

    LauncherPaths {
        user_data_dir,
        local_data_dir,
        logs_dir,
        clients_dir,
        extensions_dir,
        config_path,
    }
}

pub fn ensure_dirs(paths: &LauncherPaths) -> Result<()> {
    fs::create_dir_all(&paths.user_data_dir)
        .with_context(|| format!("creating {}", display_path(&paths.user_data_dir)))?;
    fs::create_dir_all(&paths.local_data_dir)
        .with_context(|| format!("creating {}", display_path(&paths.local_data_dir)))?;
    fs::create_dir_all(&paths.logs_dir)
        .with_context(|| format!("creating {}", display_path(&paths.logs_dir)))?;
    fs::create_dir_all(&paths.clients_dir)
        .with_context(|| format!("creating {}", display_path(&paths.clients_dir)))?;
    fs::create_dir_all(&paths.extensions_dir)
        .with_context(|| format!("creating {}", display_path(&paths.extensions_dir)))?;
    Ok(())
}

pub fn load_config(paths: &LauncherPaths) -> Result<LauncherConfig> {
    if !paths.config_path.exists() {
        return Ok(LauncherConfig::default());
    }

    let raw = fs::read_to_string(&paths.config_path)
        .with_context(|| format!("reading {}", display_path(&paths.config_path)))?;

    toml::from_str(&raw).with_context(|| format!("parsing {}", display_path(&paths.config_path)))
}

pub fn save_config(paths: &LauncherPaths, cfg: &LauncherConfig) -> Result<()> {
    let raw = toml::to_string_pretty(cfg).context("serializing config")?;
    fs::write(&paths.config_path, raw)
        .with_context(|| format!("writing {}", display_path(&paths.config_path)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&paths.config_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&paths.config_path, perms);
        }
    }

    Ok(())
}

pub fn load_config_from_path(path: &PathBuf) -> Result<LauncherConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading {}", display_path(path)))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", display_path(path)))
}

pub(crate) fn user_data_base_dir() -> PathBuf {
    if cfg!(target_os = "linux") {
        if let Ok(path) = env::var("XDG_DATA_HOME") {
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }

        return home_dir_fallback().join(".local").join("share");
    }

    if cfg!(target_os = "macos") {
        return home_dir_fallback()
            .join("Library")
            .join("Application Support");
    }

    if cfg!(target_os = "windows") {
        if let Ok(path) = env::var("APPDATA") {
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    }

    home_dir_fallback()
}

pub(crate) fn local_data_base_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Ok(path) = env::var("LOCALAPPDATA") {
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    }

    user_data_base_dir()
}

pub(crate) fn home_dir_fallback() -> PathBuf {
    if let Ok(path) = env::var("HOME") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    PathBuf::from(".")
}
