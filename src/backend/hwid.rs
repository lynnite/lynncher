#[cfg(not(target_os = "windows"))]
use std::fs;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

use anyhow::{Context, Result};

#[cfg(not(target_os = "windows"))]
use super::types::APP_VENDOR_DIR;
use super::types::LauncherConfig;

const HWID_LENGTH: usize = 32;
#[cfg(target_os = "windows")]
const REGISTRY_ROOT: &str = r"SOFTWARE\Space Wizards\Robust";

pub fn write_hwid(bytes: &[u8]) -> Result<()> {
    if bytes.len() != HWID_LENGTH {
        anyhow::bail!("HWID must be exactly {HWID_LENGTH} bytes");
    }

    #[cfg(target_os = "windows")]
    {
        write_hwid_registry(bytes)?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        write_hwid_file(bytes)?;
    }

    Ok(())
}

pub fn read_hwid() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        read_hwid_registry()
    }

    #[cfg(not(target_os = "windows"))]
    {
        read_hwid_file()
    }
}

pub fn read_hwid_hex() -> String {
    read_hwid()
        .map(|bytes| hex::encode(bytes))
        .unwrap_or_default()
}

fn random_bytes_32() -> Vec<u8> {
    let mut out = vec![0u8; HWID_LENGTH];
    let _ = getrandom::getrandom(&mut out);
    if out.iter().all(|&b| b == 0) {
        for (i, chunk) in out.chunks_mut(4).enumerate() {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let v = (seed.wrapping_add(i as u128) as u64).to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
    out
}

pub fn randomize_hwid() -> Result<String> {
    let bytes = random_bytes_32();
    write_hwid(&bytes)?;
    Ok(hex::encode(bytes))
}

pub fn set_hwid_hex(raw: &str) -> Result<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();

    if cleaned.len() != HWID_LENGTH * 2 {
        anyhow::bail!(
            "HWID must be exactly {} hex characters, got {}",
            HWID_LENGTH * 2,
            cleaned.len()
        );
    }

    let bytes = hex::decode(&cleaned)
        .context("parsing HWID hex string")?;
    write_hwid(&bytes)?;
    Ok(cleaned.to_ascii_lowercase())
}

pub fn apply_hwid_for_launch(cfg: &LauncherConfig) -> String {
    match cfg.hwid_mode.as_str() {
        "random" => {
            let _ = randomize_hwid();
            read_hwid_hex()
        }
        "custom" if !cfg.hwid_value.trim().is_empty() => {
            match set_hwid_hex(&cfg.hwid_value) {
                Ok(hexstr) => hexstr,
                Err(_) => read_hwid_hex(),
            }
        }
        _ => read_hwid_hex(),
    }
}

#[cfg(not(target_os = "windows"))]
fn hwid_file_path() -> PathBuf {
    let data_root = if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| home_dir().join(".local").join("share"))
    };

    data_root.join(APP_VENDOR_DIR).join(".hwid")
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(target_os = "windows"))]
fn read_hwid_file() -> Option<Vec<u8>> {
    let path = hwid_file_path();
    let bytes = fs::read(&path).ok()?;
    if bytes.len() == HWID_LENGTH {
        Some(bytes)
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn write_hwid_file(bytes: &[u8]) -> Result<()> {
    let path = hwid_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", super::display_path(parent)))?;
    }
    fs::write(&path, bytes)
        .with_context(|| format!("writing {}", super::display_path(&path)))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn read_hwid_registry() -> Option<Vec<u8>> {
    use winreg::RegKey; 
    use winreg::enums::{HKEY_CURRENT_USER, REG_BINARY};

    let root = RegKey::predef(HKEY_CURRENT_USER);
    let key = root.open_subkey(REGISTRY_ROOT).ok()?;

    for name in ["Hwid2", "Hwid"] {
        if let Ok(value) = key.get_raw_value(name) {
            if value.vtype == REG_BINARY && value.bytes.len() == HWID_LENGTH {
                return Some(value.bytes);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn write_hwid_registry(bytes: &[u8]) -> Result<()> {
    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::{HKEY_CURRENT_USER, REG_BINARY};

    let root = RegKey::predef(HKEY_CURRENT_USER);
    let key = root
        .create_subkey(REGISTRY_ROOT)
        .context("opening HWID registry key")?
        .0;

    let value = RegValue {
        bytes: bytes.to_vec(),
        vtype: REG_BINARY,
    };
    key.set_raw_value("Hwid", &value)
        .context("writing 'Hwid' registry value")?;
    key.set_raw_value("Hwid2", &value)
        .context("writing 'Hwid2' registry value")?;

    Ok(())
}
