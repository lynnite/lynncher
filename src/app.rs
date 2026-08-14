use eframe::egui;

mod actions;
mod loader_actions;
mod modals;
mod panels;
mod worker;

use crate::backend::{
    ensure_dirs, launcher_paths, load_config, list_sideloaded_extensions, HubServerEntry,
    LauncherConfig, LauncherPaths, ServerInfo, DEFAULT_AUTH_SERVER,
    DEFAULT_HUB_SERVER,
};

pub struct LauncherApp {
    paths: LauncherPaths,
    cfg: LauncherConfig,
    status: String,
    logs: Vec<String>,
    style_applied: bool,
    last_scheme: Option<crate::backend::ColorScheme>,
    servers: Vec<HubServerEntry>,
    selected_server_address: Option<String>,
    selected_server_info: Option<ServerInfo>,
    login_username: String,
    login_password: String,
    direct_connect_input: String,
    hub_search_query: String,
    hub_filters_visible: bool,
    hub_filter_tags: Vec<String>,
    show_add_account_modal: bool,
    show_proxy_modal: bool,
    show_direct_connect_modal: bool,
    show_rename_modal: Option<String>,
    new_proxy_preset: String,
    extension_files: Vec<String>,
    page: AppPage,
    logo_texture: Option<egui::TextureHandle>,
    background_texture: Option<egui::TextureHandle>,
    background_sig: Option<String>,
    progress: Option<ProgressState>,
    background: Option<worker::BackgroundWork>,
    favorite_name_inputs: std::collections::HashMap<String, String>,
    update_check: std::sync::Arc<std::sync::Mutex<UpdateCheckState>>,
}

#[derive(Default, Clone)]
struct ProgressState {
    fraction: f32,
    label: String,
}

#[derive(Default)]
struct UpdateCheckState {
    checking: bool,
    done: bool,
    version: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppPage {
    Favorites,
    Hub,
    Options,
}

impl LauncherApp {
    pub fn new(initial_uri: Option<String>) -> Self {
        let paths = launcher_paths();

        let mut logs = Vec::new();
        let mut status = String::from("Ready");

        if let Err(err) = ensure_dirs(&paths) {
            status = format!("Failed to create launcher directories: {err:#}");
            logs.push(status.clone());
        }

        let mut cfg = match load_config(&paths) {
            Ok(c) => c,
            Err(err) => {
                let msg = format!("Failed to load config, using defaults: {err:#}");
                logs.push(msg.clone());
                status = msg;
                LauncherConfig::default()
            }
        };

        if let Some(uri) = initial_uri {
            cfg.connect_uri = uri;
        }

        if cfg.hub_server_url.trim().is_empty() {
            cfg.hub_server_url = DEFAULT_HUB_SERVER.to_string();
        }

        if cfg.auth_server_url.trim().is_empty() {
            cfg.auth_server_url = DEFAULT_AUTH_SERVER.to_string();
        }

        let extension_files = list_sideloaded_extensions(&paths).unwrap_or_default();

        Self {
            paths,
            cfg,
            status,
            logs,
            style_applied: false,
            last_scheme: None,
            servers: Vec::new(),
            selected_server_address: None,
            selected_server_info: None,
            login_username: String::new(),
            login_password: String::new(),
            direct_connect_input: String::new(),
            hub_search_query: String::new(),
            hub_filters_visible: false,
            hub_filter_tags: Vec::new(),
            show_add_account_modal: false,
            show_proxy_modal: false,
            show_direct_connect_modal: false,
            show_rename_modal: None,
            new_proxy_preset: String::new(),
            extension_files,
            page: AppPage::Favorites,
            logo_texture: None,
            background_texture: None,
            background_sig: None,
            progress: None,
            background: None,
            favorite_name_inputs: std::collections::HashMap::new(),
            update_check: std::sync::Arc::new(std::sync::Mutex::new(UpdateCheckState::default())),
        }
    }

    pub(crate) fn set_progress(&mut self, fraction: f32, label: impl Into<String>) {
        self.progress = Some(ProgressState {
            fraction: fraction.clamp(0.0, 1.0),
            label: label.into(),
        });
    }

    pub(crate) fn clear_progress(&mut self) {
        self.progress = None;
    }

