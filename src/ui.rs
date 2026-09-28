use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Pos2, Rect, Rounding, Sense, Stroke,
    TextureHandle, TextureOptions, Vec2,
};

use crate::config::Kind;

pub const LOGO_BYTES: &[u8] = include_bytes!("../logo.png");
pub const APP_ICON_BYTES: &[u8] = include_bytes!("../icon.png");
pub const BACKGROUND_BYTES: &[u8] = include_bytes!("../background.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Connecting,
    Configuring,
    Starting,
    Done,
    Idle,
    Failed,
}

pub struct BootstrapState {
    pub phase: Phase,
    pub status: String,
    pub play_url: Option<String>,
    pub local_file: Option<String>,
    pub client: String,
    pub kind: Kind,
    pub install_only: bool,
    pub request_close: bool,
    pub error: Option<String>,
    pub progress: Option<f32>,
}

impl BootstrapState {
    pub fn new(
        play_url: Option<String>,
        local_file: Option<String>,
        client: String,
        kind: Kind,
        install_only: bool,
    ) -> Self {
        let status = if install_only {
            "Getting things ready".to_string()
        } else {
            "Connecting".to_string()
        };
        Self {
            phase: Phase::Connecting,
            status,
            play_url,
            local_file,
            client,
            kind,
            install_only,
            request_close: false,
            error: None,
            progress: None,
        }
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        let msg = message.into();
        self.phase = Phase::Failed;
        self.status = msg.clone();
        self.error = Some(msg);
    }
}

pub struct BootstrapApp {
    state: Arc<Mutex<BootstrapState>>,
    started_at: std::time::Instant,
    logo: Option<TextureHandle>,
    logo_aspect: f32,
    background: Option<TextureHandle>,
    displayed: f32,
    border_fixed: bool,
}

impl BootstrapApp {
    pub fn new(cc: &eframe::CreationContext<'_>, state: Arc<Mutex<BootstrapState>>) -> Self {
        let (logo, aspect) = match decode_png(&cc.egui_ctx, LOGO_BYTES, "memoria_logo") {
            Ok((tex, w, h)) => (Some(tex), (w as f32) / (h as f32).max(1.0)),
            Err(err) => {
                log::warn!("failed to load embedded logo: {err:#}");
                (None, 4.0)
            }
        };
        let background = match decode_png(&cc.egui_ctx, BACKGROUND_BYTES, "memoria_background") {
            Ok((tex, _, _)) => Some(tex),
            Err(err) => {
                log::warn!("failed to load embedded background: {err:#}");
                None
            }
        };
        Self {
            state,
            started_at: std::time::Instant::now(),
            logo,
            logo_aspect: aspect,
            background,
            displayed: 0.0,
            border_fixed: false,
        }
    }
}

fn decode_png(ctx: &egui::Context, bytes: &[u8], name: &str) -> anyhow::Result<(TextureHandle, u32, u32)> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    let pixels = img.into_raw();
    let color_image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
    let tex = ctx.load_texture(name, color_image, TextureOptions::LINEAR);
    Ok((tex, w, h))
}

pub fn app_icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    match image::load_from_memory(APP_ICON_BYTES) {
        Ok(img) => {
            let img = img.to_rgba8();
            let (w, h) = (img.width(), img.height());
            Some((img.into_raw(), w, h))
        }
        Err(err) => {
            log::warn!("failed to decode embedded icon for window: {err:#}");
            None
        }
    }
}

#[cfg(windows)]
fn strip_window_border() {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumThreadWindows;

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWCP_DONOTROUND: u32 = 1;
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

    unsafe extern "system" fn cb(hwnd: HWND, _l: LPARAM) -> BOOL {
        let corner = DWMWCP_DONOTROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const core::ffi::c_void,
            4,
        );
        let none = DWMWA_COLOR_NONE;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &none as *const _ as *const core::ffi::c_void,
            4,
        );
        TRUE
    }

    unsafe {
        EnumThreadWindows(GetCurrentThreadId(), Some(cb), 0);
    }
}

#[cfg(not(windows))]
fn strip_window_border() {}

impl eframe::App for BootstrapApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.4431, 0.0, 0.0078, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        if !self.border_fixed {
            strip_window_border();
            self.border_fixed = true;
        }

        let snapshot = {
            let s = self.state.lock().unwrap();
            (s.phase, s.status.clone(), s.request_close, s.error.clone(), s.progress)
        };
        if snapshot.2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let target = match snapshot.4 {
            Some(p) => p,
            None => 0.9 * (1.0 - (-self.started_at.elapsed().as_secs_f32() / 3.5).exp()),
        };
        if target > self.displayed {
            self.displayed += (target - self.displayed) * 0.16;
            if (target - self.displayed).abs() < 0.002 {
                self.displayed = target;
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let card = ui.max_rect();
                self.draw_background(ui.painter(), card);

                let close_rect = close_button_rect(card);
                let drag = ui.interact(card, egui::Id::new("winmove"), Sense::click_and_drag());
                if drag.drag_started() {
                    let over_close = ui
                        .ctx()
                        .pointer_interact_pos()
                        .map(|pos| close_rect.contains(pos))
                        .unwrap_or(false);
                    if !over_close {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }

                self.draw_logo(ui.painter(), card);
                if snapshot.0 == Phase::Failed {
                    if let Some(err) = &snapshot.3 {
                        draw_error(ui.painter(), card, &snapshot.1, err);
                    }
                } else {
                    draw_status(ui.painter(), card, &snapshot.1);
                    draw_progress(ui.painter(), card, self.displayed);
                }
                draw_close(ui, close_rect);
            });
    }
}

