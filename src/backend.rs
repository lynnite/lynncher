mod accounts;
mod api;
mod common;
mod config;
mod content;
mod content_db;
mod download;
mod extensions;
mod http;
mod launch;
mod loader;
mod loader_launch;
mod types;
mod updates;
mod uri;

pub use accounts::{
    account_key,
    active_account_for_auth,
    auth_mode_disabled,
    remove_account,
    upsert_account,
};
pub use api::{
    authenticate_account_with_proxy,
    fetch_hub_servers_with_options,
    fetch_server_info_direct_with_proxy,
    fetch_server_info_from_hub_with_options,
};
pub use common::normalize_base_url;
pub use config::{ensure_dirs, launcher_paths, load_config, load_config_from_path, save_config};
pub use uri::apply_acz_inferred_urls;
pub use uri::derive_connect_address;
pub use updates::{check_latest_release, is_newer_tag};
pub use content::{download_and_install_content, download_content_zip, download_content_entries, merge_content_into};
pub use download::{
    download_client_for_server_with_proxy_and_tokens,
    download_engine_client_for_version,
    download_engine_module_for_engine_version,
    download_engine_zip_for_loader,
    is_connection_cancelled,
    stage_sdl3_native_runtime,
};
pub use extensions::{list_sideloaded_extensions, remove_sideloaded_extension, sideload_extension_bundle};
pub use launch::launch_game_with_context;
pub use loader::{ensure_loader_installed, LoaderInstall};
pub use loader_launch::{launch_game_via_loader, LoaderLaunchSpec};
pub use content_db::{build_content_database, ContentDbEntry, ContentDatabase};
pub use types::{
    AccountProfile,
    BackgroundImageConfig,
    ClientInstall,
    ColorScheme,
    HubRequestOptions,
    HubServerEntry,
    LauncherConfig,
    LauncherPaths,
    ServerAuthInformation,
    ServerBuildInformation,
    ServerInfo,
    DEFAULT_AUTH_SERVER,
    DEFAULT_HUB_SERVER,
};

pub(crate) use common::{app_data_name, display_path};
pub(crate) use types::{APP_DATA_NAME, APP_VENDOR_DIR, CONFIG_FILE_NAME, SS14_DEFAULT_PORT};

