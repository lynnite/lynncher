use eframe::egui;

use crate::backend::{
    account_key, check_latest_release, download_and_apply_update, fetch_hub_servers_with_options,
    fetch_server_info_direct_with_proxy, fetch_server_info_from_hub_with_options, is_newer_tag,
    latest_release_url, normalize_base_url, save_config, HubRequestOptions, ServerInfo,
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

    pub(crate) fn start_update(&mut self) {
        let proxy = self
            .cfg
            .proxy_enabled
            .then(|| self.cfg.proxy_url.trim().to_string())
            .filter(|s| !s.is_empty());

        self.status = String::from("Downloading the compiled update for this OS...");
        self.push_log(self.status.clone());

        let result = self.update_action_result.clone();
        let proxy_clone = proxy;

        std::thread::spawn(move || {
            let msg = match download_and_apply_update(proxy_clone.as_deref()) {
                Ok(()) => {
                    String::from("Launcher updated successfully. Restart the launcher.")
                }
                Err(err) => format!("Launcher update: {err:#}"),
            };
            if let Ok(mut r) = result.lock() {
                *r = Some(msg);
            }
        });
    }

    pub(crate) fn poll_update_action(&mut self) {
        let msg = {
            let mut r = self.update_action_result.lock().unwrap_or_else(|e| e.into_inner());
            r.take()
        };
        if let Some(msg) = msg {
            self.status = msg.clone();
            self.push_log(msg);
        }
    }

    pub(crate) fn update_available(&self) -> Option<bool> {
        let s = self.update_check.lock().unwrap_or_else(|e| e.into_inner());
        if !s.done {
            return None;
        }
        match &s.version {
            Some(tag) => Some(is_newer_tag(tag, super::APP_VERSION)),
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

    pub(crate) fn ensure_favorite_infos(&mut self) {
        if self.cfg.favorite_servers.is_empty() {
            return;
        }
        let options = self.hub_options();
        for address in self.cfg.favorite_servers.clone() {
            if self.favorite_infos.contains_key(&address) {
                continue;
            }
            let info = fetch_server_info_from_hub_with_options(
                &self.cfg.hub_server_url,
                &address,
                options.clone(),
            )
            .or_else(|_| fetch_server_info_direct_with_proxy(&address, options.proxy_url.as_deref()));
            if let Ok(info) = info {
                self.favorite_infos.insert(address.clone(), info);
            }
        }
    }

    pub(crate) fn favorite_summary(&self, address: &str) -> (String, String) {
        let custom = self.favorite_display_name(address);
        let custom_is_addr = custom.eq_ignore_ascii_case(address);

        let hub_name = self
            .servers
            .iter()
            .find(|s| s.address.eq_ignore_ascii_case(address))
            .and_then(|s| s.status_data.name.as_deref())
            .filter(|n| !n.trim().is_empty());

        let name = if !custom_is_addr {
            custom
        } else if let Some(n) = hub_name {
            n.to_string()
        } else {
            address.to_string()
        };

        let desc = self
            .favorite_infos
            .get(address)
            .and_then(|i| i.desc.clone())
            .unwrap_or_default();

        (name, desc)
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

    pub(crate) fn save_config_if_dirty(&mut self) {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_config_save {
            if now.duration_since(last) < std::time::Duration::from_millis(1500) {
                return;
            }
        }
        let Ok(serialized) = toml::to_string_pretty(&self.cfg) else {
            self.last_config_save = Some(now);
            return;
        };
        if self.last_saved_config.as_deref() == Some(serialized.as_str()) {
            self.last_config_save = Some(now);
            return;
        }
        if save_config(&self.paths, &self.cfg).is_ok() {
            self.last_saved_config = Some(serialized);
        }
        self.last_config_save = Some(now);
    }

    pub(crate) fn run_auto_update(&mut self) {
        if !self.cfg.auto_update || self.auto_update_initiated {
            return;
        }

        {
            let state = self.update_check.lock().unwrap_or_else(|e| e.into_inner());
            if !state.done {
                if !state.checking {
                    drop(state);
                    self.start_update_check();
                }
                return;
            }
        }

        self.auto_update_initiated = true;
        if self.update_available() == Some(true) {
            self.push_log("Automatic update available; downloading at startup.");
            self.start_update();
        }
    }
}
