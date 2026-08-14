use std::env;
use std::path::Path;

use super::APP_DATA_NAME;

pub fn normalize_base_url(url: &str) -> String {
    let mut trimmed = url.trim().to_string();
    if !trimmed.ends_with('/') {
        trimmed.push('/');
    }
    trimmed
}

pub(crate) fn app_data_name() -> String {
    env::var("SS14_LAUNCHER_APPDATA_NAME").unwrap_or_else(|_| APP_DATA_NAME.to_string())
}

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
