use eframe::egui;
use std::path::Path;
use std::time::SystemTime;
use walkdir::WalkDir;

use crate::backend::{
    account_key, apply_acz_inferred_urls, check_latest_release, download_and_install_content,
    download_client_for_server_with_proxy_and_tokens, download_content_zip,
    download_engine_client_for_version, download_engine_module_for_engine_version,
    fetch_hub_servers_with_options, fetch_server_info_direct_with_proxy,
    fetch_server_info_from_hub_with_options, is_newer_tag, latest_release_url,
    launch_game_with_context, merge_content_into, normalize_base_url, stage_sdl3_native_runtime,
    HubRequestOptions, ServerInfo,
};

use super::LauncherApp;

impl LauncherApp {
    pub(crate) fn apply_flat_style(&mut self, ctx: &egui::Context) {
        let cs = self.cfg.color_scheme.clone();

        let bg = egui::Color32::from_rgb(cs.bg_r, cs.bg_g, cs.bg_b);
        let popup = egui::Color32::from_rgb(cs.popup_r, cs.popup_g, cs.popup_b);
        let fg = egui::Color32::from_rgb(cs.text_r, cs.text_g, cs.text_b);
        let sub_text = egui::Color32::from_rgb(cs.sub_text_r, cs.sub_text_g, cs.sub_text_b);
        let button = egui::Color32::from_rgb(cs.button_r, cs.button_g, cs.button_b);
        let hover = egui::Color32::from_rgb(cs.hover_r, cs.hover_g, cs.hover_b);
        let accent = egui::Color32::from_rgb(cs.accent_r, cs.accent_g, cs.accent_b);

        if self.style_applied && self.last_scheme.as_ref() == Some(&self.cfg.color_scheme) {
            return;
        }
        self.style_applied = true;
        self.last_scheme = Some(cs.clone());

        let mut style = (*ctx.style()).clone();

        style.visuals.override_text_color = Some(fg);

        style.visuals.window_rounding = 0.0.into();
        style.visuals.menu_rounding = 0.0.into();
        style.visuals.widgets.noninteractive.rounding = 0.0.into();
        style.visuals.widgets.inactive.rounding = 0.0.into();
        style.visuals.widgets.hovered.rounding = 0.0.into();
        style.visuals.widgets.active.rounding = 0.0.into();
        style.visuals.widgets.open.rounding = 0.0.into();

        style.visuals.window_shadow = egui::epaint::Shadow::NONE;
        style.visuals.popup_shadow = egui::epaint::Shadow::NONE;

        style.visuals.panel_fill = bg;
        style.visuals.window_fill = popup;
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(
            cs.footer_r,
            cs.footer_g,
            cs.footer_b,
        );
        style.visuals.faint_bg_color = egui::Color32::from_rgb(cs.item_r, cs.item_g, cs.item_b);
        style.visuals.code_bg_color = popup;

        style.visuals.widgets.noninteractive.bg_fill = bg;
        style.visuals.widgets.inactive.bg_fill = button;
        style.visuals.widgets.hovered.bg_fill = hover;
        style.visuals.widgets.active.bg_fill = accent;
        style.visuals.widgets.open.bg_fill = button;
        style.visuals.widgets.inactive.weak_bg_fill = button;

        style.visuals.widgets.inactive.fg_stroke.color = fg;
        style.visuals.widgets.hovered.fg_stroke.color = fg;
        style.visuals.widgets.active.fg_stroke.color = fg;
        style.visuals.widgets.noninteractive.fg_stroke.color = sub_text;

        style.visuals.widgets.noninteractive.bg_stroke.width = 0.0;
        style.visuals.widgets.inactive.bg_stroke.width = 0.0;
        style.visuals.widgets.active.bg_stroke.width = 0.0;
        style.visuals.widgets.hovered.bg_stroke.width = 0.0;
        style.visuals.window_stroke.width = 1.0;
        style.visuals.window_stroke.color = button;

        style.visuals.selection.bg_fill = accent;
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, fg);
        style.visuals.hyperlink_color = fg;
        style.visuals.text_cursor.stroke.color = fg;

        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.indent = 12.0;

