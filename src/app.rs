use eframe::egui;

mod actions;
mod loader_actions;
mod modals;
mod panels;
mod worker;

use crate::backend::{
    cleanup_stale_update, ensure_dirs, launcher_paths, load_config, list_sideloaded_extensions,
    HubServerEntry, LauncherConfig, LauncherPaths, ServerInfo, DEFAULT_AUTH_SERVER,
    DEFAULT_HUB_SERVER,
};

/// Launcher version string shown in the UI and used for update checks.
///
/// To change the launcher's version, edit this single constant. It defaults
/// to the Cargo package version, so you can also just bump `version` in
/// `Cargo.toml`; either way there's one obvious place to look.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    background_sig: Option<String>,
    background_items: Vec<BackgroundItem>,
    background_anim_start: Option<std::time::Instant>,
    background_edit_index: usize,
    background_decode:
        Option<std::sync::Arc<std::sync::Mutex<Option<Vec<BackgroundLoadResult>>>>>,
    background_decode_pending: bool,
    progress: Option<ProgressState>,
    background: Option<worker::BackgroundWork>,
    favorite_name_inputs: std::collections::HashMap<String, String>,
    favorite_infos: std::collections::HashMap<String, ServerInfo>,
    update_check: std::sync::Arc<std::sync::Mutex<UpdateCheckState>>,
    update_action_result: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    last_config_save: Option<std::time::Instant>,
    last_saved_config: Option<String>,
    auto_update_initiated: bool,
}

#[derive(Default, Clone)]
struct ProgressState {
    fraction: f32,
    label: String,
}

#[derive(Clone)]
struct BackgroundAnimation {
    frames: Vec<egui::ColorImage>,
    delays_ms: Vec<u64>,
}

#[derive(Clone)]
struct BackgroundItem {
    path: String,
    pos_x: f32,
    pos_y: f32,
    scale: f32,
    texture: Option<egui::TextureHandle>,
    animation: Option<BackgroundAnimation>,
    anim_frame: usize,
}

struct BackgroundLoadResult {
    path: String,
    pos_x: f32,
    pos_y: f32,
    scale: f32,
    animation: Option<BackgroundAnimation>,
    static_image: Option<egui::ColorImage>,
}

#[derive(Default)]
struct UpdateCheckState {
    checking: bool,
    done: bool,
    version: Option<String>,
    url: Option<String>,
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

        cleanup_stale_update();

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
            background_sig: None,
            background_items: Vec::new(),
            background_anim_start: None,
            background_edit_index: 0,
            background_decode: None,
            background_decode_pending: false,
            progress: None,
            background: None,
            favorite_name_inputs: std::collections::HashMap::new(),
            favorite_infos: std::collections::HashMap::new(),
            update_check: std::sync::Arc::new(std::sync::Mutex::new(UpdateCheckState::default())),
            update_action_result: std::sync::Arc::new(std::sync::Mutex::new(None)),
            last_config_save: None,
            last_saved_config: None,
            auto_update_initiated: false,
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
        let connecting = self.connection_active();

        let Some(progress) = progress else {
            if connecting {
                ui.horizontal(|ui| {
                    ui.label("Connecting...");
                    if self.cancel_button(ui) {
                        self.cancel_connection();
                    }
                });
            }
            return;
        };