const ACCENT: Color32 = Color32::from_rgb(0xFF, 0xD1, 0xD1);
const TRACK: Color32 = Color32::from_rgba_premultiplied(0x33, 0x00, 0x00, 0x96);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const TEXT_MUTED: Color32 = Color32::from_rgb(0xFF, 0xD1, 0xD1);
const CLOSE_IDLE: Color32 = Color32::from_rgb(0xFF, 0xD1, 0xD1);
const CLOSE_HOVER: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const ERROR_TEXT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);

fn close_button_rect(card: Rect) -> Rect {
    let size = 30.0;
    Rect::from_min_size(Pos2::new(card.right() - size - 10.0, card.top() + 10.0), Vec2::splat(size))
}

fn draw_close(ui: &mut egui::Ui, btn: Rect) {
    let response = ui.interact(btn, egui::Id::new("close_btn"), Sense::click());
    let color = if response.hovered() { CLOSE_HOVER } else { CLOSE_IDLE };
    let painter = ui.painter();
    let c = btn.center();
    let r = 5.0;
    let w = if response.hovered() { 2.0_f32 } else { 1.6_f32 };
    painter.line_segment([Pos2::new(c.x - r, c.y - r), Pos2::new(c.x + r, c.y + r)], Stroke::new(w, color));
    painter.line_segment([Pos2::new(c.x + r, c.y - r), Pos2::new(c.x - r, c.y + r)], Stroke::new(w, color));
    if response.clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl BootstrapApp {
    fn draw_background(&self, p: &egui::Painter, card: Rect) {
        match &self.background {
            Some(tex) => {
                p.image(
                    tex.id(),
                    card,
                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                p.rect_filled(card, Rounding::ZERO, Color32::from_rgb(0xC3, 0x2F, 0x33));
            }
        }
    }

    fn draw_logo(&self, p: &egui::Painter, card: Rect) {
        let target_h = 70.0;
        let center_x = card.center().x;
        let top_y = card.top() + 40.0;
        match &self.logo {
            Some(tex) => {
                let target_w = (target_h * self.logo_aspect).min(card.width() - 88.0);
                let real_h = target_w / self.logo_aspect;
                let logo_rect = Rect::from_min_size(
                    Pos2::new(center_x - target_w * 0.5, top_y),
                    Vec2::new(target_w, real_h),
                );
                p.image(
                    tex.id(),
                    logo_rect,
                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                p.text(
                    Pos2::new(center_x, top_y + target_h * 0.5),
                    Align2::CENTER_CENTER,
                    "MEMORIA",
                    FontId::proportional(40.0),
                    TEXT_PRIMARY,
                );
            }
        }
    }
}

fn draw_status(p: &egui::Painter, card: Rect, status: &str) {
    p.text(
        Pos2::new(card.center().x, card.top() + 150.0),
        Align2::CENTER_CENTER,
        status,
        FontId::proportional(15.0),
        TEXT_PRIMARY,
    );
}

fn draw_progress(p: &egui::Painter, card: Rect, displayed: f32) {
    let track_h = 14.0;
    let margin_x = 42.0;
    let track_y = card.top() + 176.0;
    let track = Rect::from_min_max(
        Pos2::new(card.left() + margin_x, track_y),
        Pos2::new(card.right() - margin_x, track_y + track_h),
    );
    let rounding = Rounding::same(track_h * 0.5);
    p.rect_filled(track, rounding, TRACK);

    let w = (track.width() * displayed.clamp(0.0, 1.0)).max(track_h);
    let fill = Rect::from_min_size(track.min, Vec2::new(w, track_h));
    p.rect_filled(fill, rounding, ACCENT);
}

fn draw_error(p: &egui::Painter, card: Rect, status: &str, err: &str) {
    p.text(
        Pos2::new(card.center().x, card.top() + 150.0),
        Align2::CENTER_CENTER,
        status,
        FontId::proportional(15.0),
        ERROR_TEXT,
    );
    let short = if err.len() > 92 { &err[..92] } else { err };
    p.text(
        Pos2::new(card.center().x, card.top() + 176.0),
        Align2::CENTER_CENTER,
        short,
        FontId::proportional(11.0),
        TEXT_MUTED,
    );
}
