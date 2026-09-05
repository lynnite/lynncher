use eframe::egui;

use crate::backend::{
    fetch_server_info_direct_with_proxy, fetch_server_info_from_hub_with_options,
    list_sideloaded_extensions, load_config_from_path,
    normalize_base_url, remove_sideloaded_extension, save_config, sideload_extension_bundle,
    ColorScheme, HubServerEntry, ServerInfo,
};

use super::LauncherApp;
use super::i18n::Localizer;

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

fn canonical_tag(prefix: &str, value: &str) -> String {
    match prefix {
        "rp" => match value {
            "low" | "lrp" => "low",
            "med" | "medium" | "mrp" => "med",
            "high" | "hrp" | "hard" | "very_high" => "high",
            "none" => "none",
            other => other,
        }
        .to_string(),
        "region" => match value {
            "am_n_c" | "am_n_e" | "am_n_w" | "na" | "ca" => "na",
            "am_s_e" | "am_s_w" | "am_s_s" | "sa" | "br" => "sa",
            "eu" | "eu_w" | "eu_e" | "es" => "eu",
            "luna" => "luna",
            other => other,
        }
        .to_string(),
        _ => value.to_string(),
    }
}

fn server_tag_matches(filter_tag: &str, server_tags: &[String]) -> bool {
    let Some((prefix, canon)) = filter_tag.split_once(':') else {
        return server_tags.iter().any(|t| t == filter_tag);
    };
    let needle = format!("{prefix}:");
    server_tags.iter().any(|t| {
        if let Some(val) = t.strip_prefix(&needle) {
            canonical_tag(prefix, val) == canon
        } else {
            false
        }
    })
}

fn collect_distinct_tags(
    servers: &[crate::backend::HubServerEntry],
    prefix: &str,
    loc: &Localizer,
) -> Vec<(String, String)> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for server in servers {
        if let Some(tags) = &server.status_data.tags {
            for tag in tags {
                let Some(value) = tag.strip_prefix(&format!("{prefix}:")) else {
                    continue;
                };
                let canon = canonical_tag(prefix, value);
                let key = format!("{prefix}:{canon}");
                let label = tag_label(prefix, &canon, loc);
                if !seen.iter().any(|(k, _)| k == &key) {
                    seen.push((key, label));
                }
            }
        }
    }
    seen.sort_by(|a, b| a.1.cmp(&b.1));
    seen
}

fn tag_label(prefix: &str, value: &str, loc: &Localizer) -> String {
    let mut key = String::from(value);
    match prefix {
        "region" => {
            key.insert_str(0, "region.");
            return loc.t(&key, &[]);
        }
        "rp" => {
            key.insert_str(0, "filter.");
            key = match value {
                "none" | "low" | "lrp" | "med" | "medium" | "mrp" | "high" | "hrp" | "hard"
                | "very_high" => key,
                other => other.to_string(),
            };
            return loc.t(&key, &[]);
        }
        "lang" | "language" => return value.to_uppercase(),
        _ => {}
    }
    value.to_string()
}