        let frac = progress.fraction.clamp(0.0, 1.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(&progress.label);
                let width = 200.0;
                let height = 10.0;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(width, height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter();

                painter.rect_filled(
                    rect,
                    0.0,
                    egui::Color32::from_rgb(0x36, 0x36, 0x36),
                );

                if frac > 0.0 {
                    let fill = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(width * frac, height),
                    );
                    painter.rect_filled(
                        fill,
                        0.0,
                        egui::Color32::from_rgb(0xBF, 0xBF, 0xBF),
                    );
                }
            });

            if connecting {
                if self.cancel_button(ui) {
                    self.cancel_connection();
                }
            }
        });
    }

    fn cancel_button(&mut self, ui: &mut egui::Ui) -> bool {
        let btn = egui::Button::new(egui::RichText::new("Cancel").size(11.0))
            .min_size(egui::vec2(72.0, 28.0))
            .rounding(0.0)
            .fill(egui::Color32::from_rgb(0x50, 0x18, 0x18))
            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x8A, 0x20, 0x20)));
        ui.add(btn)
            .on_hover_text("Stop connecting and remove partially downloaded files")
            .clicked()
    }

}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_flat_style(ctx);
        self.poll_background();
        self.poll_update_action();
        self.run_auto_update();
        self.save_config_if_dirty();

        self.draw_header_panel(ctx);

        self.draw_hub_search_bar(ctx);

        self.sync_background(ctx);
        self.poll_background_items(ctx);

        let page = self.page;

        let bg_fill = egui::Color32::from_rgb(
            self.cfg.color_scheme.bg_r,
            self.cfg.color_scheme.bg_g,
            self.cfg.color_scheme.bg_b,
        );

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(bg_fill))
            .show(ctx, |ui| {
                let screen = ui.max_rect();
                const START_SCALE: f32 = 0.5;
                for (i, item) in self.background_items.iter().enumerate() {
                    if let Some(tex) = &item.texture {
                        let (px, py, scale) = if !self.cfg.background_images.is_empty() {
                            let b = &self.cfg.background_images
                                [i.min(self.cfg.background_images.len() - 1)];
                            (b.pos_x, b.pos_y, b.scale)
                        } else {
                            (item.pos_x, item.pos_y, item.scale)
                        };
                        let size = tex.size_vec2();
                        let base = (screen.width() / size.x).max(screen.height() / size.y)
                            * START_SCALE
                            * scale.max(0.01);
                        let draw_size = egui::vec2(size.x * base, size.y * base);
                        let center = screen.center() + egui::vec2(px, py);
                        let pos = center - draw_size * 0.5;
                        let draw_rect = egui::Rect::from_min_size(pos, draw_size);
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(1.0, 1.0),
                        );
                        ui.painter()
                            .image(tex.id(), draw_rect, uv, egui::Color32::WHITE);
                    }
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
        let mut items: Vec<(String, f32, f32, f32)> = Vec::new();
        if !self.cfg.background_images.is_empty() {
            for img in &self.cfg.background_images {
                items.push((img.path.clone(), img.pos_x, img.pos_y, img.scale));
            }
        } else if !self.cfg.background_image.trim().is_empty() {
            let c = &self.cfg.background_image_config;
            items.push((
                self.cfg.background_image.clone(),
                c.pos_x,
                c.pos_y,
                c.scale,
            ));
        }

        let sig = items
            .iter()
            .map(|(p, _, _, _)| {
                let len = std::fs::File::open(p)
                    .ok()
                    .and_then(|f| f.metadata().ok())
                    .map(|m| m.len());
                format!("{p}:{len:?}")
            })
            .collect::<Vec<_>>()
            .join("|");

        if self.background_sig.as_deref() != Some(sig.as_str()) {
            self.background_sig = Some(sig);
            self.background_items.clear();
            self.background_anim_start = None;
            self.background_decode_pending = true;

            let slot: std::sync::Arc<
                std::sync::Mutex<Option<Vec<BackgroundLoadResult>>>,
            > = std::sync::Arc::new(std::sync::Mutex::new(None));
            self.background_decode = Some(slot.clone());

            std::thread::spawn(move || {
                let results: Vec<BackgroundLoadResult> = items
                    .into_iter()
                    .map(|(path, px, py, scale)| {
                        if let Some(animation) = load_background_animation(&path) {
                            BackgroundLoadResult {
                                path,
                                pos_x: px,
                                pos_y: py,
                                scale,
                                animation: Some(animation),
                                static_image: None,
                            }
                        } else {
                            let static_image = load_static_color_image(&path);
                            BackgroundLoadResult {
                                path,
                                pos_x: px,
                                pos_y: py,
                                scale,
                                animation: None,
                                static_image,
                            }
                        }
                    })
                    .collect();
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(results);
                }
            });
            return;
        }

        let any_anim = self.background_items.iter().any(|it| it.animation.is_some());
        if any_anim {
            let paused =
                self.cfg.pause_animations_unfocused && !ctx.input(|i| i.focused);
            if !paused {
                if let Some(dur) = self.advance_background_animations(ctx) {
                    ctx.request_repaint_after(dur);
                }
            }
        }
    }

    fn poll_background_items(&mut self, ctx: &egui::Context) {
        if !self.background_decode_pending {
            return;
        }

        let results = {
            let Some(slot) = &self.background_decode else {
                return;
            };
            let mut guard = match slot.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.take()
        };

        let Some(results) = results else {
            return; // decode still in progress; keep waiting
        };

        self.background_decode_pending = false;
        self.background_items.clear();
        for r in results {
            let mut item = BackgroundItem {
                path: r.path.clone(),
                pos_x: r.pos_x,
                pos_y: r.pos_y,
                scale: r.scale,
                texture: None,
                animation: r.animation.clone(),
                anim_frame: 0,
            };

            if let Some(anim) = &r.animation {
                if let Some(first) = anim.frames.first() {
                    item.texture = Some(ctx.load_texture(
                        "launcher_background",
                        first.clone(),
                        egui::TextureOptions::LINEAR,
                    ));
                }
            } else if let Some(img) = &r.static_image {
                item.texture = Some(ctx.load_texture(
                    "launcher_background",
                    img.clone(),
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.background_items.push(item);
        }

        let with_tex = self.background_items.iter().filter(|i| i.texture.is_some()).count();
        let with_anim = self.background_items.iter().filter(|i| i.animation.is_some()).count();
        self.status = format!(
            "Loaded {} background item(s): {} with texture, {} animated",
            self.background_items.len(),
            with_tex,
            with_anim
        );
        self.push_log(self.status.clone());
        self.background_anim_start = Some(std::time::Instant::now());
        ctx.request_repaint();
    }

    fn advance_background_animations(&mut self, _ctx: &egui::Context) -> Option<std::time::Duration> {
        let start = self.background_anim_start;
        let elapsed_ms = start?.elapsed().as_millis() as u64;
        let mut min_wait: Option<u64> = None;

        for item in &mut self.background_items {
            let Some(anim) = &item.animation else { continue };
            let frame_count = anim.frames.len();
            if frame_count == 0 {
                continue;
            }
            let total: u64 = anim.delays_ms.iter().sum();
            if total == 0 {
                continue;
            }
            let t = elapsed_ms % total;
            let mut acc = 0u64;
            let mut idx = 0usize;
            for (i, d) in anim.delays_ms.iter().enumerate() {
                if i == frame_count - 1 || t < acc + d {
                    idx = i;
                    break;
                }
                acc += d;
            }
            if idx != item.anim_frame {
                item.anim_frame = idx;
                if let Some(img) = anim.frames.get(idx) {
                    if let Some(tex) = &mut item.texture {
                        tex.set(img.clone(), egui::TextureOptions::LINEAR);
                    }
                }
            }
            let frame_elapsed = t.saturating_sub(acc);
            let wait_ms = anim.delays_ms[idx].saturating_sub(frame_elapsed).max(1);
            min_wait = Some(match min_wait {
                Some(m) => m.min(wait_ms),
                None => wait_ms,
            });
        }

        min_wait.map(std::time::Duration::from_millis)
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
                            egui::RichText::new(format!("v{APP_VERSION}"))
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

fn load_static_color_image(path: &str) -> Option<egui::ColorImage> {
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    ))
}

fn load_background_animation(path: &str) -> Option<BackgroundAnimation> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return None;
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "gif" {
        return load_gif_animation(path);
    }
    None
}

fn load_gif_animation(path: &str) -> Option<BackgroundAnimation> {
    use image::AnimationDecoder;
    let file = std::fs::File::open(path).ok()?;
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)).ok()?;
    let frames = decoder.into_frames().collect_frames().ok()?;
    if frames.is_empty() {
        return None;
    }

    let mut color_frames = Vec::with_capacity(frames.len());
    let mut delays_ms = Vec::with_capacity(frames.len());
    for frame in frames {
        let buffer = frame.buffer();
        let (w, h) = buffer.dimensions();
        color_frames.push(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            buffer.as_raw(),
        ));
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = if denom == 0 {
            100
        } else {
            ((numer as f64) / (denom as f64)) as u64
        };
        delays_ms.push(ms.max(1));
    }

    Some(BackgroundAnimation {
        frames: color_frames,
        delays_ms,
    })
}

