use eframe::egui;

use crate::backend::{
    fetch_server_info_from_hub_with_options, list_sideloaded_extensions, load_config_from_path,
    normalize_base_url, remove_sideloaded_extension, save_config, sideload_extension_bundle,
    ColorScheme, HubServerEntry, ServerInfo,
};

use super::LauncherApp;

const GOLD: egui::Color32 = egui::Color32::from_rgb(0xC8, 0xC8, 0xC8);
const SUB_TEXT: egui::Color32 = egui::Color32::from_rgb(0x8F, 0x8F, 0x8F);

pub(crate) fn nano_heading(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(
        egui::RichText::new(text.into())
            .strong()
            .size(16.0)
            .color(GOLD),
    );
}



fn format_round_time(server: &HubServerEntry) -> String {
    let Some(start) = server.status_data.round_start_time.as_deref() else {
        return String::from("—");
    };
    if let Some(run) = &server.status_data.run_level {
        let is_round = match run {
            serde_json::Value::String(s) => s.eq_ignore_ascii_case("Round"),
            serde_json::Value::Number(n) => n.as_i64() == Some(1),
            serde_json::Value::Bool(b) => *b,
            _ => false,
        };
        if !is_round {
            return String::from("Lobby");
        }
    }
    let parse_rfc3339 = chrono::DateTime::parse_from_rfc3339(start)
        .map(|dt| dt.with_timezone(&chrono::Utc));
    let mut parsed_opt = parse_rfc3339.ok();
    if parsed_opt.is_none() {
        let naive = chrono::NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M:%S"));
        if let Ok(naive) = naive {
            parsed_opt =
                Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc));
        }
    }
    let Some(start_utc) = parsed_opt else {
        return String::from("—");
    };
    let now_utc = chrono::Utc::now();
    let elapsed = (now_utc.signed_duration_since(start_utc)).num_seconds().max(0) as u64;
    format_duration(elapsed)
}

fn format_duration(total: u64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn collect_distinct_tags(
    servers: &[crate::backend::HubServerEntry],
    prefix: &str,
) -> Vec<(String, String)> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for server in servers {
        if let Some(tags) = &server.status_data.tags {
            for tag in tags {
                let Some(value) = tag.strip_prefix(&format!("{prefix}:")) else {
                    continue;
                };
                let key = format!("{prefix}:{value}");
                let label = tag_label(prefix, value);
                if !seen.iter().any(|(k, _)| k == &key) {
                    seen.push((key, label));
                }
            }
        }
    }
    seen.sort_by(|a, b| a.1.cmp(&b.1));
    seen
}

fn tag_label(prefix: &str, value: &str) -> String {
    match prefix {
        "region" => region_short(value),
        "rp" => match value {
            "none" => String::from("None"),
            "low" => String::from("Low"),
            "medium" => String::from("Medium"),
            "high" => String::from("High"),
            other => other.to_string(),
        },
        "language" => value.to_uppercase(),
        _ => value.to_string(),
    }
}

fn region_short(code: &str) -> String {
    match code {
        "eu_w" => String::from("EU West"),
        "eu_e" => String::from("EU East"),
        "am_n_e" => String::from("NA East"),
        "am_n_c" => String::from("NA Central"),
        "am_n_w" => String::from("NA West"),
        "am_s_e" => String::from("SA East"),
        "am_s_w" => String::from("SA West"),
        "am_s_s" => String::from("SA South"),
        "c_am" => String::from("Central America"),
        "af_c" => String::from("Africa Central"),
        "af_n" => String::from("Africa North"),
        "af_s" => String::from("Africa South"),
        "an" => String::from("Antarctica"),
        "as_e" => String::from("Asia East"),
        "as_s_e" => String::from("Asia Southeast"),
        "as_n" => String::from("Asia North"),
        "in" => String::from("India"),
        "me" => String::from("Middle East"),
        "mo" => String::from("The Moon"),
        "oc" => String::from("Oceania"),
        "gl" => String::from("Greenland"),
        other => other.to_string(),
    }
}

