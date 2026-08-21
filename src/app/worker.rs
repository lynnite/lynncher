
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::i18n::Localizer;
use crate::backend::{
    account_key, active_account_for_auth, apply_acz_inferred_urls, apply_hwid_for_launch,
    auth_mode_disabled,
    build_content_database, derive_connect_address, download_and_install_content,
    download_client_for_server_with_proxy_and_tokens, download_content_entries,
    download_content_zip, download_engine_client_for_version,
    download_engine_module_for_engine_version, download_engine_zip_for_loader,
    ensure_loader_installed, is_connection_cancelled, launch_game_via_loader,
    launch_game_with_context, merge_content_into, normalize_base_url, stage_sdl3_native_runtime,
    ContentDbEntry, HubRequestOptions,
    LauncherConfig, LauncherPaths, LoaderLaunchSpec, ServerInfo,
};

#[derive(Default)]
pub struct FeedbackState {
    pub status: String,
    pub progress: Option<(f32, String)>,
    pub logs: Vec<String>,
    pub running: bool,
    pub cancel: Arc<AtomicBool>,
}

pub type SharedFeedback = Arc<Mutex<FeedbackState>>;

pub fn new_feedback() -> SharedFeedback {
    Arc::new(Mutex::new(FeedbackState {
        running: true,
        cancel: Arc::new(AtomicBool::new(false)),
        ..Default::default()
    }))
}

pub struct BackgroundWork {
    pub feedback: SharedFeedback,
    pub thread: Option<std::thread::JoinHandle<()>>,
}

pub struct Connector {
    pub paths: LauncherPaths,
    pub cfg: LauncherConfig,
    feedback: SharedFeedback,
    loc: Localizer,
}

impl Connector {
    pub fn new(paths: LauncherPaths, cfg: LauncherConfig, feedback: SharedFeedback) -> Self {
        let loc = Localizer::new(&cfg.language);
        Self { paths, cfg, feedback, loc }
    }

    fn t(&self, key: &str, args: &[&str]) -> String {
        self.loc.t(key, args)
    }

    fn set_status(&self, msg: impl Into<String>) {
        if let Ok(mut f) = self.feedback.lock() {
            f.status = msg.into();
        }
    }

    fn push_log(&self, msg: impl Into<String>) {
        if let Ok(mut f) = self.feedback.lock() {
            f.logs.push(msg.into());
        }
    }

    fn set_progress(&self, fraction: f32, label: impl Into<String>) {
        if let Ok(mut f) = self.feedback.lock() {
            f.progress = Some((fraction.clamp(0.0, 1.0), label.into()));
        }
    }

    fn clear_progress(&self) {
        if let Ok(mut f) = self.feedback.lock() {
            f.progress = None;
        }
    }

    fn hub_options(&self) -> HubRequestOptions {
        HubRequestOptions {
            proxy_url: if self.cfg.proxy_enabled && !self.cfg.proxy_url.trim().is_empty() {
                Some(self.cfg.proxy_url.trim().to_string())
            } else {
                None
            },
        }
    }

    fn download_auth_tokens(&self) -> Vec<Option<String>> {
        let mut out: Vec<Option<String>> = Vec::new();

        if let Some(active_key) = self.cfg.active_account_key.as_deref() {
            if let Some(active) = self
                .cfg
                .accounts
                .iter()
                .find(|acc| account_key(&acc.auth_server, &acc.user_id) == active_key)
            {
                let token = active.token.trim().to_string();
                if !token.is_empty() {
                    out.push(Some(token));
                }
            }
        }

        for account in &self.cfg.accounts {
            let token = account.token.trim();
            if token.is_empty() {
                continue;
            }
            if out
                .iter()
                .any(|t| t.as_deref().map(|x| x == token).unwrap_or(false))
            {
                continue;
            }
            out.push(Some(token.to_string()));
        }

        out.push(None);
        out
    }

    fn feedback_status(&self) -> String {
        self.feedback
            .lock()
            .ok()
            .map(|f| f.status.clone())
            .unwrap_or_default()
    }

    fn finish(&self) {
        self.clear_progress();
        if let Ok(mut f) = self.feedback.lock() {
            f.running = false;
        }
    }

