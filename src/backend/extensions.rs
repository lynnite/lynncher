use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::{display_path, LauncherPaths};

pub fn sideload_extension_bundle(paths: &LauncherPaths, source_path: &Path) -> Result<String> {
    if !source_path.exists() {
        anyhow::bail!("extension source file does not exist");
    }

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .context("invalid extension file name")?;

    let target = paths.extensions_dir.join(file_name);
    fs::copy(source_path, &target).with_context(|| {
        format!(
            "copying extension bundle from {} to {}",
            display_path(source_path),
            display_path(&target)
        )
    })?;

    target
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .context("failed to determine copied extension name")
}

pub fn remove_sideloaded_extension(paths: &LauncherPaths, name: &str) -> Result<()> {
    let safe_name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid extension name"))?;
    let target = paths.extensions_dir.join(safe_name);
    if !target.exists() {
        anyhow::bail!("extension not found: {}", safe_name);
    }
    fs::remove_file(&target).with_context(|| {
        format!(
            "removing extension bundle {}",
            display_path(&target)
        )
    })
}

pub fn list_sideloaded_extensions(paths: &LauncherPaths) -> Result<Vec<String>> {
    let mut result = Vec::new();
    if !paths.extensions_dir.exists() {
        return Ok(result);
    }

    for entry in fs::read_dir(&paths.extensions_dir)
        .with_context(|| format!("reading {}", display_path(&paths.extensions_dir)))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            result.push(name);
        }
    }

    result.sort();
    Ok(result)
}