        fn draw_progress(&mut self, ui: &mut egui::Ui) {
        let progress = self.progress.clone();
        let Some(progress) = progress else {
            if self.connection_active() {
                ui.horizontal(|ui| {
                    ui.label("Connecting...");
                    if self.cancel_button(ui) {
                        self.cancel_connection();
                    }
                });
            }
            return;
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(&progress.label);
                let side = 18.0;
                let height = (3.0_f32.sqrt() / 2.0) * side;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(height + 4.0, side + 4.0),
                    egui::Sense::hover(),
                );
                let painter = ui.painter();

                let frac = progress.fraction.clamp(0.0, 1.0);
                let apex_x = rect.right() - 2.0;
                let base_x = apex_x - height;
                let center_y = rect.center().y;
                let base_half = side / 2.0;

                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(base_x, center_y - base_half),
                        egui::pos2(base_x, center_y + base_half),
                        egui::pos2(apex_x, center_y),
                    ],
                    egui::Color32::from_rgb(0x36, 0x36, 0x36),
                    egui::Stroke::NONE,
                ));

                if frac > 0.0 {
                    let fill_x = base_x + height * frac;
                    let half = base_half * frac;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(apex_x, center_y),
                            egui::pos2(fill_x, center_y - half),
                            egui::pos2(fill_x, center_y + half),
                        ],
                        egui::Color32::from_rgb(0xBF, 0xBF, 0xBF),
                        egui::Stroke::NONE,
                    ));
                }
            });

            if self.cancel_button(ui) {
                self.cancel_connection();
            }
        });
    }

    fn cancel_button(&mut self, ui: &mut egui::Ui) -> bool {
        let btn = egui::Button::new(egui::RichText::new("Cancel").size(11.0))
            .min_size(egui::vec2(72.0, 28.0))
            .rounding(0.0)
            .fill(egui::Color32::from_rgb(0x50, 0x18, 0x18))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x8A, 0x20, 0x20)));
        ui.add(btn)
            .on_hover_text("Stop connecting and remove partially downloaded files")
            .clicked()
    }

}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_flat_style(ctx);
        self.poll_background();

        self.draw_header_panel(ctx);

        self.sync_background(ctx);

        let page = self.page;

        let bg_fill = egui::Color32::from_rgb(
            self.cfg.color_scheme.bg_r,
            self.cfg.color_scheme.bg_g,
            self.cfg.color_scheme.bg_b,
        );

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(bg_fill))
            .show(ctx, |ui| {
                if let Some(tex) = self.background_texture.clone() {
                    let bg_cfg = self.cfg.background_image_config.clone();
                    let screen = ui.max_rect();
                    let size = tex.size_vec2();
                    const START_SCALE: f32 = 0.1;
                    let base = (screen.width() / size.x).max(screen.height() / size.y)
                        * START_SCALE
                        * bg_cfg.scale.max(0.01);
                    let draw_size = egui::vec2(size.x * base, size.y * base);
                    let center = screen.center() + egui::vec2(bg_cfg.pos_x, bg_cfg.pos_y);
                    let pos = center - draw_size * 0.5;
                    let draw_rect = egui::Rect::from_min_size(pos, draw_size);
                    let uv = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(1.0, 1.0),
                    );
                    ui.painter().image(tex.id(), draw_rect, uv, egui::Color32::WHITE);
                }

                match page {
                    AppPage::Favorites => self.draw_favorites_page(ui),
                    AppPage::Hub => self.draw_hub_page(ui),
                    AppPage::Options => self.draw_options_page(ui),
                }
            });

        self.draw_footer_panel(ctx);

        self.render_add_account_modal(ctx);
        self.render_proxy_modal(ctx);
        self.render_direct_connect_modal(ctx);
        self.render_rename_modal(ctx);
    }
}

impl LauncherApp {
    fn sync_background(&mut self, ctx: &egui::Context) {
        let path = self.cfg.background_image.trim().to_string();
        let sig = if path.is_empty() {
            String::new()
        } else {
            let len = std::fs::File::open(&path)
                .ok()
                .and_then(|f| f.metadata().ok())
                .map(|m| m.len());
            format!("{path}:{len:?}")
        };
        if self.background_sig.as_deref() == Some(sig.as_str()) {
            return;
        }
        self.background_sig = Some(sig);
        self.background_texture = if path.is_empty() {
            None
        } else {
            load_background_texture(ctx, &path)
        };
    }