    fn is_cancelled(&self) -> bool {
        self.feedback
            .lock()
            .ok()
            .map(|f| f.cancel.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    fn remove_dir(&self, dir: &Path) {
        if dir.exists() && dir.is_dir() {
            let _ = walkdir::WalkDir::new(dir)
                .sort_by_file_name()
                .into_iter()
                .filter_map(|e| e.ok())
                .for_each(|e| {
                    let _ = std::fs::remove_file(e.path());
                });
            let _ = std::fs::remove_dir_all(dir);
            self.push_log(format!(
                "Cancelled connection; removed partial download {}",
                dir.to_string_lossy()
            ));
        }
    }

    /// Runs a game in a reconnecting loop. Returns `true` if the game managed
    /// to start at least once, or `false` if the very first launch attempt
    /// failed (which the caller can use to trigger a fresh-install retry).
    fn reconnect_loop<F>(&self, mut launch: F, initial_msg: String) -> bool
    where
        F: FnMut() -> anyhow::Result<std::process::Child>,
    {
        let mut first = true;
        let mut first_launch_ok = false;
        loop {
            match launch() {
                Ok(mut child) => {
                    first_launch_ok = true;
                    if first {
                        self.set_status(initial_msg.clone());
                        self.push_log(self.feedback_status());
                        first = false;
                    } else {
                        self.set_status(self.t("status.reconnecting", &[]));
                        self.push_log(self.feedback_status());
                    }

                    // The game process has spawned; hide the progress overlay
                    // immediately so the loading bar does not linger over the
                    // launched client window while it is running.
                    self.clear_progress();

                    let _ = child.wait();
                    if self.is_cancelled() || !self.cfg.auto_reconnect {
                        break;
                    }

                    let delay_ms = self.cfg.auto_reconnect_delay_ms;
                    self.set_status(self.t("status.reconnect_delay", &[&delay_ms.to_string()]));
                    self.push_log(self.feedback_status());
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    if self.is_cancelled() {
                        break;
                    }
                }
                Err(err) => {
                    self.push_log(if first {
                        format!("Launch failed: {err:#}")
                    } else {
                        format!("Reconnect launch failed: {err:#}")
                    });
                    break;
                }
            }
        }
        first_launch_ok
    }

    pub fn connect(&self, address: &str, mut info: ServerInfo) {
        if let Some(build) = &info.build {
            info.build = apply_acz_inferred_urls(address, build).ok().or(info.build);
        }

        let proxy = self.hub_options().proxy_url;

        let cancel_flag = self
            .feedback
            .lock()
            .ok()
            .map(|f| f.cancel.clone());

        let auth_token_strings = self.download_auth_tokens();
        let auth_tokens: Vec<Option<&str>> = auth_token_strings
            .iter()
            .map(|t| t.as_deref())
            .collect();

        let download_err = match download_client_for_server_with_proxy_and_tokens(
            &self.paths,
            address,
            &info,
            proxy.as_deref(),
            &auth_tokens,
            cancel_flag.as_ref().map(Arc::as_ref),
        ) {
            Ok(install) => {
                if self.is_cancelled() {
                    self.remove_dir(&install.install_dir);
                    self.set_status(self.t("status.cancelled", &[]));
                    self.push_log(self.feedback_status());
                    self.finish();
                    return;
                }

                self.set_status(self.t(
                    "status.client_downloaded",
                    &[&install.install_dir.to_string_lossy()],
                ));
                self.push_log(self.feedback_status());

                let mut runtime_cfg = self.cfg.clone();
                runtime_cfg.game_executable = install.executable_path.to_string_lossy().into_owned();

                if let Some(exe_dir) = install.executable_path.parent() {
                    match stage_sdl3_native_runtime(&self.paths, exe_dir, proxy.as_deref()) {
                        Ok(()) => {
                            self.push_log(format!(
                                "Prepared SDL3 native runtime next to {}",
                                exe_dir.to_string_lossy()
                            ));
                        }
                        Err(err) => {
                            self.push_log(format!(
                                "SDL3 native staging failed, continuing with system libraries if available: {err:#}"
                            ));
                        }
                    }
                }

                let exe_dir = install
                    .executable_path
                    .parent()
                    .map(|p| p.to_path_buf());

                // Try to launch the freshly downloaded client. If the very
                // first launch attempt fails (e.g. because of stale/corrupted
                // files from an earlier engine/client download), clear the
                // staging directory and re-download once before giving up.
                let mut attempts = 0;
                loop {
                    let launched_msg = self.t("status.launched", &[&runtime_cfg.game_executable]);
                    let ok = self.reconnect_loop(
                        || launch_game_with_context(&runtime_cfg, Some(address), Some(&info)),
                        launched_msg,
                    );

                    if ok || self.is_cancelled() {
                        break;
                    }

                    attempts += 1;
                    if attempts >= 2 {
                        break;
                    }

                    self.push_log(
                        "Initial launch failed; clearing stale client files and re-downloading...",
                    );
                    if let Some(dir) = &exe_dir {
                        let _ = std::fs::remove_dir_all(dir);
                    }

                    match download_client_for_server_with_proxy_and_tokens(
                        &self.paths,
                        address,
                        &info,
                        proxy.as_deref(),
                        &auth_tokens,
                        cancel_flag.as_ref().map(Arc::as_ref),
                    ) {
                        Ok(install2) => {
                            runtime_cfg.game_executable =
                                install2.executable_path.to_string_lossy().into_owned();
                            let exe2_dir = install2
                                .executable_path
                                .parent()
                                .map(|p| p.to_path_buf());
                            if let Some(dir) = exe2_dir {
                                let _ = stage_sdl3_native_runtime(
                                    &self.paths,
                                    &dir,
                                    proxy.as_deref(),
                                );
                            }
                        }
                        Err(err) => {
                            self.push_log(format!("Re-install failed: {err:#}"));
                            break;
                        }
                    }
                }

                self.finish();
                return;
            }
            Err(err) => err,
        };

        if self.is_cancelled() {
            self.set_status(self.t("status.cancelled", &[]));
            self.push_log(self.feedback_status());
            self.finish();
            return;
        }

        if let Some(build) = &info.build {
            if let Some(engine_version) = build.engine_version.as_deref() {
                match self.try_launch_via_loader(address, &info, engine_version) {
                    Ok(true) => {
                        self.set_status(self.t("status.launched_loader", &[]));
                        self.push_log(self.feedback_status());
                        self.finish();
                        
                        return;
                    }
                    Ok(false) => {
                        self.push_log(
                            "Server has no manifest content; falling back to merged-engine launch.",
                        );
                    }
                    Err(err) => {
                        self.push_log(format!(
                            "SS14.Loader launch failed; falling back to merged-engine path: {err:#}"
                        ));
                    }
                }

                self.engine_fallback(address, &info, engine_version);
                self.finish();
                return;
            }
        }

        if is_connection_cancelled(&download_err) {
            self.set_status(self.t("status.cancelled", &[]));
        } else {
            self.set_status(self.t("status.client_download_fail", &[&download_err.to_string()]));
        }
        self.push_log(self.feedback_status());

        self.finish();
    }

    fn engine_fallback(&self, address: &str, info: &ServerInfo, engine_version: &str) {
        if self.is_cancelled() {
            self.set_status(self.t("status.cancelled", &[]));
            self.push_log(self.feedback_status());
            return;
        }

        let proxy = self.hub_options().proxy_url;

        match download_engine_client_for_version(&self.paths, engine_version, proxy.as_deref()) {
            Ok(engine_exe) => {
                if let Ok(module_dir) = download_engine_module_for_engine_version(
                    &self.paths,
                    "Robust.Client.WebView",
                    engine_version,
                    proxy.as_deref(),
                ) {
                    let module_dir = module_dir.to_string_lossy().to_string();
                    std::env::set_var("ROBUST_MODULE_ROBUST_CLIENT_WEBVIEW", &module_dir);
                    std::env::set_var("ROBUST_MODULE_SPACEWIZARDS_SDL", &module_dir);
                    self.push_log(format!("Prepared native module fallback at {}", module_dir));
                }

                let mut runtime_cfg = self.cfg.clone();
                runtime_cfg.game_executable = engine_exe.to_string_lossy().into_owned();
                self.set_status(self.t(
                    "status.engine_fallback",
                    &[&engine_version],
                ));
                self.push_log(self.feedback_status());

                let mut content_downloaded = false;
                if let Some(build) = &info.build {
                    let manifest_result =
                        download_and_install_content(&self.paths, address, build, proxy.as_deref());

                    match manifest_result {
                        Ok(Some(content_dir)) => {
                            if let Some(engine_dir) = engine_exe.parent() {
                                match merge_content_into(&content_dir, engine_dir) {
                                    Ok(()) => {
                                        content_downloaded = true;
                                        self.push_log(format!(
                                            "Installed game content from manifest into {}",
                                            engine_dir.to_string_lossy()
                                        ));
                                    }
                                    Err(err) => {
                                        self.push_log(format!(
                                            "Failed to merge manifest content into engine dir: {err:#}"
                                        ));
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            self.push_log(
                                "Server has no complete manifest download; trying content ZIP.",
                            );
                            match download_content_zip(
                                &self.paths,
                                address,
                                build,
                                proxy.as_deref(),
                            ) {
                                Ok(Some(content_dir)) => {
                                    if let Some(engine_dir) = engine_exe.parent() {
                                        match merge_content_into(&content_dir, engine_dir) {
                                            Ok(()) => {
                                                content_downloaded = true;
                                                self.push_log(format!(
                                                    "Installed game content from ZIP into {}",
                                                    engine_dir.to_string_lossy()
                                                ));
                                            }
                                            Err(err) => {
                                                self.push_log(format!(
                                                    "Failed to merge ZIP content into engine dir: {err:#}"
                                                ));
                                            }
                                        }
                                    }
                                }
                                Ok(None) => {
                                    self.push_log(
                                        "Server provides neither a manifest download nor a content ZIP URL.",
                                    );
                                }
                                Err(err) => {
                                    self.push_log(format!(
                                        "Game content ZIP download failed: {err:#}"
                                    ));
                                }
                            }
                        }
                        Err(err) => {
                            self.push_log(format!(
                                "Game content manifest download failed: {err:#}"
                            ));
                        }
                    }
                }

                let has_content = if content_downloaded {
                    true
                } else {
                    engine_exe.parent().map(Self::has_game_content).unwrap_or(false)
                };

                if !has_content {
                    self.set_status(self.t("status.no_game_content", &[]));
                    self.push_log(self.feedback_status());
                    return;
                }

                self.push_log(if content_downloaded {
                    self.t("status.game_content_present", &[])
                } else {
                    self.t("status.game_content_present_nm", &[])
                });

                let launched_msg = self.t("status.launched", &[&runtime_cfg.game_executable]);
                self.reconnect_loop(
                    || launch_game_with_context(&runtime_cfg, Some(address), Some(&info)),
                    launched_msg,
                );
            }
            Err(err) => {
                self.push_log(format!(
                    "Engine fallback download failed for {}: {err:#}",
                    engine_version
                ));
            }
        }
    }

    fn try_launch_via_loader(
        &self,
        address: &str,
        info: &ServerInfo,
        engine_version: &str,
    ) -> anyhow::Result<bool> {
        if self.is_cancelled() {
            self.set_status(self.t("status.cancelled", &[]));
            self.push_log(self.feedback_status());
            return Ok(false);
        }

        let proxy = self.hub_options().proxy_url;

        self.set_progress(0.1, self.t("progress.loader", &[]));
        let loader = ensure_loader_installed(&self.paths, proxy.as_deref())?;

        self.set_progress(0.3, self.t("progress.engine", &[]));
        let (engine_zip, signature) =
            download_engine_zip_for_loader(&self.paths, engine_version, proxy.as_deref())?;

        let mut modules = Vec::new();
        if let Ok(module_dir) = download_engine_module_for_engine_version(
            &self.paths,
            "Robust.Client.WebView",
            engine_version,
            proxy.as_deref(),
        ) {
            modules.push(("Robust.Client.WebView".to_string(), module_dir.clone()));
            modules.push(("SpaceWizards.Sdl".to_string(), module_dir));
        }

        let content_db_path = self.paths.clients_dir.join("content.db");
        let mut content_db = content_db_path.clone();
        let mut content_version_id: i64 = 0;
        let mut have_content = false;

        if let Some(build) = &info.build {
            self.set_progress(0.5, self.t("progress.content", &[]));
            match download_content_entries(&self.paths, address, build, proxy.as_deref()) {
                Ok(Some((cache_dir, files, manifest_hash))) => {
                    self.set_progress(0.75, self.t("progress.content_db", &[]));
                    let entries: Vec<ContentDbEntry> = files
                        .into_iter()
                        .map(|f| ContentDbEntry {
                            path: f.path,
                            hash_hex: f.hash_hex,
                        })
                        .collect();
                    let db = build_content_database(
                        &content_db_path,
                        &cache_dir,
                        &entries,
                        &manifest_hash,
                        build.fork_id.as_deref(),
                        build.version.as_deref(),
                        engine_version,
                    )?;
                    content_db = db.path;
                    content_version_id = db.version_id;
                    have_content = true;
                }
                Ok(None) => {
                    return Ok(false);
                }
                Err(err) => return Err(err),
            }
        }

        if !have_content {
            return Ok(false);
        }

        self.set_progress(0.9, self.t("progress.launch", &[]));

        self.push_log(format!(
            "Launching SS14.Loader for engine_version={engine_version} (signature={})",
            if signature.is_empty() { "none" } else { "present" },
        ));

        let mut build_cvars = Vec::new();
        if let Some(build) = &info.build {
            for (key, value) in [
                ("download_url", build.download_url.as_deref()),
                ("manifest_url", build.manifest_url.as_deref()),
                ("manifest_download_url", build.manifest_download_url.as_deref()),
                ("version", build.version.as_deref()),
                ("fork_id", build.fork_id.as_deref()),
                ("hash", build.hash.as_deref()),
                ("manifest_hash", build.manifest_hash.as_deref()),
                ("engine_version", build.engine_version.as_deref()),
            ] {
                if let Some(value) = value {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        build_cvars.push((key.to_string(), trimmed.to_string()));
                    }
                }
            }
        }

        let connect_uri =
            derive_connect_address(address, info).unwrap_or_else(|_| address.to_string());

        let mut auth_token = None;
        let mut auth_userid = None;
        let mut auth_server = None;
        let mut auth_pubkey = None;
        let mut username = None;
        let disable_signing = signature.is_empty();
        if let Some(account) = active_account_for_auth(&self.cfg, &self.cfg.auth_server_url) {
            if !auth_mode_disabled(&info.auth) {
                username = Some(account.username.trim().to_string());
                auth_token = Some(account.token.trim().to_string());
                auth_userid = Some(account.user_id.trim().to_string());
                auth_server = Some(normalize_base_url(&account.auth_server));
                if let Some(pk) = &info.auth.public_key {
                    if !pk.trim().is_empty() {
                        auth_pubkey = Some(pk.trim().to_string());
                    }
                }
            }
        }

        // Apply the configured HWID policy so the loader-launched client uses
        // the desired hardware identifier.
        apply_hwid_for_launch(&self.cfg);

        let spec = LoaderLaunchSpec {
            engine_zip,
            engine_signature: signature,
            signing_key: loader.signing_key,
            loader_exe: loader.loader_exe,
            content_db,
            content_version_id,
            overlay_zip: None,
            modules,
            connect_uri: Some(connect_uri),
            connect_ss14_address: Some(address.to_string()),
            build_cvars,
            username,
            compat_mode: false,
            extra_args: Vec::new(),
            auth_token,
            auth_userid,
            auth_server,
            auth_pubkey,
            disable_signing,
        };

        let mut child = match launch_game_via_loader(&spec) {
            Ok(child) => child,
            Err(err) => return Err(err),
        };

        loop {
            // The game process has spawned; hide the progress overlay
            // immediately so the loading bar does not linger over the
            // launched client window while it is running.
            self.clear_progress();

            let _ = child.wait();
            if self.is_cancelled() || !self.cfg.auto_reconnect {
                break;
            }
            let delay_ms = self.cfg.auto_reconnect_delay_ms;
            self.set_status(self.t("status.reconnect_delay", &[&delay_ms.to_string()]));
            self.push_log(self.feedback_status());
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if self.is_cancelled() {
                break;
            }
            match launch_game_via_loader(&spec) {
                Ok(new_child) => {
                    child = new_child;
                    self.set_status(self.t("status.reconnecting", &[]));
                    self.push_log(self.feedback_status());
                }
                Err(err) => {
                    self.push_log(format!("Reconnect launch failed: {err:#}"));
                    break;
                }
            }
        }

        Ok(true)
    }

    fn has_game_content(exe_dir: &Path) -> bool {
        const CONTENT_MARKERS: [&str; 3] = ["SS14.Loader", "Content.Client.dll", "content.ftl"];
        for name in CONTENT_MARKERS {
            if exe_dir.join(name).exists() {
                return true;
            }
        }
        if exe_dir.join("Resources").join("Content").is_dir() {
            return true;
        }
        false
    }
}