impl LauncherApp {
    pub(super) fn draw_favorites_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("favorites_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                nano_heading(ui, "Favorite servers");
        ui.separator();

        if self.cfg.favorite_servers.is_empty() {
            ui.label("No favorite servers yet.");
        } else {
            let mut connect_target: Option<String> = None;
            let mut remove_target: Option<String> = None;
            let mut rename_target: Option<String> = None;

            self.ensure_favorite_infos();

            egui::ScrollArea::vertical()
                .id_salt("favorites_list")
                .max_height(260.0)
                .show(ui, |ui| {
                let item_col = egui::Color32::from_rgb(
                    self.cfg.color_scheme.item_r,
                    self.cfg.color_scheme.item_g,
                    self.cfg.color_scheme.item_b,
                );
                for address in self.cfg.favorite_servers.clone() {
                    let (name, desc) = self.favorite_summary(&address);
                    egui::Frame::none()
                        .fill(item_col)
                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                        .show(ui, |ui| {
                            let summary = if desc.is_empty() {
                                format!("{}\n{}", name, address)
                            } else {
                                format!("{}\n{}\n{}", name, address, desc)
                            };
                            let resp = ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    if desc.is_empty() {
                                        ui.label(egui::RichText::new(&name).strong());
                                        ui.label(
                                            egui::RichText::new(&address)
                                                .small()
                                                .weak(),
                                        );
                                    } else {
                                        ui.label(egui::RichText::new(&name).strong())
                                            .on_hover_text(&summary);
                                        ui.label(
                                            egui::RichText::new(&desc)
                                                .small()
                                                .weak(),
                                        )
                                        .on_hover_text(&summary);
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("Connect").clicked() {
                                            connect_target = Some(address.clone());
                                        }
                                        if ui.small_button("Rename").clicked() {
                                            rename_target = Some(address.clone());
                                        }
                                        if ui.small_button("Remove").clicked() {
                                            remove_target = Some(address.clone());
                                        }
                                    },
                                );
                            });
                            let _ = resp;
                        });
                    ui.add_space(1.0);
                }
            });

            if let Some(address) = connect_target {
                self.connect_direct(&address);
            }

            if let Some(address) = remove_target {
                self.toggle_favorite(&address);
            }

            if let Some(address) = rename_target {
                let default_name = self.favorite_display_name(&address);
                self.favorite_name_inputs
                    .entry(address.clone())
                    .or_insert(default_name);
                self.show_rename_modal = Some(address);
            }
        }

        ui.add_space(8.0);

        ui.separator();
        ui.label("Event log");
        egui::ScrollArea::vertical()
            .id_salt("event_log")
            .max_height(180.0)
            .show(ui, |ui| {
            if self.logs.is_empty() {
                ui.label("No events yet.");
            } else {
                for line in &self.logs {
                    ui.label(line);
                }
            }
        });
    });
    }

    pub(super) fn draw_hub_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);

        let mut connect_target: Option<String> = None;
        let mut select_target: Option<String> = None;
        let mut clear_selection = false;
        let mut favorite_target: Option<String> = None;
        let selected_address_current = self.selected_server_address.clone();
        let selected_info_current = self.selected_server_info.clone();

        let query = self.hub_search_query.trim().to_lowercase();
        let filtered: Vec<HubServerEntry> = self
            .servers
            .iter()
            .filter(|s| {
                let name = s.status_data.name.as_deref().unwrap_or("");
                if !query.is_empty() {
                    let hay = format!("{} {}", s.address.to_lowercase(), name.to_lowercase());
                    if !hay.contains(&query) {
                        return false;
                    }
                }
                true
                    && self.hub_filter_tags.iter().all(|wanted| {
                        s.status_data
                            .tags
                            .as_deref()
                            .map(|t| t.iter().any(|x| x == wanted))
                            .unwrap_or(false)
                    })
            })
            .cloned()
            .collect();

        let table_h = (ui.available_height() - 12.0).max(120.0);

        if self.hub_filters_visible {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    self.draw_server_filters(ui, filtered.len());
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(ui.available_width());
                    self.draw_server_table(
                        ui,
                        filtered.as_slice(),
                        &selected_address_current,
                        &selected_info_current,
                        table_h,
                        &mut select_target,
                        &mut clear_selection,
                        &mut connect_target,
                        &mut favorite_target,
                    );
                });
            });
        } else {
            self.draw_server_table(
                ui,
                filtered.as_slice(),
                &selected_address_current,
                &selected_info_current,
                table_h,
                &mut select_target,
                &mut clear_selection,
                &mut connect_target,
                &mut favorite_target,
            );
        }

        if let Some(address) = favorite_target {
            self.toggle_favorite(&address);
        }
        if clear_selection {
            self.selected_server_address = None;
            self.selected_server_info = None;
        }
        if let Some(address) = select_target {
            self.load_hub_server_info(&address);
        }
        if let Some(address) = connect_target {
            self.connect_via_hub(&address);
        }
    }

    pub(super) fn draw_hub_search_bar(&mut self, ctx: &egui::Context) {
        if self.page != super::AppPage::Hub {
            return;
        }

        let mut refresh = false;
        let filtered_len = self.filtered_server_count();

        egui::TopBottomPanel::top("hub_search")
            .frame(egui::Frame::default().fill(egui::Color32::from_rgb(
                self.cfg.color_scheme.footer_r,
                self.cfg.color_scheme.footer_g,
                self.cfg.color_scheme.footer_b,
            )))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.hub_search_query)
                            .hint_text("Search For Servers…")
                            .desired_width((ui.available_width() - 180.0).max(120.0)),
                    );
                    let filters_label = format!(
                        "Filters ({}/{})",
                        filtered_len,
                        self.servers.len()
                    );
                    ui.toggle_value(&mut self.hub_filters_visible, filters_label);
                    if ui.button("⟳").on_hover_text("Refresh").clicked() {
                        refresh = true;
                    }
                });
                ui.add_space(4.0);
            });

        if refresh {
            self.refresh_hub_servers();
        }
    }

    fn filtered_server_count(&self) -> usize {
        let query = self.hub_search_query.trim().to_lowercase();
        self.servers
            .iter()
            .filter(|s| {
                let name = s.status_data.name.as_deref().unwrap_or("");
                if !query.is_empty() {
                    let hay = format!("{} {}", s.address.to_lowercase(), name.to_lowercase());
                    if !hay.contains(&query) {
                        return false;
                    }
                }
                true
                    && self.hub_filter_tags.iter().all(|wanted| {
                        s.status_data
                            .tags
                            .as_deref()
                            .map(|t| t.iter().any(|x| x == wanted))
                            .unwrap_or(false)
                    })
            })
            .count()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_server_table(
        &mut self,
        ui: &mut egui::Ui,
        servers: &[HubServerEntry],
        selected_address_current: &Option<String>,
        selected_info_current: &Option<ServerInfo>,
        table_h: f32,
        select_target: &mut Option<String>,
        clear_selection: &mut bool,
        connect_target: &mut Option<String>,
        favorite_target: &mut Option<String>,
    ) {
        egui::ScrollArea::vertical()
            .id_salt("server_table")
            .max_height(table_h)
            .show(ui, |ui| {
            let item_col = egui::Color32::from_rgb(
                self.cfg.color_scheme.item_r,
                self.cfg.color_scheme.item_g,
                self.cfg.color_scheme.item_b,
            );
            let accent_col = egui::Color32::from_rgb(
                self.cfg.color_scheme.accent_r,
                self.cfg.color_scheme.accent_g,
                self.cfg.color_scheme.accent_b,
            );
            if servers.is_empty() {
                let status = if self.servers.is_empty() {
                    "There are no public servers. Ensure your hub configuration is correct. Or try refreshing."
                } else {
                    "No servers match your search or filter settings."
                };
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(status).color(SUB_TEXT));
                });
                return;
            }

            for server in servers {
                let name = server
                    .status_data
                    .name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("Unnamed server");

                let is_selected = selected_address_current
                    .as_deref()
                    .map(|s| s == server.address)
                    .unwrap_or(false);

                let row_bg = if is_selected {
                    accent_col
                } else {
                    item_col
                };

                egui::Frame::none()
                    .fill(row_bg)
                    .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let name_w = (ui.available_width() - 260.0).max(120.0);
                            let click = truncated_label(ui, name, name_w, egui::Color32::from_rgb(0xEE, 0xEE, 0xEE), true);
                            if click.clicked() {
                                if is_selected {
                                    *clear_selection = true;
                                } else {
                                    *select_target = Some(server.address.clone());
                                }
                            }

                            ui.allocate_ui_with_layout(
                                egui::vec2(90.0, 18.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format_round_time(server))
                                            .color(SUB_TEXT),
                                    );
                                },
                            );

                            ui.allocate_ui_with_layout(
                                egui::vec2(90.0, 18.0),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} / {}",
                                            server.status_data.players,
                                            server.status_data.soft_max_players
                                        ))
                                        .strong()
                                        .color(GOLD),
                                    );
                                },
                            );

                            ui.allocate_ui_with_layout(
                                egui::vec2(80.0, 18.0),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Connect").clicked() {
                                        *connect_target = Some(server.address.clone());
                                    }
                                },
                            );
                        });

                        if is_selected {
                            if let Some(info) = selected_info_current {
                                ui.separator();
                                ui.label(format!(
                                    "Description: {}",
                                    info.desc.clone().unwrap_or_else(|| String::from("No description"))
                                ));

                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label("Social links");
                                        if let Some(links) = &info.links {
                                            if links.is_empty() {
                                                ui.label("No links");
                                            } else {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                                    for link in links {
                                                        let caption =
                                                            if link.name.chars().count() > 18 {
                                                                let short = link.name
                                                                    .chars()
                                                                    .take(18)
                                                                    .collect::<String>();
                                                                format!("{short}...")
                                                            } else {
                                                                link.name.clone()
                                                            };
                                                        if ui
                                                            .add_sized([120.0, 22.0], egui::Button::new(caption))
                                                            .clicked()
                                                        {
                                                            ui.ctx().open_url(egui::output::OpenUrl {
                                                                url: link.url.clone(),
                                                                new_tab: true,
                                                            });
                                                        }
                                                    }
                                                });
                                            }
                                        } else {
                                            ui.label("No links");
                                        }
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                        let selected_address = selected_address_current
                                            .clone()
                                            .unwrap_or_else(|| String::from("(none)"));
                                        let label = if self.is_favorite(&selected_address) {
                                            "Unfavorite"
                                        } else {
                                            "Favorite"
                                        };
                                        if ui.button(label).clicked() {
                                            *favorite_target = Some(selected_address);
                                        }
                                    });
                                });
                            }
                        }
                    });
                ui.add_space(1.0);
            }
        });
    }

    fn draw_server_filters(&mut self, ui: &mut egui::Ui, _filtered_len: usize) {
        ui.horizontal(|ui| {
            nano_heading(ui, "Filters");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("\u{2715}").on_hover_text("Close filters").clicked() {
                    self.hub_filters_visible = false;
                }
            });
        });
        ui.separator();

        let languages = collect_distinct_tags(&self.servers, "language");
        let regions = collect_distinct_tags(&self.servers, "region");
        let rps = collect_distinct_tags(&self.servers, "rp");

        ui.add_space(6.0);
        self.filter_combo(ui, "Language", &languages, "language");

        ui.add_space(6.0);
        self.filter_combo(ui, "Region", &regions, "region");

        ui.add_space(6.0);
        self.filter_combo(ui, "Role-play level", &rps, "rp");

        if !self.hub_filter_tags.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Active filters").strong().small());
            for tag in &self.hub_filter_tags {
                ui.label(egui::RichText::new(format!("\u{2022} {tag}")).small());
            }
        }

        ui.add_space(12.0);
        if ui.button("Clear all filters").clicked() {
            self.hub_filter_tags.clear();
        }
    }

    fn filter_combo(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        options: &[(String, String)],
        prefix: &str,
    ) {
        let prefix_tag = format!("{prefix}:");
        let current = self
            .hub_filter_tags
            .iter()
            .find_map(|t| options.iter().find(|(k, _)| k == t));

        ui.label(egui::RichText::new(label).strong());
        let selected_text = current
            .map(|(_, disp)| disp.as_str())
            .unwrap_or("Any");
        egui::ComboBox::from_id_salt(format!("filter_{label}"))
            .selected_text(selected_text)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                let clear_category = ui
                    .selectable_label(current.is_none(), "Any")
                    .on_hover_text("Show all servers regardless of this category")
                    .clicked();
                if clear_category {
                    self.hub_filter_tags.retain(|t| !t.starts_with(&prefix_tag));
                }
                for (key, disp) in options {
                    let is_sel = current.map(|(k, _)| k == key).unwrap_or(false);
                    if ui.selectable_label(is_sel, disp).clicked() {
                        self.hub_filter_tags.retain(|t| !t.starts_with(&prefix_tag));
                        if !self.hub_filter_tags.contains(key) {
                            self.hub_filter_tags.push(key.clone());
                        }
                    }
                }
            });
    }

    fn load_hub_server_info(&mut self, address: &str) {
        self.selected_server_address = Some(address.to_string());
        self.cfg.hub_server_url = normalize_base_url(&self.cfg.hub_server_url);
        let options = self.hub_options();
        match fetch_server_info_from_hub_with_options(&self.cfg.hub_server_url, address, options) {
            Ok(info) => {
                self.selected_server_info = Some(info);
                self.status = format!("Loaded server info for {address}");
                self.push_log(self.status.clone());
            }
            Err(err) => {
                self.selected_server_info = None;
                self.status = format!("Failed to get server info from hub: {err:#}");
                self.push_log(self.status.clone());
            }
        }
    }

    pub(super) fn draw_options_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("options_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                nano_heading(ui, "Options");
        ui.separator();

        ui.label("Networking");
        ui.checkbox(&mut self.cfg.proxy_enabled, "Enable proxy for launcher network traffic");
        if ui.button("Proxy settings").clicked() {
            self.show_proxy_modal = true;
        }
        egui::ComboBox::from_label("Proxy presets")
            .selected_text(if self.cfg.proxy_url.trim().is_empty() {
                "None"
            } else {
                &self.cfg.proxy_url
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(self.cfg.proxy_url.trim().is_empty(), "None").clicked() {
                    self.cfg.proxy_url.clear();
                }

                for preset in &self.cfg.proxy_presets {
                    let selected = self.cfg.proxy_url == *preset;
                    if ui.selectable_label(selected, preset).clicked() {
                        self.cfg.proxy_url = preset.clone();
                    }
                }
            });

        ui.separator();
        ui.label("Connection");
        ui.checkbox(&mut self.cfg.auto_reconnect, "Auto-reconnect to the last server when disconnected");
        ui.horizontal(|ui| {
            ui.label("Reconnect delay (ms):");
            ui.add(
                egui::DragValue::new(&mut self.cfg.auto_reconnect_delay_ms)
                    .range(0..=600000)
                    .speed(100),
            );
        });

        ui.separator();
        ui.label("Storage");
        ui.horizontal(|ui| {
            if ui.button("Clear installed server content").clicked() {
                self.clear_installed_server_content();
            }
            if ui.button("Clear installed engines").clicked() {
                self.clear_installed_engines();
            }
        });

        ui.separator();
        ui.label("Appearance");
        ui.horizontal(|ui| {
            ui.label("Background image:");
            if ui.button("Choose image").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.cfg.background_image = path.display().to_string();
                    self.status = format!("Background image set to {}", self.cfg.background_image);
                    self.push_log(self.status.clone());
                }
            }
            if ui.button("Clear").clicked() {
                self.cfg.background_image.clear();
                self.status = String::from("Background image cleared");
                self.push_log(self.status.clone());
            }
        });
        if !self.cfg.background_image.trim().is_empty() {
            ui.label(
                egui::RichText::new(&self.cfg.background_image)
                    .small()
                    .weak(),
            );
        } else {
            ui.label(
                egui::RichText::new("No background image selected")
                    .small()
                    .weak(),
            );
        }
        ui.separator();
        ui.label("Background image positioning");
        ui.horizontal(|ui| {
            ui.label("Position X:");
            ui.add(egui::DragValue::new(&mut self.cfg.background_image_config.pos_x).speed(1.0));
            ui.label("Position Y:");
            ui.add(egui::DragValue::new(&mut self.cfg.background_image_config.pos_y).speed(1.0));
            ui.label("Scale:");
            ui.add(
                egui::DragValue::new(&mut self.cfg.background_image_config.scale)
                    .range(0.1..=10.0)
                    .speed(0.05),
            );
        });
        if !self.cfg.background_image.trim().is_empty() {
            if ui
                .button("Center image")
                .on_hover_text("Reset the image to the center of the window")
                .clicked()
            {
                self.cfg.background_image_config.pos_x = 0.0;
                self.cfg.background_image_config.pos_y = 0.0;
            }
        }
        ui.separator();
        ui.separator();
        ui.label("Color scheme");
        let cs = &mut self.cfg.color_scheme;
        color_row(ui, "Background", cs, 0);
        color_row(ui, "Top bar", cs, 1);
        color_row(ui, "Footer", cs, 2);
        color_row(ui, "Buttons", cs, 3);
        color_row(ui, "Hover", cs, 4);
        color_row(ui, "Hub", cs, 5);
        color_row(ui, "Text", cs, 6);
        color_row(ui, "Muted text", cs, 7);
        color_row(ui, "Accent", cs, 8);
        if ui.button("Reset colors to default").clicked() {
            self.cfg.color_scheme = crate::backend::ColorScheme::default();
        }

        ui.separator();
        ui.label("Extensions");
        ui.horizontal(|ui| {
            if ui.button("Sideload extension bundle").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    match sideload_extension_bundle(&self.paths, &path) {
                        Ok(name) => {
                            if !self.cfg.enabled_extensions.iter().any(|e| e == &name) {
                                self.cfg.enabled_extensions.push(name.clone());
                            }
                            self.extension_files = list_sideloaded_extensions(&self.paths).unwrap_or_default();
                            self.status = format!("Sideloaded extension bundle: {name}");
                            self.push_log(self.status.clone());
                        }
                        Err(err) => {
                            self.status = format!("Failed to sideload extension: {err:#}");
                            self.push_log(self.status.clone());
                        }
                    }
                }
            }

            if ui.button("Refresh extension list").clicked() {
                self.extension_files = list_sideloaded_extensions(&self.paths).unwrap_or_default();
            }
        });

        for ext in self.extension_files.clone() {
            ui.horizontal(|ui| {
                let mut enabled = self.cfg.enabled_extensions.iter().any(|e| e == &ext);
                if ui.checkbox(&mut enabled, &ext).changed() {
                    if enabled {
                        if !self.cfg.enabled_extensions.iter().any(|e| e == &ext) {
                            self.cfg.enabled_extensions.push(ext.clone());
                        }
                    } else if let Some(idx) = self.cfg.enabled_extensions.iter().position(|e| e == &ext) {
                        self.cfg.enabled_extensions.remove(idx);
                    }
                }

                if ui.small_button("Remove").on_hover_text("Remove this sideloaded bundle").clicked() {
                    match remove_sideloaded_extension(&self.paths, &ext) {
                        Ok(()) => {
                            self.cfg.enabled_extensions.retain(|e| e != &ext);
                            self.extension_files =
                                list_sideloaded_extensions(&self.paths).unwrap_or_default();
                            self.status = format!("Removed extension: {ext}");
                            self.push_log(self.status.clone());
                        }
                        Err(err) => {
                            self.status = format!("Failed to remove extension: {err:#}");
                            self.push_log(self.status.clone());
                        }
                    }
                }
            });
        }

        ui.separator();
        ui.separator();
        ui.label("Updates");
        ui.horizontal(|ui| {
            ui.label(format!("Current version: v{}", env!("CARGO_PKG_VERSION")));
            if !self.release_checking() {
                let label = if self.release_check_done() {
                    "Re-check for updates"
                } else {
                    "Check for updates"
                };
                if ui.button(label).on_hover_text("Contact GitHub to look for a newer release").clicked() {
                    self.start_update_check();
                }
                if self.update_available() == Some(true) {
                    if ui
                        .button("Update launcher")
                        .on_hover_text("Download the compiled update for this OS")
                        .clicked()
                    {
                        self.start_update();
                    }
                }
            }
        });
        if let Some(tag) = self.latest_release_label() {
            ui.label(format!("Latest version: {tag}"));
        }
        if self.release_checking() {
            ui.label(egui::RichText::new("Checking for updates...").weak());
        } else if let Some(error) = self.release_check_error() {
            ui.label(egui::RichText::new(error).color(SUB_TEXT));
        } else if self.update_available() == Some(true) {
            ui.label(
                egui::RichText::new("Updates available")
                    .strong()
                    .color(GOLD),
            );
        } else if self.update_available() == Some(false) {
            ui.label(egui::RichText::new("You are up to date").weak());
        }
        ui.checkbox(
            &mut self.cfg.auto_update,
            "Enable automatic updates",
        );

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button("Save Config")
                .on_hover_text("Save the current settings to the config file")
                .clicked()
            {
                match save_config(&self.paths, &self.cfg) {
                    Ok(()) => {
                        self.status = format!(
                            "Config saved to {}",
                            self.paths.config_path.to_string_lossy()
                        );
                        self.push_log(self.status.clone());
                    }
                    Err(err) => {
                        self.status = format!("Failed to save config: {err:#}");
                        self.push_log(self.status.clone());
                    }
                }
            }

            if ui
                .button("Load Config")
                .on_hover_text("Load settings from a config file")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new().add_filter("Config", &["toml", "cfg"]).pick_file() {
                    match load_config_from_path(&path) {
                        Ok(cfg) => {
                            self.cfg = cfg;
                            self.extension_files =
                                list_sideloaded_extensions(&self.paths).unwrap_or_default();
                            self.status =
                                format!("Config loaded from {}", path.display());
                            self.push_log(self.status.clone());
                        }
                        Err(err) => {
                            self.status = format!("Failed to load config: {err:#}");
                            self.push_log(self.status.clone());
                        }
                    }
                }
            }

            if ui
                .button("Reset Config")
                .on_hover_text("Reset the current config to defaults")
                .clicked()
            {
                self.cfg = crate::backend::LauncherConfig::default();
                self.extension_files =
                    list_sideloaded_extensions(&self.paths).unwrap_or_default();
                self.status = String::from("Config reset to defaults");
                self.push_log(self.status.clone());
            }
        });

        ui.separator();
        ui.label("Launcher data directories");
        ui.label(format!("User data: {}", self.paths.user_data_dir.to_string_lossy()));
        ui.label(format!("Local data: {}", self.paths.local_data_dir.to_string_lossy()));
        ui.label(format!("Logs: {}", self.paths.logs_dir.to_string_lossy()));
        ui.label(format!("Clients: {}", self.paths.clients_dir.to_string_lossy()));
        ui.label(format!("Extensions: {}", self.paths.extensions_dir.to_string_lossy()));
    });
    }
}