    fn draw_header_panel(&mut self, ctx: &egui::Context) {
        let cs = &self.cfg.color_scheme;
        let header_fill = egui::Color32::from_rgb(cs.header_r, cs.header_g, cs.header_b);
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::default().fill(header_fill))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        self.draw_logo(ui, ctx);
                        self.draw_progress(ui);
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.draw_account_menu(ui);

                        if ui.button("Discord").on_hover_text("Join the SS14 Discord").clicked() {
                            ui.ctx().open_url(egui::output::OpenUrl {
                                url: "https://discord.gg/Jg2r8JETyf".to_string(),
                                new_tab: true,
                            });
                        }
                    });
                });
                ui.add_space(8.0);

                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 2.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(0xD5, 0xD5, 0xD5));
            });
    }

    fn draw_logo(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.logo_texture.is_none() {
            self.logo_texture = load_logo_texture(ctx);
        }
        if let Some(tex) = &self.logo_texture {
            let size = egui::vec2(112.0, 81.0);
            ui.add(egui::Image::new(tex).fit_to_exact_size(size));
        } else {
            ui.label(
                egui::RichText::new("LYNNCHER")
                    .size(28.0)
                    .strong()
                    .color(egui::Color32::from_rgb(0xD5, 0xD5, 0xD5)),
            );
        }
    }

    fn draw_footer_panel(&mut self, ctx: &egui::Context) {
        let cs = &self.cfg.color_scheme;
        let footer_fill = egui::Color32::from_rgb(cs.footer_r, cs.footer_g, cs.footer_b);
        egui::TopBottomPanel::bottom("footer")
            .frame(egui::Frame::default().fill(footer_fill))
            .show(ctx, |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.page, AppPage::Favorites, "Home");
                    ui.selectable_value(&mut self.page, AppPage::Hub, "Servers");
                    ui.selectable_value(&mut self.page, AppPage::Options, "Options");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Direct Connect").clicked() {
                            self.open_direct_connect_modal();
                        }
                        ui.label(
                            egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                .weak()
                                .small(),
                        );
                    });
                });
                ui.add_space(5.0);
            });
    }

    pub(super) fn open_direct_connect_modal(&mut self) {
        self.show_direct_connect_modal = true;
    }

    fn render_direct_connect_modal(&mut self, ctx: &egui::Context) {
        if !self.show_direct_connect_modal {
            return;
        }

        let mut open = self.show_direct_connect_modal;
        let mut close_requested = false;
        egui::Window::new("direct_connect")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(
                    egui::RichText::new("Direct Connect")
                        .strong()
                        .size(16.0)
                        .color(egui::Color32::from_rgb(0xD5, 0xD5, 0xD5)),
                );
                ui.add_space(8.0);
                ui.label("Enter a server address (e.g. ss14://194.97.20.81:1212)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.direct_connect_input)
                        .hint_text("ss14://address")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Connect").clicked() {
                        let target = self.direct_connect_input.trim().to_string();
                        if target.is_empty() {
                            self.status = String::from("Direct connect address is empty");
                            self.push_log(self.status.clone());
                        } else {
                            self.connect_direct(&target);
                        }
                        self.show_direct_connect_modal = false;
                        close_requested = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close_requested = true;
                    }
                });
            });
        if close_requested {
            self.show_direct_connect_modal = false;
        }
        let _ = open;
    }
}

fn load_background_texture(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return None;
    }
    let img = image::open(p).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let color_image =
        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture(
        "launcher_background",
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

fn load_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let mut paths: Vec<std::path::PathBuf> = vec!["logo.png".into()];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("logo.png"));
            paths.push(dir.join("../../logo.png"));
        }
    }

    for p in &paths {
        if !p.exists() {
            continue;
        }
        let Ok(img) = image::open(p) else {
            continue;
        };
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        );
        return Some(ctx.load_texture(
            "launcher_logo",
            color_image,
            egui::TextureOptions::LINEAR,
        ));
    }
    None
}