impl LauncherApp {
    pub(super) fn draw_favorites_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("favorites_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    nano_heading(ui, self.t("favorites.title", &[]));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let t_refresh = self.t("favorites.refresh_hint", &[]);
                        if ui.small_button("⟳").on_hover_text(t_refresh).clicked() {
                            let msg = self.t("status.refreshing_home", &[]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                            self.refresh_home();
                        }
                    });
                });
        ui.separator();

        if self.cfg.favorite_servers.is_empty() {
            let t_empty = self.t("favorites.empty", &[]);
            ui.label(t_empty);
        } else {
            let mut connect_target: Option<String> = None;
            let mut remove_target: Option<String> = None;
            let mut rename_target: Option<String> = None;
            let mut desc_toggle_target: Option<String> = None;
            let t_connect = self.t("favorites.connect", &[]);
            let t_rename = self.t("favorites.rename", &[]);
            let t_remove = self.t("favorites.remove", &[]);
            let t_offline = self.t("favorites.offline", &[]);
            let t_no_desc = self.t("favorites.no_desc", &[]);

            self.ensure_favorite_infos();

            egui::ScrollArea::vertical()
                .id_salt("favorites_list")
                .max_height(260.0)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                let item_alpha = (self.cfg.color_scheme.item_alpha.clamp(0.0, 1.0) * 255.0) as u8;
                let item_col = egui::Color32::from_rgba_unmultiplied(
                    self.cfg.color_scheme.item_r,
                    self.cfg.color_scheme.item_g,
                    self.cfg.color_scheme.item_b,
                    item_alpha,
                );
                for address in self.cfg.favorite_servers.clone() {
                    let (name, desc) = self.favorite_summary(&address);
                    let show_desc = *self.favorite_desc_visible.get(&address).unwrap_or(&false);
                    let server_entry = self
                        .servers
                        .iter()
                        .find(|s| s.address.eq_ignore_ascii_case(&address));
                    let t_show_hint = self.t("favorites.show_desc", &[]);
                    egui::Frame::none()
                        .fill(item_col)
                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // The name widget is sized only to its text. Clicking
                                // it toggles the description.
                                let name_resp = ui
                                    .add(
                                        egui::Label::new(egui::RichText::new(&name).strong())
                                            .truncate(),
                                    )
                                    .on_hover_text(t_show_hint);
                                if name_resp.clicked() {
                                    desc_toggle_target = Some(address.clone());
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::TOP),
                                    |ui| {
                                        if ui.small_button(&t_remove).clicked() {
                                            remove_target = Some(address.clone());
                                        }
                                        if ui.small_button(&t_rename).clicked() {
                                            rename_target = Some(address.clone());
                                        }
                                        if ui.small_button(&t_connect).clicked() {
                                            connect_target = Some(address.clone());
                                        }
                                        let round_time = server_entry
                                            .map(format_round_time)
                                            .unwrap_or_else(|| String::from("—"));
                                        ui.label(
                                            egui::RichText::new(round_time)
                                                .small()
                                                .color(SUB_TEXT),
                                        );
                                        if let Some(entry) = server_entry {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{} / {}",
                                                    entry.status_data.players,
                                                    entry.status_data.soft_max_players
                                                ))
                                                .small()
                                                .strong()
                                                .color(GOLD),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new(&t_offline)
                                                    .small()
                                                    .weak(),
                                            );
                                        }
                                    },
                                );
                            });

                            if show_desc {
                                ui.label(
                                    egui::RichText::new(self.t("favorites.ip", &[&address]))
                                        .small()
                                        .weak(),
                                );
                                if desc.is_empty() {
                                    ui.label(egui::RichText::new(&t_no_desc).small().weak());
                                } else {
                                    ui.label(egui::RichText::new(&desc).small().weak());
                                }
                            }
                        });
                    ui.add_space(1.0);
                }
            });

            if let Some(address) = desc_toggle_target {
                let cur = self.favorite_desc_visible.get(&address).copied().unwrap_or(false);
                self.favorite_desc_visible.insert(address, !cur);
            }

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
        let t_event_log = self.t("favorites.event_log", &[]);
        let t_no_events = self.t("favorites.no_events", &[]);
        ui.label(t_event_log);
        egui::ScrollArea::vertical()
            .id_salt("event_log")
            .max_height(180.0)
            .show(ui, |ui| {
            if self.logs.is_empty() {
                ui.label(t_no_events);
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
                            .map(|t| server_tag_matches(wanted, t))
                            .unwrap_or(false)
                    })
            })
            .cloned()
            .collect();

        let table_h = (ui.available_height() - 12.0).max(120.0);

        if self.hub_filters_visible {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    let avail_h = (ui.available_height() - 12.0).max(120.0);
                    egui::ScrollArea::vertical()
                        .id_salt("server_filters_scroll")
                        .auto_shrink([false, true])
                        .max_height(avail_h)
                        .show(ui, |ui| {
                            self.draw_server_filters(ui, filtered.len());
                        });
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
                    let t_search_hint = self.t("hub.search_hint", &[]);
                    let t_filters = self.t(
                        "hub.filters_count",
                        &[&filtered_len.to_string(), &self.servers.len().to_string()],
                    );
                    let t_refresh = self.t("hub.refresh", &[]);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.hub_search_query)
                            .hint_text(t_search_hint)
                            .desired_width((ui.available_width() - 180.0).max(120.0)),
                    );
                    ui.toggle_value(&mut self.hub_filters_visible, t_filters);
                    if ui.button("⟳").on_hover_text(t_refresh).clicked() {
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
                            .map(|t| server_tag_matches(wanted, t))
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
            let item_alpha = (self.cfg.color_scheme.item_alpha.clamp(0.0, 1.0) * 255.0) as u8;
            let item_col = egui::Color32::from_rgba_unmultiplied(
                self.cfg.color_scheme.item_r,
                self.cfg.color_scheme.item_g,
                self.cfg.color_scheme.item_b,
                item_alpha,
            );
            let accent_col = egui::Color32::from_rgba_unmultiplied(
                self.cfg.color_scheme.accent_r,
                self.cfg.color_scheme.accent_g,
                self.cfg.color_scheme.accent_b,
                item_alpha,
            );
            if servers.is_empty() {
                let status = if self.servers.is_empty() {
                    self.t("hub.no_public", &[])
                } else {
                    self.t("hub.no_match", &[])
                };
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(status).color(SUB_TEXT));
                });
                return;
            }

            for server in servers {
                let t_unnamed = self.t("hub.unnamed", &[]);
                let name = server
                    .status_data
                    .name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&t_unnamed);

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
                                    let t_connect = self.t("hub.connect", &[]);
                                    if ui.button(t_connect).clicked() {
                                        *connect_target = Some(server.address.clone());
                                    }
                                },
                            );
                        });

                        if is_selected {
                            if let Some(info) = selected_info_current {
                                ui.separator();
                                let no_desc = self.t("hub.description_no", &[]);
                                let t_desc = self.t(
                                    "hub.description",
                                    &[&info.desc.clone().unwrap_or_else(|| no_desc)],
                                );
                                ui.label(t_desc);

                                let t_social = self.t("hub.social_links", &[]);
                                let t_no_links = self.t("hub.no_links", &[]);
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(t_social);
                                        if let Some(links) = &info.links {
                                            if links.is_empty() {
                                                ui.label(t_no_links);
                                            } else {
                                                let btn_w = 120.0;
                                                let btn_h = 22.0;
                                                let spacing = 6.0;
                                                let avail = ui.available_width();
                                                let per_row = ((avail + spacing) / (btn_w + spacing)).floor().max(1.0) as usize;
                                                let row_start = ui.cursor().left_top();
                                                let max_x = row_start.x + avail;
                                                let mut x = row_start.x;
                                                let mut y = row_start.y;
                                                let mut placed = 0usize;
                                                for link in links {
                                                    if placed > 0 && placed % per_row == 0 {
                                                        x = row_start.x;
                                                        y += btn_h + spacing;
                                                    }
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
                                                    let rect = egui::Rect::from_min_size(
                                                        egui::pos2(x.min(max_x), y),
                                                        egui::vec2(btn_w, btn_h),
                                                    );
                                                    if ui.put(rect, egui::Button::new(caption)).clicked() {
                                                        ui.ctx().open_url(egui::output::OpenUrl {
                                                            url: link.url.clone(),
                                                            new_tab: true,
                                                        });
                                                    }
                                                    x += btn_w + spacing;
                                                    placed += 1;
                                                }
                                                let used_h = (y + btn_h - row_start.y).max(1.0);
                                                ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                                                    row_start,
                                                    egui::vec2(1.0, used_h),
                                                ));
                                            }
                                        } else {
                                            ui.label(t_no_links);
                                        }
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                        let selected_address = selected_address_current
                                            .clone()
                                            .unwrap_or_else(|| String::from("(none)"));
                                        let label = if self.is_favorite(&selected_address) {
                                            self.t("hub.unfavorite", &[])
                                        } else {
                                            self.t("hub.favorite", &[])
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
        let t_filters = self.t("hub.filters", &[]);
        let t_close = self.t("hub.close_filters", &[]);
        let t_language = self.t("hub.language", &[]);
        let t_region = self.t("hub.region", &[]);
        let t_roleplay = self.t("hub.roleplay", &[]);
        let t_active = self.t("hub.active_filters", &[]);
        let t_clear = self.t("hub.clear_all_filters", &[]);

        ui.horizontal(|ui| {
            nano_heading(ui, t_filters);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("\u{2715}").on_hover_text(t_close).clicked() {
                    self.hub_filters_visible = false;
                }
            });
        });
        ui.separator();

        let loc = &self.localizer;
        let languages = collect_distinct_tags(&self.servers, "lang", loc);
        let regions = collect_distinct_tags(&self.servers, "region", loc);
        let rps = collect_distinct_tags(&self.servers, "rp", loc);

        self.draw_filter_category(ui, &t_language, &languages, "lang");
        ui.separator();
        self.draw_filter_category(ui, &t_region, &regions, "region");
        ui.separator();
        self.draw_filter_category(ui, &t_roleplay, &rps, "rp");

        if !self.hub_filter_tags.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&t_active).strong().small());
            for tag in &self.hub_filter_tags {
                ui.label(egui::RichText::new(format!("\u{2022} {tag}")).small());
            }
        }

        ui.add_space(12.0);
        if ui.button(t_clear).clicked() {
            self.hub_filter_tags.clear();
        }
    }

    fn draw_filter_category(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        options: &[(String, String)],
        prefix: &str,
    ) {
        let prefix_tag = format!("{prefix}:");

        ui.add_space(4.0);
        ui.label(egui::RichText::new(label).strong());
        ui.add_space(2.0);

        if options.is_empty() {
            let t_none = self.t("hub.none", &[]);
            ui.label(
                egui::RichText::new(t_none).small().color(SUB_TEXT),
            );
            return;
        }

        for (key, disp) in options {
            let mut checked = self.hub_filter_tags.iter().any(|t| t == key);
            if ui.checkbox(&mut checked, disp).changed() {
                if checked {
                    self.hub_filter_tags.retain(|t| !t.starts_with(&prefix_tag));
                    if !self.hub_filter_tags.iter().any(|t| t == key) {
                        self.hub_filter_tags.push(key.clone());
                    }
                } else {
                    self.hub_filter_tags.retain(|t| t != key);
                }
            }
        }
    }

    fn load_hub_server_info(&mut self, address: &str) {
        self.selected_server_address = Some(address.to_string());

        if let Some(cached) = self.server_info_cache.get(address).cloned() {
            self.selected_server_info = cached;
            return;
        }

        if self.server_info_loading.contains(address) {
            return;
        }
        self.server_info_loading.insert(address.to_string());

        self.cfg.hub_server_url = normalize_base_url(&self.cfg.hub_server_url);
        let url = self.cfg.hub_server_url.clone();
        let options = self.hub_options();
        let proxy = options.proxy_url.clone();
        let addr = address.to_string();
        let pending = self.server_info_pending.clone();

        std::thread::spawn(move || {
            let result = fetch_server_info_from_hub_with_options(&url, &addr, options)
                .or_else(|_| fetch_server_info_direct_with_proxy(&addr, proxy.as_deref()));
            if let Ok(mut slot) = pending.lock() {
                *slot = Some((addr, result));
            }
        });
    }

    pub(crate) fn poll_hub_server_info(&mut self, ctx: &egui::Context) {
        let result = {
            let mut slot = self.server_info_pending.lock().unwrap_or_else(|e| e.into_inner());
            slot.take()
        };
        let Some((address, result)) = result else {
            return;
        };

        self.server_info_loading.remove(&address);
        match result {
            Ok(info) => {
                self.server_info_cache.insert(address.clone(), Some(info.clone()));
                if self.selected_server_address.as_deref() == Some(address.as_str()) {
                    self.selected_server_info = Some(info);
                }
                let msg = self.t("status.server_info_loaded", &[&address]);
                self.status = msg;
                self.push_log(self.status.clone());
            }
            Err(err) => {
                self.server_info_cache.insert(address.clone(), None);
                if self.selected_server_address.as_deref() == Some(address.as_str()) {
                    self.selected_server_info = None;
                }
                let msg = self.t("status.server_info_fail", &[&err.to_string()]);
                self.status = msg;
                self.push_log(self.status.clone());
            }
        }
        ctx.request_repaint();
    }

    pub(super) fn draw_options_page(&mut self, ui: &mut egui::Ui) {
        // Translate once up-front; the options page mutates self.cfg extensively
        // inside the scroll closure, so we capture owned strings to avoid
        // borrow conflicts with self.t().
        let t_options = self.t("options.title", &[]);
        let t_networking = self.t("options.networking", &[]);
        let t_proxy_enable = self.t("options.proxy_enable", &[]);
        let t_proxy_settings = self.t("options.proxy_settings", &[]);
        let t_proxy_presets = self.t("options.proxy_presets", &[]);
        let t_none = self.t("hub.none", &[]);
        let t_connection = self.t("options.connection", &[]);
        let t_auto_reconnect = self.t("options.auto_reconnect", &[]);
        let t_reconnect_delay = self.t("options.reconnect_delay", &[]);
        let t_storage = self.t("options.storage", &[]);
        let t_clear_content = self.t("options.clear_content", &[]);
        let t_clear_engines = self.t("options.clear_engines", &[]);
        let t_hwid = self.t("options.hwid", &[]);
        let t_hwid_desc = self.t("options.hwid_desc", &[]);
        let t_hwid_default = self.t("options.hwid_default", &[]);
        let t_hwid_random = self.t("options.hwid_random", &[]);
        let t_hwid_custom = self.t("options.hwid_custom", &[]);
        let t_hwid_randomize = self.t("options.hwid_randomize", &[]);
        let t_hwid_set = self.t("options.hwid_set", &[]);
        let t_hwid_value_hint = self.t("options.hwid_value_hint", &[]);
        let t_hwid_none = self.t("options.hwid_none", &[]);
        let t_hwid_randomized = self.t("options.hwid_randomized", &[]);
        let t_hwid_set_ok = self.t("options.hwid_set_ok", &[]);
        let t_hwid_set_bad = self.t("options.hwid_set_bad", &[]);
        let t_hwid_random_fail = self.t("options.hwid_random_fail", &[]);
        let t_appearance = self.t("options.appearance", &[]);
        let t_logo_text_only = self.t("options.logo_text_only", &[]);
        let t_text_shadow = self.t("options.text_shadow", &[]);
        let t_text_shadow_color = self.t("options.text_shadow_color", &[]);
        let t_select_font = self.t("options.select_font", &[]);
        let t_reset_font = self.t("options.reset_font", &[]);
        let t_font_ubuntu = self.t("options.font_ubuntu", &[]);
        let t_font_size = self.t("options.font_size", &[]);
        let t_add_image = self.t("options.add_image", &[]);
        let t_remove_selected = self.t("options.remove_selected", &[]);
        let t_no_background = self.t("options.no_background", &[]);
        let t_background_positioning = self.t("options.background_positioning", &[]);
        let t_pos_x = self.t("options.pos_x", &[]);
        let t_pos_y = self.t("options.pos_y", &[]);
        let t_scale = self.t("options.scale", &[]);
        let t_center_image = self.t("options.center_image", &[]);
        let t_center_image_hover = self.t("options.center_image_hover", &[]);
        let t_no_background_pos = self.t("options.no_background_pos", &[]);
        let t_pause_animations = self.t("options.pause_animations", &[]);
        let t_color_scheme = self.t("options.color_scheme", &[]);
        let t_c_bg = self.t("options.color_background", &[]);
        let t_c_top = self.t("options.color_topbar", &[]);
        let t_c_footer = self.t("options.color_footer", &[]);
        let t_c_buttons = self.t("options.color_buttons", &[]);
        let t_c_hover = self.t("options.color_hover", &[]);
        let t_c_hub = self.t("options.color_hub", &[]);
        let t_c_text = self.t("options.color_text", &[]);
        let t_c_muted = self.t("options.color_muted", &[]);
        let t_c_accent = self.t("options.color_accent", &[]);
        let t_box_opacity = self.t("options.box_opacity", &[]);
        let t_reset_colors = self.t("options.reset_colors", &[]);
        let t_extensions = self.t("options.extensions", &[]);
        let t_sideload = self.t("options.sideload", &[]);
        let t_refresh_extensions = self.t("options.refresh_extensions", &[]);
        let t_updates = self.t("options.updates", &[]);
        let t_check_updates_hover = self.t("options.check_updates_hover", &[]);
        let t_update_launcher = self.t("options.update_launcher", &[]);
        let t_update_launcher_hover = self.t("options.update_launcher_hover", &[]);
        let t_checking = self.t("options.checking_updates", &[]);
        let t_updates_available = self.t("options.updates_available", &[]);
        let t_up_to_date = self.t("options.up_to_date", &[]);
        let t_auto_update = self.t("options.auto_update", &[]);
        let t_save_config = self.t("options.save_config", &[]);
        let t_save_config_hover = self.t("options.save_config_hover", &[]);
        let t_load_config = self.t("options.load_config", &[]);
        let t_load_config_hover = self.t("options.load_config_hover", &[]);
        let t_reset_config = self.t("options.reset_config", &[]);
        let t_reset_config_hover = self.t("options.reset_config_hover", &[]);
        let t_data_dirs = self.t("options.data_dirs", &[]);
        let t_language = self.t("options.language", &[]);

        egui::ScrollArea::vertical()
            .id_salt("options_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                nano_heading(ui, t_options);
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(&t_language);
            egui::ComboBox::from_id_salt("lang_combo")
                .selected_text(
                    super::i18n::LANGUAGES
                        .iter()
                        .find(|(id, _)| *id == self.localizer.language)
                        .map(|(_, name)| *name)
                        .unwrap_or(self.localizer.language.as_str()),
                )
                .show_ui(ui, |ui| {
                    for (id, name) in super::i18n::LANGUAGES {
                        if ui
                            .selectable_label(self.localizer.language == *id, *name)
                            .clicked()
                        {
                            self.cfg.language = id.to_string();
                            self.localizer.set_language(id);
                            self.font_source_initialized = false;
                        }
                    }
                });
        });
        ui.separator();

        ui.label(t_networking);
        ui.checkbox(&mut self.cfg.proxy_enabled, &t_proxy_enable);
        if ui.button(t_proxy_settings).clicked() {
            self.show_proxy_modal = true;
        }
        let alpha = (self.cfg.color_scheme.item_alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let item_fill = egui::Color32::from_rgba_unmultiplied(
            self.cfg.color_scheme.item_r,
            self.cfg.color_scheme.item_g,
            self.cfg.color_scheme.item_b,
            alpha,
        );
        let old_inactive = ui.visuals().widgets.inactive.bg_fill;
        let old_hovered = ui.visuals().widgets.hovered.bg_fill;
        ui.visuals_mut().widgets.inactive.bg_fill = item_fill;
        ui.visuals_mut().widgets.hovered.bg_fill = item_fill;
        egui::ComboBox::from_label(t_proxy_presets)
            .selected_text(if self.cfg.proxy_url.trim().is_empty() {
                t_none.as_str()
            } else {
                &self.cfg.proxy_url
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(self.cfg.proxy_url.trim().is_empty(), t_none.as_str()).clicked() {
                    self.cfg.proxy_url.clear();
                }

                for preset in &self.cfg.proxy_presets {
                    let selected = self.cfg.proxy_url == *preset;
                    if ui.selectable_label(selected, preset).clicked() {
                        self.cfg.proxy_url = preset.clone();
                    }
                }
            });
        ui.visuals_mut().widgets.inactive.bg_fill = old_inactive;
        ui.visuals_mut().widgets.hovered.bg_fill = old_hovered;

        ui.separator();
        ui.label(t_connection);
        ui.checkbox(&mut self.cfg.auto_reconnect, &t_auto_reconnect);
        ui.horizontal(|ui| {
            ui.label(&t_reconnect_delay);
            ui.add(
                egui::DragValue::new(&mut self.cfg.auto_reconnect_delay_ms)
                    .range(0..=600000)
                    .speed(100),
            );
        });

        ui.separator();
        ui.label(t_storage);
        ui.horizontal(|ui| {
            if ui.button(&t_clear_content).clicked() {
                self.clear_installed_server_content();
            }
            if ui.button(&t_clear_engines).clicked() {
                self.clear_installed_engines();
            }
        });

        ui.separator();
        ui.label(t_hwid);
        ui.label(egui::RichText::new(&t_hwid_desc).small().weak());

        let current = if self.hwid_current.trim().is_empty() {
            crate::backend::read_hwid_hex()
        } else {
            self.hwid_current.clone()
        };
        if current.is_empty() {
            ui.label(egui::RichText::new(&t_hwid_none).small().weak());
        } else {
            ui.label(egui::RichText::new(&current).small());
        }

        let modes: [(&str, &String); 3] = [
            ("default", &t_hwid_default),
            ("random", &t_hwid_random),
            ("custom", &t_hwid_custom),
        ];
        let selected = self.cfg.hwid_mode.clone();
        egui::ComboBox::from_id_salt("hwid_mode")
            .selected_text(
                modes
                    .iter()
                    .find(|(k, _)| **k == selected)
                    .map(|(_, l)| l.as_str())
                    .unwrap_or("default"),
            )
            .show_ui(ui, |ui| {
                for (id, label) in &modes {
                    if ui
                        .selectable_label(self.cfg.hwid_mode == *id, label.as_str())
                        .clicked()
                    {
                        self.cfg.hwid_mode = id.to_string();
                    }
                }
            });

        ui.horizontal(|ui| {
            if ui.button(&t_hwid_randomize).clicked() {
                match crate::backend::randomize_hwid() {
                    Ok(hexstr) => {
                        self.hwid_current = hexstr;
                        self.hwid_feedback = t_hwid_randomized.clone();
                    }
                    Err(err) => {
                        self.hwid_feedback = t_hwid_random_fail.replace("{0}", &err.to_string());
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.hwid_input_buffer)
                    .hint_text(&t_hwid_value_hint)
                    .desired_width(260.0),
            );
            if ui.button(&t_hwid_set).clicked() {
                match crate::backend::set_hwid_hex(&self.hwid_input_buffer) {
                    Ok(hexstr) => {
                        self.hwid_current = hexstr.clone();
                        // Persist the value into the config so it is re-applied
                        // on later launches (mode "custom").
                        self.cfg.hwid_value = hexstr;
                        self.hwid_feedback = t_hwid_set_ok.clone();
                    }
                    Err(_) => {
                        self.hwid_feedback = t_hwid_set_bad.clone();
                    }
                }
            }
        });

        if !self.hwid_feedback.trim().is_empty() {
            ui.label(egui::RichText::new(&self.hwid_feedback).small());
        }

        ui.separator();
        ui.label(t_appearance);

        ui.checkbox(
            &mut self.cfg.logo_text_only,
            &t_logo_text_only,
        );

        ui.checkbox(
            &mut self.cfg.text_shadow,
            &t_text_shadow,
        );
        if self.cfg.text_shadow {
            let ts = &mut self.cfg.text_shadow_color;
            ui.horizontal(|ui| {
                ui.label(&t_text_shadow_color);
                ui.add(egui::DragValue::new(&mut ts.r).range(0..=255).speed(1).prefix("R "));
                ui.add(egui::DragValue::new(&mut ts.g).range(0..=255).speed(1).prefix("G "));
                ui.add(egui::DragValue::new(&mut ts.b).range(0..=255).speed(1).prefix("B "));
                let color = egui::Color32::from_rgb(ts.r, ts.g, ts.b);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, color);
            });
        }

        ui.horizontal(|ui| {
            if ui.button(&t_select_font).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.cfg.font_path = path.display().to_string();
                }
            }
            if !self.cfg.font_path.trim().is_empty() {
                if ui.button(&t_reset_font).clicked() {
                    self.cfg.font_path.clear();
                }
            }
        });
        if self.cfg.font_path.trim().is_empty() {
            ui.label(
                egui::RichText::new(&t_font_ubuntu)
                    .small()
                    .color(SUB_TEXT),
            );
        } else {
            let msg = self.t("options.font_path", &[&self.cfg.font_path]);
            ui.label(
                egui::RichText::new(msg)
                    .small()
                    .color(SUB_TEXT),
            );
        }

        ui.horizontal(|ui| {
            ui.label(&t_font_size);
            let mut slider_size = self.cfg.font_size.clamp(8.0, 40.0);
            let size_px = slider_size.clamp(8.0, 40.0);
            let px_label = self.t("options.px", &[&format!("{size_px:.0}")]);
            let resp = ui.add(
                egui::Slider::new(&mut slider_size, 8.0..=40.0)
                    .step_by(1.0)
                    .text(px_label),
            );
            if resp.drag_stopped() {
                self.cfg.font_size = slider_size;
            }
        });

        let mut add_image: Option<String> = None;
        let mut remove_index: Option<usize> = None;
        

        ui.horizontal(|ui| {
            if ui.button(&t_add_image).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    add_image = Some(path.display().to_string());
                }
            }
            if !self.cfg.background_images.is_empty() {
                if ui.button(&t_remove_selected).clicked() {
                    remove_index = Some(self.background_edit_index);
                }
            }
        });

        if self.cfg.background_images.is_empty() {
            ui.label(
                egui::RichText::new(&t_no_background)
                    .small()
                    .weak(),
            );
        } else {
            if self.background_edit_index >= self.cfg.background_images.len() {
                self.background_edit_index = self.cfg.background_images.len() - 1;
            }
            let edit = self.background_edit_index;
            let current = self.cfg.background_images[edit].path.clone();
            let mut chosen = edit;
            let alpha = (self.cfg.color_scheme.item_alpha.clamp(0.0, 1.0) * 255.0) as u8;
            let item_fill = egui::Color32::from_rgba_unmultiplied(
                self.cfg.color_scheme.item_r,
                self.cfg.color_scheme.item_g,
                self.cfg.color_scheme.item_b,
                alpha,
            );
            let old_inactive = ui.visuals().widgets.inactive.bg_fill;
            let old_hovered = ui.visuals().widgets.hovered.bg_fill;
            ui.visuals_mut().widgets.inactive.bg_fill = item_fill;
            ui.visuals_mut().widgets.hovered.bg_fill = item_fill;
            egui::ComboBox::from_id_salt("background_images_combo")
                .selected_text(current)
                .width(160.0)
                .show_ui(ui, |ui| {
                    for (i, img) in self.cfg.background_images.iter().enumerate() {
                        if ui
                            .selectable_label(i == self.background_edit_index, &img.path)
                            .clicked()
                        {
                            chosen = i;
                        }
                    }
                });
            ui.visuals_mut().widgets.inactive.bg_fill = old_inactive;
            ui.visuals_mut().widgets.hovered.bg_fill = old_hovered;
            if chosen != self.background_edit_index {
                self.background_edit_index = chosen;
            }
        }

        if let Some(path) = add_image {
            if !self.cfg.background_images.iter().any(|b| b.path == path) {
                let cascade = self.cfg.background_images.len() as f32;
                self.cfg.background_images.push(crate::backend::BackgroundImage {
                    path: path.clone(),
                    pos_x: cascade * 40.0,
                    pos_y: cascade * 25.0,
                    scale: 1.0,
                });
                self.background_edit_index = self.cfg.background_images.len() - 1;
            }
            let msg = self.t("options.background_added", &[&path]);
            self.status = msg;
            self.push_log(self.status.clone());
        }

        if let Some(i) = remove_index {
            if i < self.cfg.background_images.len() {
                self.cfg.background_images.remove(i);
                self.status = self.t("options.background_removed", &[]);
                self.push_log(self.status.clone());
            }
            if self.background_edit_index >= self.cfg.background_images.len() {
                self.background_edit_index =
                    self.cfg.background_images.len().saturating_sub(1);
            }
        }

        ui.separator();
        ui.label(t_background_positioning);
        if let Some(edit) = self.cfg.background_images.get_mut(self.background_edit_index) {
            ui.horizontal(|ui| {
                ui.label(&t_pos_x);
                ui.add(egui::DragValue::new(&mut edit.pos_x).speed(1.0));
                ui.label(&t_pos_y);
                ui.add(egui::DragValue::new(&mut edit.pos_y).speed(1.0));
                ui.label(&t_scale);
                ui.add(
                    egui::DragValue::new(&mut edit.scale)
                        .range(0.1..=10.0)
                        .speed(0.05),
                );
            });
            if ui
                .button(t_center_image.clone())
                .on_hover_text(t_center_image_hover.clone())
                .clicked()
            {
                edit.pos_x = 0.0;
                edit.pos_y = 0.0;
            }
        } else {
            ui.label(
                egui::RichText::new(&t_no_background_pos)
                    .small()
                    .weak(),
            );
        }

        ui.checkbox(
            &mut self.cfg.pause_animations_unfocused,
            &t_pause_animations,
        );

        ui.separator();
        ui.separator();
        ui.label(t_color_scheme);
        let alpha_pct = self.cfg.color_scheme.item_alpha * 100.0;
        let pct_label = self.t("options.pct", &[&format!("{alpha_pct:.0}")]);
        let cs = &mut self.cfg.color_scheme;
        color_row(ui, &t_c_bg, cs, 0);
        color_row(ui, &t_c_top, cs, 1);
        color_row(ui, &t_c_footer, cs, 2);
        color_row(ui, &t_c_buttons, cs, 3);
        color_row(ui, &t_c_hover, cs, 4);
        color_row(ui, &t_c_hub, cs, 5);
        color_row(ui, &t_c_text, cs, 6);
        color_row(ui, &t_c_muted, cs, 7);
        color_row(ui, &t_c_accent, cs, 8);
        ui.horizontal(|ui| {
            ui.label(&t_box_opacity);
            ui.add(
                egui::Slider::new(&mut cs.item_alpha, 0.0..=1.0)
                    .step_by(0.01)
                    .text(pct_label),
            );
        });
        if ui.button(t_reset_colors).clicked() {
            self.cfg.color_scheme = crate::backend::ColorScheme::default();
        }

        ui.separator();
        ui.label(t_extensions);
        ui.horizontal(|ui| {
            if ui.button(&t_sideload).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    match sideload_extension_bundle(&self.paths, &path) {
                        Ok(name) => {
                            if !self.cfg.enabled_extensions.iter().any(|e| e == &name) {
                                self.cfg.enabled_extensions.push(name.clone());
                            }
                            self.extension_files = list_sideloaded_extensions(&self.paths).unwrap_or_default();
                            let msg = self.t("options.sideloaded", &[&name]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                        }
                        Err(err) => {
                            let msg = self.t("options.sideload_fail", &[&err.to_string()]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                        }
                    }
                }
            }

            if ui.button(&t_refresh_extensions).clicked() {
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

                let t_rem = self.t("options.remove_extension", &[]);
                let t_rem_hover = self.t("options.remove_extension_hover", &[]);
                if ui.small_button(t_rem.clone()).on_hover_text(t_rem_hover).clicked() {
                    match remove_sideloaded_extension(&self.paths, &ext) {
                        Ok(()) => {
                            self.cfg.enabled_extensions.retain(|e| e != &ext);
                            self.extension_files =
                                list_sideloaded_extensions(&self.paths).unwrap_or_default();
                            let msg = self.t("options.extension_removed", &[&ext]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                        }
                        Err(err) => {
                            let msg = self.t("options.extension_remove_fail", &[&err.to_string()]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                        }
                    }
                }
            });
        }

        ui.separator();
        ui.separator();
        ui.label(t_updates);
        ui.horizontal(|ui| {
            let cv = self.t("options.current_version", &[super::APP_VERSION]);
            ui.label(cv);
            if !self.release_checking() {
                let label = if self.release_check_done() {
                    self.t("options.recheck_updates", &[])
                } else {
                    self.t("options.check_updates", &[]).clone()
                };
                if ui.button(label).on_hover_text(t_check_updates_hover.clone()).clicked() {
                    self.start_update_check();
                }
                if self.update_available() == Some(true) {
                    if ui
                        .button(t_update_launcher.clone())
                        .on_hover_text(t_update_launcher_hover.clone())
                        .clicked()
                    {
                        self.start_update();
                    }
                }
            }
        });
        if let Some(tag) = self.latest_release_label() {
            let lv = self.t("options.latest_version", &[&tag]);
            ui.label(lv);
        }
        if self.release_checking() {
            ui.label(egui::RichText::new(t_checking.clone()).weak());
        } else if let Some(error) = self.release_check_error_localized() {
            ui.label(egui::RichText::new(error).color(SUB_TEXT));
        } else if self.update_available() == Some(true) {
            ui.label(
                egui::RichText::new(&t_updates_available)
                    .strong()
                    .color(GOLD),
            );
        } else if self.update_available() == Some(false) {
            ui.label(egui::RichText::new(&t_up_to_date).weak());
        }
        ui.checkbox(
            &mut self.cfg.auto_update,
            &t_auto_update,
        );

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(t_save_config.clone())
                .on_hover_text(t_save_config_hover.clone())
                .clicked()
            {
                match save_config(&self.paths, &self.cfg) {
                    Ok(()) => {
                        let msg = self.t(
                            "options.config_saved",
                            &[&self.paths.config_path.to_string_lossy()],
                        );
                        self.status = msg;
                        self.push_log(self.status.clone());
                    }
                    Err(err) => {
                        let msg = self.t("options.config_save_fail", &[&err.to_string()]);
                        self.status = msg;
                        self.push_log(self.status.clone());
                    }
                }
            }

            if ui
                .button(t_load_config.clone())
                .on_hover_text(t_load_config_hover.clone())
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new().add_filter("Config", &["toml", "cfg"]).pick_file() {
                    match load_config_from_path(&path) {
                        Ok(cfg) => {
                            self.cfg = cfg;
                            self.extension_files =
                                list_sideloaded_extensions(&self.paths).unwrap_or_default();
                            let msg = self.t("options.config_loaded", &[&path.display().to_string()]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                        }
                        Err(err) => {
                            let msg = self.t("options.config_load_fail", &[&err.to_string()]);
                            self.status = msg;
                            self.push_log(self.status.clone());
                        }
                    }
                }
            }

            if ui
                .button(t_reset_config.clone())
                .on_hover_text(t_reset_config_hover.clone())
                .clicked()
            {
                self.cfg = crate::backend::LauncherConfig::default();
                self.extension_files =
                    list_sideloaded_extensions(&self.paths).unwrap_or_default();
                self.status = self.t("options.config_reset", &[]);
                self.push_log(self.status.clone());
            }
        });

        ui.separator();
        ui.label(t_data_dirs);
        let dd = self.t("connection.user_state", &[&self.paths.user_data_dir.to_string_lossy()]);
        ui.label(dd);
        let dd = self.t("connection.local_state", &[&self.paths.local_data_dir.to_string_lossy()]);
        ui.label(dd);
        let dd = self.t("connection.logs_state", &[&self.paths.logs_dir.to_string_lossy()]);
        ui.label(dd);
        let dd = self.t("connection.clients_state", &[&self.paths.clients_dir.to_string_lossy()]);
        ui.label(dd);
        let dd = self.t("connection.extensions_state", &[&self.paths.extensions_dir.to_string_lossy()]);
        ui.label(dd);
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
    response
}
