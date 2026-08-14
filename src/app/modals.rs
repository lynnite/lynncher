use eframe::egui;

use crate::backend::{authenticate_account_with_proxy, normalize_base_url, remove_account, upsert_account};

use super::LauncherApp;

impl LauncherApp {
    pub(super) fn draw_account_menu(&mut self, ui: &mut egui::Ui) {
        let active_label = self.active_account_label();
        let account_items: Vec<(String, String)> = self
            .cfg
            .accounts
            .iter()
            .map(|account| {
                (
                    crate::backend::account_key(&account.auth_server, &account.user_id),
                    format!("{} @ {}", account.username, account.auth_server),
                )
            })
            .collect();

        ui.menu_button(format!("Account: {active_label}"), |ui| {
            ui.label("Auth server");
            ui.text_edit_singleline(&mut self.cfg.auth_server_url);

            ui.separator();
            if ui.selectable_label(self.cfg.active_account_key.is_none(), "No account").clicked() {
                self.cfg.active_account_key = None;
            }
            for (key, label) in &account_items {
                let selected = self.cfg.active_account_key.as_deref() == Some(key.as_str());
                if ui.selectable_label(selected, label).clicked() {
                    self.cfg.active_account_key = Some(key.clone());
                }
            }

            ui.separator();
            if ui.button("Add account").clicked() {
                self.show_add_account_modal = true;
                ui.close_menu();
            }

            let mut remove_key: Option<String> = None;
            for (key, label) in &account_items {
                let hover = egui::RichText::new("Remove").small().weak();
                if ui
                    .add(egui::Button::new(hover).small())
                    .on_hover_text(format!("Remove saved account {label}"))
                    .clicked()
                {
                    remove_key = Some(key.clone());
                }
            }
            if let Some(key) = remove_key {
                remove_account(&mut self.cfg, &key);
                self.status = format!("Removed saved account");
                self.push_log(self.status.clone());
            }
        });
    }

    pub(super) fn render_add_account_modal(&mut self, ctx: &egui::Context) {
        if !self.show_add_account_modal {
            return;
        }

        let mut open = self.show_add_account_modal;
        let mut close_requested = false;
        egui::Window::new("add_account_modal")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let close_btn = egui::Button::new(egui::RichText::new("x").size(9.0))
                        .min_size(egui::vec2(16.0, 16.0))
                        .rounding(8.0)
                        .fill(egui::Color32::from_rgb(22, 22, 22));
                    if ui.add(close_btn).clicked() {
                        close_requested = true;
                    }
                });

                ui.label("Auth server");
                ui.text_edit_singleline(&mut self.cfg.auth_server_url);
                ui.label("Username");
                ui.text_edit_singleline(&mut self.login_username);
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(&mut self.login_password).password(true));

                if ui.button("Add account").clicked() {
                    self.cfg.auth_server_url = normalize_base_url(&self.cfg.auth_server_url);
                    let proxy = self.hub_options().proxy_url;
                    match authenticate_account_with_proxy(
                        &self.cfg.auth_server_url,
                        self.login_username.trim(),
                        self.login_password.as_str(),
                        proxy.as_deref(),
                    ) {
                        Ok(account) => {
                            let key = upsert_account(&mut self.cfg, account);
                            self.cfg.active_account_key = Some(key);
                            self.login_password.clear();
                            self.status = String::from("Authentication succeeded");
                            self.push_log(self.status.clone());
                            self.show_add_account_modal = false;
                            close_requested = true;
                        }
                        Err(err) => {
                            self.status = format!("Authentication failed: {err:#}");
                            self.push_log(self.status.clone());
                        }
                    }
                }
            });
        if close_requested {
            open = false;
        }
        self.show_add_account_modal = open;
    }

    pub(super) fn render_proxy_modal(&mut self, ctx: &egui::Context) {
        if !self.show_proxy_modal {
            return;
        }

        let mut open = self.show_proxy_modal;
        let mut close_requested = false;
        egui::Window::new("proxy_settings_modal")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let close_btn = egui::Button::new(egui::RichText::new("x").size(9.0))
                        .min_size(egui::vec2(16.0, 16.0))
                        .rounding(8.0)
                        .fill(egui::Color32::from_rgb(22, 22, 22));
                    if ui.add(close_btn).clicked() {
                        close_requested = true;
                    }
                });

                ui.label("Proxy URL");
                ui.text_edit_singleline(&mut self.cfg.proxy_url);
                ui.label("Preset URL");
                ui.text_edit_singleline(&mut self.new_proxy_preset);

                if ui.button("Save preset").clicked() {
                    let preset = self.new_proxy_preset.trim().to_string();
                    if !preset.is_empty() && !self.cfg.proxy_presets.iter().any(|p| p == &preset) {
                        self.cfg.proxy_presets.push(preset.clone());
                        self.cfg.proxy_url = preset;
                        self.new_proxy_preset.clear();
                    }
                }
            });
        if close_requested {
            open = false;
        }
        self.show_proxy_modal = open;
    }

    pub(super) fn render_rename_modal(&mut self, ctx: &egui::Context) {
        let Some(address) = self.show_rename_modal.clone() else {
            return;
        };

        let mut open = self.show_rename_modal.is_some();
        let mut close_requested = false;
        let mut save_requested = false;
        let mut clear_requested = false;

        egui::Window::new("rename_favorite_modal")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(
                    egui::RichText::new("Rename favorite")
                        .strong()
                        .size(16.0)
                        .color(egui::Color32::from_rgb(0xD5, 0xD5, 0xD5)),
                );
                ui.add_space(6.0);
                ui.label("Custom name for this server (shown only in the favorites list):");
                ui.add_space(4.0);
                let default_name = self.favorite_display_name(&address);
                let buf = self
                    .favorite_name_inputs
                    .entry(address.clone())
                    .or_insert(default_name);
                ui.add(
                    egui::TextEdit::singleline(buf)
                        .hint_text("custom name")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_requested = true;
                    }
                    if ui.button("Clear").clicked() {
                        clear_requested = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close_requested = true;
                    }
                });
            });

        if save_requested {
            let name = self
                .favorite_name_inputs
                .get(&address)
                .cloned()
                .unwrap_or_default();
            self.set_favorite_name(&address, &name);
            close_requested = true;
        }
        if clear_requested {
            self.set_favorite_name(&address, "");
            self.favorite_name_inputs
                .entry(address.clone())
                .or_default();
            close_requested = true;
        }
        if close_requested {
            self.show_rename_modal = None;
        }
        let _ = open;
    }
}
