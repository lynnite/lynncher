use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{display_path, LauncherPaths};

pub const MAX_BUNDLE_TOTAL_BYTES: u64 = 45 * 1024 * 1024;

pub const MAX_BUNDLE_FILES: usize = 650;

pub const MAX_BUNDLE_COMPRESSION_RATIO: f64 = 100.0;

pub const MAX_BUNDLE_ASSEMBLIES: usize = 16;

pub const RESOURCES_DIR_NAME: &str = "Resources";

const CHECKSUM_FILE_NAME: &str = ".lynncher_bundle_hash";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInfo {
    pub name: String,
    pub kind: String,
    pub has_assembly: bool,
    pub has_resources: bool,
    pub hash: String,
    pub description: String,
}

impl ExtensionInfo {
    pub fn is_loadable(&self) -> bool {
        self.has_assembly || self.has_resources
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedBundle {
    pub name: String,
    pub root: PathBuf,
    pub assembly: Option<PathBuf>,
    pub resources: Option<PathBuf>,
}

fn is_safe_entry_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }

    let normalised = name.replace('\\', "/");

    if normalised.starts_with('/') {
        return false;
    }
    if normalised.len() >= 2 {
        let bytes = normalised.as_bytes();
        if bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            return false;
        }
    }

    let path = Path::new(&normalised);
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    true
}

fn hash_file(path: &Path) -> Result<String> {
    use blake2::digest::consts::U32;
    use blake2::{Blake2b, Digest};
    let mut hasher = Blake2b::<U32>::new();
    let mut file = fs::File::open(path)
        .with_context(|| format!("opening {} for hashing", display_path(path)))?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buf)
            .with_context(|| format!("reading {} for hashing", display_path(path)))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn analyse_zip(archive: &mut zip::ZipArchive<fs::File>) -> Result<()> {
    let mut total_bytes: u64 = 0;
    let mut file_count: usize = 0;

    for idx in 0..archive.len() {
        let entry = archive
            .by_index_raw(idx)
            .with_context(|| format!("reading zip entry #{idx}"))?;
        let name = entry.name().to_string();

        if !is_safe_entry_name(&name) {
            anyhow::bail!("zip entry has an unsafe path: {name:?}");
        }

        if name.ends_with('/') {
            continue;
        }

        file_count += 1;
        total_bytes = total_bytes.saturating_add(entry.size());

        if file_count > MAX_BUNDLE_FILES {
            anyhow::bail!("bundle contains too many files (> {MAX_BUNDLE_FILES})");
        }
        if total_bytes > MAX_BUNDLE_TOTAL_BYTES {
            anyhow::bail!(
                "bundle would expand to too many bytes (> {MAX_BUNDLE_TOTAL_BYTES})"
            );
        }

        let compressed = entry.compressed_size().max(1);
        let ratio = entry.size() as f64 / compressed as f64;
        if ratio > MAX_BUNDLE_COMPRESSION_RATIO {
            anyhow::bail!(
                "zip entry {name:?} has a suspicious compression ratio ({ratio:.0}x)"
            );
        }
    }

    Ok(())
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("opening {}", display_path(zip_path)))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("opening zip {}", display_path(zip_path)))?;

    analyse_zip(&mut archive)?;

    fs::create_dir_all(dest).with_context(|| format!("creating {}", display_path(dest)))?;

    let mut total_written: u64 = 0;
    let mut files_written: usize = 0;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .with_context(|| format!("reading zip entry #{idx}"))?;
        let raw_name = entry.name().to_string();

        if !is_safe_entry_name(&raw_name) {
            anyhow::bail!("zip entry has an unsafe path: {raw_name:?}");
        }

        let out_path = dest.join(raw_name.replace('\\', "/"));

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("creating {}", display_path(&out_path)))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", display_path(parent)))?;
        }

        let mut out = fs::File::create(&out_path)
            .with_context(|| format!("creating {}", display_path(&out_path)))?;
        let mut buf = [0u8; 16 * 1024];
        loop {
            let read = entry
                .read(&mut buf)
                .with_context(|| format!("reading zip entry {raw_name:?}"))?;
            if read == 0 {
                break;
            }
            total_written = total_written.saturating_add(read as u64);
            files_written += 1;
            if total_written > MAX_BUNDLE_TOTAL_BYTES {
                anyhow::bail!("bundle exceeded the extraction size limit");
            }
            if files_written > MAX_BUNDLE_FILES.saturating_mul(64) {
                anyhow::bail!("bundle exceeded the extraction write limit");
            }
            out.write_all(&buf[..read])
                .with_context(|| format!("writing {}", display_path(&out_path)))?;
        }
    }

    Ok(())
}

