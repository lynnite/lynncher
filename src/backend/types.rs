use std::path::PathBuf;


use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(crate) const APP_VENDOR_DIR: &str = "Space Station 14";
pub(crate) const APP_DATA_NAME: &str = "launcher-rust";
pub(crate) const CONFIG_FILE_NAME: &str = "config.toml";
pub const DEFAULT_HUB_SERVER: &str = "https://hub.playss14.com/";
pub const DEFAULT_AUTH_SERVER: &str = "https://auth.playss14.com/";
pub(crate) const SS14_DEFAULT_PORT: u16 = 1212;

#[derive(Debug, Clone)]
pub struct LauncherPaths {
    pub user_data_dir: PathBuf,
    pub local_data_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub clients_dir: PathBuf,
    pub extensions_dir: PathBuf,
    pub config_path: PathBuf,
}

fn default_reconnect_delay() -> u64 {
    3000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorScheme {
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
    pub header_r: u8,
    pub header_g: u8,
    pub header_b: u8,
    pub footer_r: u8,
    pub footer_g: u8,
    pub footer_b: u8,
    pub popup_r: u8,
    pub popup_g: u8,
    pub popup_b: u8,
    pub button_r: u8,
    pub button_g: u8,
    pub button_b: u8,
    pub hover_r: u8,
    pub hover_g: u8,
    pub hover_b: u8,
    pub item_r: u8,
    pub item_g: u8,
    pub item_b: u8,
    pub text_r: u8,
    pub text_g: u8,
    pub text_b: u8,
    pub sub_text_r: u8,
    pub sub_text_g: u8,
    pub sub_text_b: u8,
    pub accent_r: u8,
    pub accent_g: u8,
    pub accent_b: u8,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            bg_r: 0x2B,
            bg_g: 0x2B,
            bg_b: 0x2B,
            header_r: 0x24,
            header_g: 0x24,
            header_b: 0x24,
            footer_r: 0x19,
            footer_g: 0x19,
            footer_b: 0x19,
            popup_r: 0x1F,
            popup_g: 0x1F,
            popup_b: 0x1F,
            button_r: 0x3A,
            button_g: 0x3A,
            button_b: 0x3A,
            hover_r: 0x3E,
            hover_g: 0x3E,
            hover_b: 0x3E,
            item_r: 0x33,
            item_g: 0x33,
            item_b: 0x33,
            text_r: 0xE6,
            text_g: 0xE6,
            text_b: 0xE6,
            sub_text_r: 0x8F,
            sub_text_g: 0x8F,
            sub_text_b: 0x8F,
            accent_r: 0x4A,
            accent_g: 0x4A,
            accent_b: 0x4A,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundImageConfig {
    pub pos_x: f32,
    pub pos_y: f32,
    pub scale: f32,
    #[serde(default)]
    pub locked: bool,
}

impl Default for BackgroundImageConfig {
    fn default() -> Self {
        Self {
            pos_x: 0.0,
            pos_y: 0.0,
            scale: 1.0,
            locked: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub game_executable: String,
    pub connect_uri: String,
    pub extra_args: String,
    pub hub_server_url: String,
    pub auth_server_url: String,
    pub proxy_enabled: bool,
    pub proxy_url: String,
    pub proxy_presets: Vec<String>,
    #[serde(default)]
    pub auto_reconnect: bool,
    #[serde(default = "default_reconnect_delay")]
    pub auto_reconnect_delay_ms: u64,
    #[serde(default)]
    pub auto_update: bool,
    #[serde(default)]
    pub background_image: String,
    #[serde(default)]
    pub background_image_config: BackgroundImageConfig,
    #[serde(default)]
    pub color_scheme: ColorScheme,
    pub favorite_servers: Vec<String>,
    #[serde(default)]
    pub favorite_names: Vec<(String, String)>,
    pub accounts: Vec<AccountProfile>,
    pub active_account_key: Option<String>,
    pub enabled_extensions: Vec<String>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            game_executable: String::new(),
            connect_uri: String::new(),
            extra_args: String::new(),
            hub_server_url: DEFAULT_HUB_SERVER.to_string(),
            auth_server_url: DEFAULT_AUTH_SERVER.to_string(),
            proxy_enabled: false,
            proxy_url: String::new(),
            proxy_presets: Vec::new(),
            auto_reconnect: false,
            auto_reconnect_delay_ms: 3000,
            auto_update: false,
            background_image: String::new(),
            background_image_config: BackgroundImageConfig::default(),
            color_scheme: ColorScheme::default(),
            favorite_servers: Vec::new(),
            favorite_names: Vec::new(),
            accounts: Vec::new(),
            active_account_key: None,
            enabled_extensions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountProfile {
    pub auth_server: String,
    pub username: String,
    pub user_id: String,
    pub token: String,
    pub expire_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HubServerEntry {
    #[serde(rename = "address", alias = "Address")]
    pub address: String,
    #[serde(rename = "statusData", alias = "StatusData")]
    pub status_data: ServerStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerStatus {
    #[serde(rename = "name")]
    pub name: Option<String>,
    #[serde(rename = "players")]
    pub players: i32,
    #[serde(rename = "soft_max_players")]
    pub soft_max_players: i32,
    #[serde(rename = "round_start_time")]
    pub round_start_time: Option<String>,
    #[serde(rename = "run_level")]
    pub run_level: Option<serde_json::Value>,
    #[serde(rename = "tags")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct HubRequestOptions {
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    #[serde(rename = "connect_address")]
    pub connect_address: Option<String>,
    #[serde(rename = "build")]
    pub build: Option<ServerBuildInformation>,
    #[serde(rename = "auth")]
    pub auth: ServerAuthInformation,
    #[serde(rename = "desc")]
    pub desc: Option<String>,
    #[serde(rename = "links")]
    pub links: Option<Vec<ServerInfoLink>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfoLink {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "icon")]
    pub icon: Option<String>,
    #[serde(rename = "url")]
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerAuthInformation {
    #[serde(rename = "mode")]
    pub mode: serde_json::Value,
    #[serde(rename = "public_key")]
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerBuildInformation {
    #[serde(rename = "download_url")]
    pub download_url: Option<String>,
    #[serde(rename = "manifest_url")]
    pub manifest_url: Option<String>,
    #[serde(rename = "manifest_download_url")]
    pub manifest_download_url: Option<String>,
    #[serde(rename = "engine_version")]
    pub engine_version: Option<String>,
    #[serde(rename = "version")]
    pub version: Option<String>,
    #[serde(rename = "fork_id")]
    pub fork_id: Option<String>,
    #[serde(rename = "hash")]
    pub hash: Option<String>,
    #[serde(rename = "manifest_hash")]
    pub manifest_hash: Option<String>,
    #[serde(rename = "acz")]
    pub acz: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ClientInstall {
    pub install_dir: PathBuf,
    pub executable_path: PathBuf,
    pub connect_address: String,
}