fn load_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();

    let names = ["logo.png", "lynncher.png"];

    for name in names {
        paths.push(name.into());
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in names {
                paths.push(dir.join(name));
            }
            paths.push(dir.join("../../logo.png"));
            paths.push(dir.join("../../lynncher.png"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        for name in names {
            paths.push(std::path::PathBuf::from("/usr/share/pixmaps").join(name));
            paths.push(std::path::PathBuf::from("/usr/share/icons/hicolor/256x256/apps").join(name));
            paths.push(std::path::PathBuf::from("/usr/share/icons/hicolor/128x128/apps").join(name));
            paths.push(std::path::PathBuf::from("/usr/share/icons/hicolor/64x64/apps").join(name));
        }
    }
    #[cfg(target_os = "windows")]
    {
        for name in names {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    paths.push(dir.join(name));
                }
            }
        }
    }

    for p in &paths {
        if let Some(tex) = load_texture_from_file(ctx, p, "launcher_logo") {
            return Some(tex);
        }
    }

    load_texture_from_bytes(ctx, include_bytes!("../logo.png"), "launcher_logo")
}

fn load_texture_from_file(
    ctx: &egui::Context,
    path: &std::path::Path,
    name: &str,
) -> Option<egui::TextureHandle> {
    if !path.exists() || !path.is_file() {
        return None;
    }
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(
        name.to_string(),
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

fn load_texture_from_bytes(
    ctx: &egui::Context,
    bytes: &[u8],
    name: &str,
) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(
        name.to_string(),
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}