fn color_row(ui: &mut egui::Ui, label: &str, cs: &mut ColorScheme, kind: u8) {
        let (r, g, b) = match kind {
            0 => (&mut cs.bg_r, &mut cs.bg_g, &mut cs.bg_b),
            1 => (&mut cs.header_r, &mut cs.header_g, &mut cs.header_b),
            2 => (&mut cs.footer_r, &mut cs.footer_g, &mut cs.footer_b),
            3 => (&mut cs.button_r, &mut cs.button_g, &mut cs.button_b),
            4 => (&mut cs.hover_r, &mut cs.hover_g, &mut cs.hover_b),
            5 => (&mut cs.item_r, &mut cs.item_g, &mut cs.item_b),
            6 => (&mut cs.text_r, &mut cs.text_g, &mut cs.text_b),
            7 => (&mut cs.sub_text_r, &mut cs.sub_text_g, &mut cs.sub_text_b),
            _ => (&mut cs.accent_r, &mut cs.accent_g, &mut cs.accent_b),
        };
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(r).range(0..=255).speed(1).prefix("R "));
            ui.add(egui::DragValue::new(g).range(0..=255).speed(1).prefix("G "));
            ui.add(egui::DragValue::new(b).range(0..=255).speed(1).prefix("B "));
            let color = egui::Color32::from_rgb(*r, *g, *b);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, color);
        });
    }


fn truncated_label(ui: &mut egui::Ui, text: &str, width: f32, color: egui::Color32, _strong: bool) -> egui::Response {
    let font_id = egui::FontId::proportional(14.0);
    let rect = egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(width, 18.0));
    let response = ui.allocate_rect(rect, egui::Sense::click());

    let mut fit_text = String::new();
    const ELL_PAD: f32 = 12.0;
    let mut w = 0.0;
    for ch in text.chars() {
        let gw = ui.fonts(|f| f.layout_no_wrap(ch.to_string(), font_id.clone(), color)).size().x;
        if w + gw > width - ELL_PAD {
            fit_text.push('…');
            break;
        }
        w += gw;
        fit_text.push(ch);
    }

    let painter = ui.painter().clone();
    painter.text(
        rect.left_center() + egui::vec2(0.0, 0.0),
        egui::Align2::LEFT_CENTER,
        fit_text,
        font_id,
        color,
    );
    response.on_hover_text(text)
}