        ctx.set_style(style);
    }

    pub(crate) fn push_log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > 200 {
            let keep_from = self.logs.len().saturating_sub(200);
            self.logs.drain(0..keep_from);
        }
    }

    pub(crate) fn active_account_label(&self) -> String {
        let active = self.cfg.active_account_key.as_deref();
        match active {
            Some(key) => self
                .cfg
                .accounts
                .iter()
                .find(|acc| account_key(&acc.auth_server, &acc.user_id) == key)
                .map(|acc| format!("{} @ {}", acc.username, acc.auth_server))
                .unwrap_or_else(|| String::from("Unknown account")),
            None => String::from("None"),
        }
    }

    pub(crate) fn hub_options(&self) -> HubRequestOptions {
        HubRequestOptions {
            proxy_url: if self.cfg.proxy_enabled && !self.cfg.proxy_url.trim().is_empty() {
                Some(self.cfg.proxy_url.trim().to_string())
            } else {
                None
            },
        }
    }

    pub(crate) fn start_update_check(&mut self) {
        {
            let mut state = self.update_check.lock().unwrap_or_else(|e| e.into_inner());
            if state.checking {
                return;
            }
            state.checking = true;
            state.done = false;
            state.version = None;
            state.url = None;
            state.error = None;
        }

        let proxy = self
            .cfg
            .proxy_enabled
            .then(|| self.cfg.proxy_url.trim().to_string())
            .filter(|s| !s.is_empty());
        let state = self.update_check.clone();
        std::thread::spawn(move || {
            let result = check_latest_release(proxy.as_deref());
            if let Ok(mut s) = state.lock() {
                match result {
                    Ok(Some(tag)) => {
                        s.version = Some(tag);
                        s.url = latest_release_url(proxy.as_deref()).ok().flatten();
                    }
                    Ok(None) => {
                        s.error = Some(String::from("Could not determine latest release"));
                    }
                    Err(err) => {
                        s.error = Some(format!("Update check failed: {err:#}"));
                    }
                }
                s.checking = false;
                s.done = true;
            }
        });
    }

    pub(crate) fn start_update(&mut self, ctx: &egui::Context) {
        let url = self
            .update_check
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .url
            .clone()
            .unwrap_or_else(|| String::from("https://github.com/lynnite/lynncher/releases/latest"));
        ctx.open_url(egui::output::OpenUrl {
            url,
            new_tab: true,
        });
        self.status = String::from("Opening the latest release page to update the launcher");
        self.push_log(self.status.clone());
    }

    pub(crate) fn update_available(&self) -> Option<bool> {
        let s = self.update_check.lock().unwrap_or_else(|e| e.into_inner());
        if !s.done {
            return None;
        }
        match &s.version {
            Some(tag) => Some(is_newer_tag(tag, env!("CARGO_PKG_VERSION"))),
            None => None,
        }
    }

    pub(crate) fn latest_release_label(&self) -> Option<String> {
        self.update_check
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .version
            .clone()
    }

    pub(crate) fn release_check_error(&self) -> Option<String> {
        self.update_check
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error
            .clone()
    }

    pub(crate) fn release_checking(&self) -> bool {
        self.update_check
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .checking
    }

    pub(crate) fn release_check_done(&self) -> bool {
        self.update_check
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .done
    }

    pub(crate) fn refresh_hub_servers(&mut self) {
        self.cfg.hub_server_url = normalize_base_url(&self.cfg.hub_server_url);
        let options = self.hub_options();
        match fetch_hub_servers_with_options(&self.cfg.hub_server_url, options) {
            Ok(mut list) => {
                list.sort_by(|a, b| b.status_data.players.cmp(&a.status_data.players));
                self.servers = list;
                self.status = format!("Loaded {} servers from hub", self.servers.len());
                self.push_log(self.status.clone());
            }
            Err(err) => {
                self.status = format!("Failed to load hub servers: {err:#}");
                self.push_log(self.status.clone());
            }
        }
    }

    #[allow(dead_code, unused_variables, unused_mut, unused_assignments)]
    pub(crate) fn connect_with_server_info(&mut self, address: &str, mut info: ServerInfo) {
        if let Some(build) = &info.build {
            info.build = apply_acz_inferred_urls(address, build).ok().or(info.build);
        }

        let proxy = self.hub_options().proxy_url;
        let mut download_err: Option<anyhow::Error> = None;

        let auth_token_strings = self.download_auth_tokens();
        let auth_tokens: Vec<Option<&str>> = auth_token_strings
            .iter()
            .map(|t| t.as_deref())
            .collect();

        match download_client_for_server_with_proxy_and_tokens(
            &self.paths,
            address,
            &info,
            proxy.as_deref(),
            &auth_tokens,
            None,
        ) {
            Ok(install) => {
                self.status = format!("Client downloaded to {}", install.install_dir.to_string_lossy());
                self.push_log(self.status.clone());

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

                match launch_game_with_context(&runtime_cfg, Some(address), Some(&info)) {
                    Ok(msg) => {
                        self.status = msg;
                        self.push_log(self.status.clone());
                    }
                    Err(err) => {
                        self.status = format!("Launch failed: {err:#}");
                        self.push_log(self.status.clone());
                    }
                }

                return;
            }
            Err(err) => {
                download_err = Some(err);
            }
        }

        if let Some(fallback_exe) = self.resolve_fallback_executable() {
            let mut runtime_cfg = self.cfg.clone();
            runtime_cfg.game_executable = fallback_exe;

            self.status = String::from(
                "ZIP endpoints unavailable; falling back to local client executable",
            );
            self.push_log(self.status.clone());

            if let Some(build) = &info.build {
                if build.manifest_download_url.is_some() || build.manifest_url.is_some() {
                    self.push_log(
                        "Server appears to use manifest/content protocol; using local client with build metadata",
                    );
                }
            }

            let has_content = Path::new(&runtime_cfg.game_executable)
                .parent()
                .map(Self::has_game_content)
                .unwrap_or(false);

            if !has_content {
                self.push_log(
                    "Local fallback client has no game content; skipping it and trying other paths.",
                );
            } else {
                match launch_game_with_context(&runtime_cfg, Some(address), Some(&info)) {
                    Ok(msg) => {
                        self.status = msg;
                        self.push_log(self.status.clone());
                        return;
                    }
                    Err(err) => {
                        self.status = format!("Fallback launch failed: {err:#}");
                        self.push_log(self.status.clone());
                    }
                }
            }
        }

        if let Some(build) = &info.build {
            if let Some(engine_version) = build.engine_version.as_deref() {
                let proxy = self.hub_options().proxy_url;

                match self.try_launch_via_loader(address, &info, engine_version) {
                    Ok(true) => {
                        self.status = String::from("Launched client through SS14.Loader");
                        self.push_log(self.status.clone());
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
                            self.push_log(format!(
                                "Prepared native module fallback at {}",
                                module_dir
                            ));
                        }

                        let mut runtime_cfg = self.cfg.clone();
                        runtime_cfg.game_executable = engine_exe.to_string_lossy().into_owned();
                        self.status = format!(
                            "Downloaded engine {} as fallback client",
                            engine_version
                        );
                        self.push_log(self.status.clone());

                        let mut content_downloaded = false;
                        if let Some(build) = &info.build {
                            let manifest_result = download_and_install_content(
                                &self.paths,
                                address,
                                build,
                                proxy.as_deref(),
                            );

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
                            engine_exe
                                .parent()
                                .map(Self::has_game_content)
                                .unwrap_or(false)
                        };

                        if !has_content {
                            self.status = String::from(
                                "Engine fallback client has no game content installed; \
                                 cannot launch (bare Robust engine cannot boot to the game). \
                                 If the server uses the manifest/content-download protocol, \
                                 the content download failed. Check the log above.",
                            );
                            self.push_log(self.status.clone());
                            return;
                        }

                        self.push_log(
                            if content_downloaded {
                                String::from("Game content present; launching client.")
                            } else {
                                String::from(
                                    "Game content present (no manifest required); launching client.",
                                )
                            },
                        );

                        match launch_game_with_context(&runtime_cfg, Some(address), Some(&info)) {
                            Ok(msg) => {
                                self.status = msg;
                                self.push_log(self.status.clone());
                                return;
                            }
                            Err(err) => {
                                self.status = format!("Engine fallback launch failed: {err:#}");
                                self.push_log(self.status.clone());
                            }
                        }
                    }
                    Err(err) => {
                        self.push_log(format!(
                            "Engine fallback download failed for {}: {err:#}",
                            engine_version
                        ));
                    }
                }
            }
        }

        if let Some(err) = download_err {
            self.status = format!("Client download failed: {err:#}");
            self.push_log(self.status.clone());
        } else {
            self.status = String::from("Client download failed: no attempts executed");
            self.push_log(self.status.clone());
        }
    }

    pub(crate) fn start_background_connect(&mut self, address: &str, info: ServerInfo) {
        let paths = self.paths.clone();
        let cfg = self.cfg.clone();
        let feedback = super::worker::new_feedback();
        let fb = feedback.clone();
        let addr = address.to_string();

        let handle = std::thread::spawn(move || {
            let connector = super::worker::Connector::new(paths, cfg, fb);
            connector.connect(&addr, info);
        });

        self.background = Some(super::worker::BackgroundWork {
            feedback,
            thread: Some(handle),
        });
        self.poll_background();
    }

    pub(crate) fn cancel_connection(&mut self) {
        if let Some(work) = &self.background {
            if let Ok(fb) = work.feedback.lock() {
                fb.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        self.status = String::from("Cancelling connection and cleaning up partial downloads...");
        self.push_log(self.status.clone());
    }

    pub(crate) fn connection_active(&self) -> bool {
        self.background.is_some()
    }

    pub(crate) fn poll_background(&mut self) {
        let (logs, status, progress, running) = {
            let Some(work) = &self.background else {
                return;
            };
            let mut fb = work.feedback.lock().unwrap_or_else(|e| e.into_inner());
            (
                std::mem::take(&mut fb.logs),
                fb.status.clone(),
                fb.progress.clone(),
                fb.running,
            )
        };

        for log in logs {
            self.push_log(log);
        }

        if !status.is_empty() {
            self.status = status;
        }

        self.progress = progress.map(|(f, label)| super::ProgressState {
            fraction: f,
            label,
        });

        if !running {
            if let Some(mut work) = self.background.take() {
                if let Some(handle) = work.thread.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    #[allow(dead_code)]
    fn download_auth_tokens(&self) -> Vec<Option<String>> {
        let mut out: Vec<Option<String>> = Vec::new();

        if let Some(active_key) = self.cfg.active_account_key.as_deref() {            if let Some(active) = self
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

    #[allow(dead_code)]
    fn resolve_fallback_executable(&self) -> Option<String> {
        let configured = self.cfg.game_executable.trim();
        if !configured.is_empty() && Path::new(configured).exists() {
            return Some(configured.to_string());
        }

        Self::find_newest_cached_client_executable(&self.paths.clients_dir)
            .map(|p| p.to_string_lossy().into_owned())
    }


#[allow(dead_code)]
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

#[allow(dead_code)]
fn find_newest_cached_client_executable(clients_dir: &Path) -> Option<std::path::PathBuf> {
    let candidates = [
        "SS14.Loader",
        "SS14.Loader.exe",
        "Robust.Client",
        "Robust.Client.exe",
    ];

    let mut best: Option<(SystemTime, std::path::PathBuf)> = None;

    for entry in WalkDir::new(clients_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if !candidates.iter().any(|c| file_name.eq_ignore_ascii_case(c)) {
            continue;
        }

        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        match &best {
            Some((best_time, _)) if modified <= *best_time => {}
            _ => best = Some((modified, entry.path().to_path_buf())),
        }
    }

    best.map(|(_, p)| p)
}
    pub(crate) fn connect_via_hub(&mut self, address: &str) {
        self.cfg.hub_server_url = normalize_base_url(&self.cfg.hub_server_url);
        let options = self.hub_options();
        match fetch_server_info_from_hub_with_options(&self.cfg.hub_server_url, address, options) {
            Ok(info) => self.start_background_connect(address, info),
            Err(err) => {
                self.status = format!("Failed to get server info from hub: {err:#}");
                self.push_log(self.status.clone());
            }
        }
    }

    pub(crate) fn connect_direct(&mut self, target: &str) {
        let proxy = self.hub_options().proxy_url;
        match fetch_server_info_direct_with_proxy(target, proxy.as_deref()) {
            Ok(info) => self.start_background_connect(target, info),
            Err(err) => {
                self.status = format!("Direct server info failed: {err:#}");
                self.push_log(self.status.clone());
            }
        }
    }

    pub(crate) fn is_favorite(&self, address: &str) -> bool {
        self.cfg
            .favorite_servers
            .iter()
            .any(|f| f.eq_ignore_ascii_case(address))
    }

    pub(crate) fn toggle_favorite(&mut self, address: &str) {
        if let Some(idx) = self
            .cfg
            .favorite_servers
            .iter()
            .position(|f| f.eq_ignore_ascii_case(address))
        {
            self.cfg.favorite_servers.remove(idx);
            self.cfg.favorite_names.retain(|(a, _)| a != address);
            self.status = format!("Removed favorite: {address}");
            self.push_log(self.status.clone());
        } else {
            self.cfg.favorite_servers.push(address.to_string());
            self.status = format!("Added favorite: {address}");
            self.push_log(self.status.clone());
        }
    }

    pub(crate) fn favorite_display_name(&self, address: &str) -> String {
        self.cfg
            .favorite_names
            .iter()
            .find(|(a, _)| a == address)
            .map(|(_, name)| name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| address.to_string())
    }

    pub(crate) fn set_favorite_name(&mut self, address: &str, name: &str) {
        let trimmed = name.trim();
        if let Some(pair) = self
            .cfg
            .favorite_names
            .iter_mut()
            .find(|(a, _)| a == address)
        {
            pair.1 = trimmed.to_string();
        } else if !trimmed.is_empty() {
            self.cfg
                .favorite_names
                .push((address.to_string(), trimmed.to_string()));
        }
    }

    pub(crate) fn clear_installed_engines(&mut self) {
        let clients = &self.paths.clients_dir;
        let mut removed = 0usize;
        if let Ok(entries) = std::fs::read_dir(clients) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_engine_dir = name.starts_with("engine-")
                    || name == "engine-modules"
                    || name == "native-packages";
                if is_engine_dir && entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                    if std::fs::remove_dir_all(entry.path()).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        let _ = std::fs::remove_file(clients.join("robust-manifest.json"));
        self.status = format!("Cleared installed engines ({removed} items)");
        self.push_log(self.status.clone());
    }

    pub(crate) fn clear_installed_server_content(&mut self) {
        let clients = &self.paths.clients_dir;
        let mut removed = 0usize;
        if let Ok(entries) = std::fs::read_dir(clients) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_content_dir = name.starts_with("content-") || name == "content-cache";
                if is_content_dir && entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                    if std::fs::remove_dir_all(entry.path()).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        let _ = std::fs::remove_file(clients.join("content.db"));
        self.status = format!("Cleared installed server content ({removed} items)");
        self.push_log(self.status.clone());
    }
}
