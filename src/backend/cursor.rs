use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const FALLBACK_CURSOR_SIZE: &str = "24";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorTheme {
    pub name: String,
    pub size: String,
    pub path: Option<String>,
}

impl CursorTheme {
    fn new(name: String, size: String, path: Option<String>) -> Self {
        Self { name, size, path }
    }
}

pub fn resolve_cursor_theme() -> Option<CursorTheme> {
    let env_theme = std::env::var("XCURSOR_THEME").ok().filter(|s| !s.trim().is_empty());
    let env_size = std::env::var("XCURSOR_SIZE").ok().filter(|s| !s.trim().is_empty());

    if let Some(name) = env_theme {
        let size = env_size.unwrap_or_else(|| FALLBACK_CURSOR_SIZE.to_string());
        let path = std::env::var("XCURSOR_PATH").ok().filter(|s| !s.trim().is_empty());
        return Some(CursorTheme::new(name, size, path));
    }

    let (gsettings_theme, gsettings_size) = query_gsettings_cursor();
    let (kde_theme, kde_size) = query_kde_cursor();

    let name = gsettings_theme
        .or(kde_theme)
        .or_else(guess_installed_cursor_theme);

    let name = name?;
    let size = env_size
        .or(gsettings_size)
        .or(kde_size)
        .unwrap_or_else(|| FALLBACK_CURSOR_SIZE.to_string());

    let path = std::env::var("XCURSOR_PATH").ok().filter(|s| !s.trim().is_empty());
    Some(CursorTheme::new(name, size, path))
}

pub fn apply_cursor_env_with_override(command: &mut Command, theme_override: Option<&str>) {
    let forced = theme_override
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let theme = match forced {
        Some(name) => {
            let size = std::env::var("XCURSOR_SIZE")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| query_gsettings_cursor().1)
                .or_else(|| query_kde_cursor().1)
                .unwrap_or_else(|| FALLBACK_CURSOR_SIZE.to_string());
            let path = std::env::var("XCURSOR_PATH").ok().filter(|s| !s.trim().is_empty());
            CursorTheme::new(name.to_string(), size, path)
        }
        None => match resolve_cursor_theme() {
            Some(theme) => theme,
            None => return,
        },
    };

    command.env("XCURSOR_THEME", &theme.name);
    command.env("XCURSOR_SIZE", &theme.size);

    let path = theme
        .path
        .clone()
        .unwrap_or_else(default_cursor_search_path);
    if !path.trim().is_empty() {
        command.env("XCURSOR_PATH", path);
    }

    for key in ["XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", "WAYLAND_DISPLAY"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn query_gsettings_cursor() -> (Option<String>, Option<String>) {
    let theme = run_and_trim("gsettings", &["get", "org.gnome.desktop.interface", "cursor-theme"])
        .map(|s| unquote_gvariant(&s))
        .filter(|s| !s.is_empty());
    let size = run_and_trim("gsettings", &["get", "org.gnome.desktop.interface", "cursor-size"])
        .map(|s| unquote_gvariant(&s))
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
    (theme, size)
}

fn query_kde_cursor() -> (Option<String>, Option<String>) {
    let theme = run_and_trim(
        "kreadconfig6",
        &["--file", "kcminputrc", "--group", "Mouse", "--key", "cursorTheme"],
    )
    .or_else(|| {
        run_and_trim(
            "kreadconfig5",
            &["--file", "kcminputrc", "--group", "Mouse", "--key", "cursorTheme"],
        )
    })
    .filter(|s| !s.is_empty());
    let size = run_and_trim(
        "kreadconfig6",
        &["--file", "kcminputrc", "--group", "Mouse", "--key", "cursorSize"],
    )
    .or_else(|| {
        run_and_trim(
            "kreadconfig5",
            &["--file", "kcminputrc", "--group", "Mouse", "--key", "cursorSize"],
        )
    })
    .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
    (theme, size)
}

fn run_and_trim(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(text)
}

fn unquote_gvariant(value: &str) -> String {
    value.trim().trim_matches('\'').to_string()
}

fn default_cursor_search_path() -> String {
    let mut dirs: Vec<String> = vec![
        "/usr/share/icons".to_string(),
        "/usr/local/share/icons".to_string(),
        "/usr/share/X11/icons".to_string(),
    ];

    if let Some(home) = home_dir() {
        dirs.insert(0, home.join(".icons").to_string_lossy().into_owned());
        dirs.insert(
            0,
            home.join(".local/share/icons").to_string_lossy().into_owned(),
        );
        if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
            if !data_home.trim().is_empty() {
                dirs.insert(0, Path::new(&data_home).join("icons").to_string_lossy().into_owned());
            }
        }
    }

    dirs.join(":")
}

fn guess_installed_cursor_theme() -> Option<String> {
    let mut found: BTreeMap<String, ()> = BTreeMap::new();

    if let Some(home) = home_dir() {
        collect_cursor_themes(&home.join(".icons"), &mut found);
        collect_cursor_themes(&home.join(".local/share/icons"), &mut found);
    }
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.trim().is_empty() {
            collect_cursor_themes(&Path::new(&data_home).join("icons"), &mut found);
        }
    }
    collect_cursor_themes(Path::new("/usr/share/icons"), &mut found);
    collect_cursor_themes(Path::new("/usr/local/share/icons"), &mut found);

    for preferred in [
        "Adwaita",
        "Adwaita-dark",
        "Yaru",
        "breeze_cursors",
        "Breeze",
        "DMZ-White",
        "DMZ-Black",
        "Vanilla-DMZ",
        "whiteglass",
    ] {
        if found.contains_key(preferred) {
            return Some(preferred.to_string());
        }
    }

    if found.len() == 1 {
        return found.keys().next().cloned();
    }

    None
}

fn collect_cursor_themes(dir: &Path, out: &mut BTreeMap<String, ()>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path: PathBuf = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("cursors").is_dir() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                out.insert(name.to_string(), ());
            }
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_gvariant_strips_quotes() {
        assert_eq!(unquote_gvariant("'Adwaita'"), "Adwaita");
        assert_eq!(unquote_gvariant("Adwaita"), "Adwaita");
        assert_eq!(unquote_gvariant("  'Yaru'  "), "Yaru");
    }

    #[test]
    fn collect_cursor_themes_requires_cursors_dir() {
        let mut map = BTreeMap::new();
        collect_cursor_themes(Path::new("/definitely/not/a/dir"), &mut map);
        assert!(map.is_empty());
    }
}