fn find_bundle_assembly(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    let mut count = 0;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("dll"))
                .unwrap_or(false)
        {
            return Some(path);
        }
        count += 1;
        if count > MAX_BUNDLE_ASSEMBLIES * 64 {
            break;
        }
    }
    None
}

fn find_bundle_resources(root: &Path) -> Option<PathBuf> {
    let candidate = root.join(RESOURCES_DIR_NAME);
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

fn extracted_cache_dir(paths: &LauncherPaths, name: &str) -> PathBuf {
    paths
        .clients_dir
        .join("extensions_cache")
        .join(safe_file_stem(name))
}

fn safe_file_stem(name: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bundle");
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn ensure_zip_extracted(
    paths: &LauncherPaths,
    name: &str,
    zip_path: &Path,
) -> Result<(PathBuf, String)> {
    let current_hash = hash_file(zip_path)?;
    let cache_dir = extracted_cache_dir(paths, name);
    let checksum_path = cache_dir.join(CHECKSUM_FILE_NAME);

    if cache_dir.is_dir() {
        if let Ok(stored) = fs::read_to_string(&checksum_path) {
            if stored.trim() == current_hash {
                return Ok((cache_dir, current_hash));
            }
        }
        let _ = fs::remove_dir_all(&cache_dir);
    }

    extract_zip(zip_path, &cache_dir)?;
    fs::write(&checksum_path, &current_hash)
        .with_context(|| format!("writing {}", display_path(&checksum_path)))?;

    Ok((cache_dir, current_hash))
}

fn inspect_entry(path: &Path) -> Result<Option<ExtensionInfo>> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .context("invalid extension file name")?;

    if path.is_dir() {
        let assembly = find_bundle_assembly(path);
        let resources = find_bundle_resources(path);
        return Ok(Some(ExtensionInfo {
            name,
            kind: "folder".to_string(),
            has_assembly: assembly.is_some(),
            has_resources: resources.is_some(),
            hash: String::new(),
            description: read_bundle_description(path),
        }));
    }

    if !path.is_file() {
        return Ok(None);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "dll" => Ok(Some(ExtensionInfo {
            name,
            kind: "dll".to_string(),
            has_assembly: true,
            has_resources: false,
            hash: String::new(),
            description: String::new(),
        })),
        "zip" => {
            let file = fs::File::open(path)
                .with_context(|| format!("opening {}", display_path(path)))?;
            let mut archive = zip::ZipArchive::new(file)
                .with_context(|| format!("opening zip {}", display_path(path)))?;
            let mut has_assembly = false;
            let mut has_resources = false;
            for idx in 0..archive.len() {
                let entry = archive.by_index_raw(idx)?;
                let entry_name = entry.name().replace('\\', "/");
                let is_root = !entry_name.contains('/');
                if is_root && entry_name.to_ascii_lowercase().ends_with(".dll") {
                    has_assembly = true;
                }
                if entry_name == format!("{RESOURCES_DIR_NAME}/")
                    || entry_name.starts_with(&format!("{RESOURCES_DIR_NAME}/"))
                {
                    has_resources = true;
                }
            }
            Ok(Some(ExtensionInfo {
                name,
                kind: "zip".to_string(),
                has_assembly,
                has_resources,
                hash: String::new(),
                description: String::new(),
            }))
        }
        _ => Ok(None),
    }
}

fn read_bundle_description(root: &Path) -> String {
    for candidate in ["description.txt", "README.md", "readme.md", "README.txt"] {
        let p = root.join(candidate);
        if let Ok(text) = fs::read_to_string(&p) {
            if let Some(line) = text.lines().find(|l| !l.trim().is_empty()) {
                return line.trim().to_string();
            }
        }
    }
    String::new()
}

pub fn sideload_extension_bundle(paths: &LauncherPaths, source_path: &Path) -> Result<String> {
    if !source_path.exists() {
        anyhow::bail!("extension source does not exist");
    }

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .context("invalid extension file name")?;

    if source_path.is_file() {
        let ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "dll" && ext != "zip" {
            anyhow::bail!("unsupported extension bundle type: .{ext} (expected .dll or .zip)");
        }
    }

    let target = paths.extensions_dir.join(&file_name);

    if source_path.is_dir() {
        if target.exists() {
            fs::remove_dir_all(&target).ok();
        }
        copy_dir_recursive(source_path, &target).with_context(|| {
            format!(
                "copying extension folder from {} to {}",
                display_path(source_path),
                display_path(&target)
            )
        })?;
    } else {
        fs::copy(source_path, &target).with_context(|| {
            format!(
                "copying extension bundle from {} to {}",
                display_path(source_path),
                display_path(&target)
            )
        })?;
    }

    Ok(file_name)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
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

pub fn remove_sideloaded_extension(paths: &LauncherPaths, name: &str) -> Result<()> {
    let safe_name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid extension name"))?;
    let target = paths.extensions_dir.join(safe_name);
    let cache = extracted_cache_dir(paths, safe_name);

    let mut removed = false;
    if target.exists() {
        if target.is_dir() {
            fs::remove_dir_all(&target).with_context(|| {
                format!("removing extension folder {}", display_path(&target))
            })?;
        } else {
            fs::remove_file(&target).with_context(|| {
                format!("removing extension bundle {}", display_path(&target))
            })?;
        }
        removed = true;
    }

    if cache.exists() {
        let _ = fs::remove_dir_all(&cache);
    }

    if !removed {
        anyhow::bail!("extension not found: {}", safe_name);
    }
    Ok(())
}

pub fn inspect_extensions(paths: &LauncherPaths) -> Result<Vec<ExtensionInfo>> {
    let mut result = Vec::new();
    if !paths.extensions_dir.exists() {
        return Ok(result);
    }

    for entry in fs::read_dir(&paths.extensions_dir)
        .with_context(|| format!("reading {}", display_path(&paths.extensions_dir)))?
    {
        let entry = entry?;
        if let Some(info) = inspect_entry(&entry.path())? {
            if info.is_loadable() {
                result.push(info);
            }
        }
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

pub fn list_sideloaded_extensions(paths: &LauncherPaths) -> Result<Vec<String>> {
    Ok(inspect_extensions(paths)?
        .into_iter()
        .map(|info| info.name)
        .collect())
}

pub fn resolve_enabled_extensions(
    paths: &LauncherPaths,
    enabled: &[String],
) -> Result<Vec<ResolvedBundle>> {
    let mut resolved = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for name in enabled {
        if !seen.insert(name.clone()) {
            continue;
        }

        let candidate = paths.extensions_dir.join(
            Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(name),
        );
        if !candidate.exists() {
            continue;
        }

        let info = match inspect_entry(&candidate)? {
            Some(info) if info.is_loadable() => info,
            _ => continue,
        };

        let root = match info.kind.as_str() {
            "zip" => {
                let (dir, _hash) = ensure_zip_extracted(paths, &info.name, &candidate)?;
                dir
            }
            "folder" => candidate.clone(),
            _ => {
                let dir = extracted_cache_dir(paths, &info.name);
                fs::create_dir_all(&dir)
                    .with_context(|| format!("creating {}", display_path(&dir)))?;
                let target = dir.join(&info.name);
                if !target.exists() {
                    fs::copy(&candidate, &target).with_context(|| {
                        format!(
                            "staging dll {} into {}",
                            display_path(&candidate),
                            display_path(&target)
                        )
                    })?;
                }
                dir
            }
        };

        let assembly = find_bundle_assembly(&root);
        let resources = find_bundle_resources(&root);

        if assembly.is_none() && resources.is_none() {
            continue;
        }

        resolved.push(ResolvedBundle {
            name: info.name,
            root,
            assembly,
            resources,
        });
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("lynncher-ext-test-{tag}-{pid}-{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_paths(base: &Path) -> LauncherPaths {
        LauncherPaths {
            user_data_dir: base.join("user"),
            local_data_dir: base.join("local"),
            logs_dir: base.join("logs"),
            clients_dir: base.join("clients"),
            extensions_dir: base.join("extensions"),
            config_path: base.join("config.toml"),
        }
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        assert!(!is_safe_entry_name("../evil.dll"));
        assert!(!is_safe_entry_name("a/../../evil.dll"));
        assert!(!is_safe_entry_name("..\\evil.dll"));
    }

    #[test]
    fn rejects_absolute_and_drive_paths() {
        assert!(!is_safe_entry_name("/etc/passwd"));
        assert!(!is_safe_entry_name("C:/windows/system32/evil.dll"));
        assert!(!is_safe_entry_name("\\\\server\\share\\evil.dll"));
    }

    #[test]
    fn rejects_null_byte_and_empty() {
        assert!(!is_safe_entry_name(""));
        assert!(!is_safe_entry_name("evil\0.dll"));
    }

    #[test]
    fn accepts_normal_names() {
        assert!(is_safe_entry_name("Patch.dll"));
        assert!(is_safe_entry_name("Resources/foo.png"));
        assert!(is_safe_entry_name("./sub/file.txt"));
    }

    #[test]
    fn safe_file_stem_sanitizes() {
        assert_eq!(safe_file_stem("My Patch.zip"), "My_Patch");
        assert_eq!(safe_file_stem("a/../../evil.zip"), "evil");
        assert_eq!(safe_file_stem("ok-name_1.0.dll"), "ok-name_1.0");
    }

    #[test]
    fn hash_is_deterministic_and_content_addressed() {
        let dir = unique_dir("hash");
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        fs::write(&a, b"hello").unwrap();
        fs::write(&b, b"hello").unwrap();
        let ha = hash_file(&a).unwrap();
        let hb = hash_file(&b).unwrap();
        assert_eq!(ha, hb);
        assert_eq!(ha.len(), 64);
        fs::write(&b, b"different").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn resolves_zip_bundle_with_dll_and_resources() {
        let dir = unique_dir("zipok");
        let paths = test_paths(&dir);
        fs::create_dir_all(&paths.extensions_dir).unwrap();
        let zip_path = paths.extensions_dir.join("bundle.zip");
        write_zip(
            &zip_path,
            &[
                ("Patch.dll", b"MZ fake assembly bytes"),
                ("Resources/Content/foo.txt", b"hi"),
            ],
        );

        let names = list_sideloaded_extensions(&paths).unwrap();
        assert_eq!(names, vec!["bundle.zip".to_string()]);

        let resolved = resolve_enabled_extensions(&paths, &names).unwrap();
        assert_eq!(resolved.len(), 1);
        let rb = &resolved[0];
        assert!(rb.assembly.is_some());
        assert!(rb.resources.is_some());
        assert!(rb.assembly.as_ref().unwrap().exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_traversal_entry_is_rejected_on_extract() {
        let dir = unique_dir("zipevil");
        let paths = test_paths(&dir);
        fs::create_dir_all(&paths.extensions_dir).unwrap();
        let zip_path = paths.extensions_dir.join("evil.zip");
        write_zip(&zip_path, &[("../escape.dll", b"evil")]);

        let dest = dir.join("out");
        let result = extract_zip(&zip_path, &dest);
        assert!(result.is_err());
        assert!(!dir.join("escape.dll").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn standalone_dll_is_resolved() {
        let dir = unique_dir("dll");
        let paths = test_paths(&dir);
        fs::create_dir_all(&paths.extensions_dir).unwrap();
        fs::write(paths.extensions_dir.join("Patch.dll"), b"MZ fake").unwrap();

        let resolved = resolve_enabled_extensions(&paths, &["Patch.dll".to_string()]).unwrap();
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].assembly.as_ref().unwrap().exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_or_missing_names_resolve_to_nothing() {
        let dir = unique_dir("none");
        let paths = test_paths(&dir);
        fs::create_dir_all(&paths.extensions_dir).unwrap();
        let resolved =
            resolve_enabled_extensions(&paths, &["does-not-exist.zip".to_string()]).unwrap();
        assert!(resolved.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sideload_rejects_unknown_extension_type() {
        let dir = unique_dir("badtype");
        let paths = test_paths(&dir);
        fs::create_dir_all(&paths.extensions_dir).unwrap();
        let src = dir.join("patch.exe");
        fs::write(&src, b"MZ").unwrap();
        let err = sideload_extension_bundle(&paths, &src);
        assert!(err.is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
