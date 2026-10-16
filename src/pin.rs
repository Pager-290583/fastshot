//! Pinned screenshot: a frameless always-on-top window showing the capture
//! (Flameshot's `PinWidget`).
//!
//! * drag – move, wheel – zoom, Ctrl+wheel – opacity
//! * double click / Esc – close, right click – context menu

use std::path::Path;

use egui::{Color32, Rect, Sense, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use image::RgbaImage;

use crate::config::Config;
use crate::export;

const MARGIN: f32 = 8.0;

pub fn run(path: &Path, delete: bool, pos: Option<(f32, f32)>, scale: Option<f32>) -> anyhow::Result<()> {
    let img = image::open(path)?.to_rgba8();
    if delete {
        let _ = std::fs::remove_file(path);
    }
    // The image is in physical pixels; show it at 1:1 on the monitor it came from.
    let scale = scale.unwrap_or(1.0).max(0.1);
    let logical = vec2(img.width() as f32 / scale, img.height() as f32 / scale);
    let mut vp = egui::ViewportBuilder::default()
        .with_title("Fastshot Pin")
        .with_app_id("fastshot-pin")
        .with_icon(crate::icon::icon_data(64))
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_inner_size(logical + Vec2::splat(MARGIN * 2.0));
    if let Some((x, y)) = pos {
        vp = vp.with_position(pos2(x - MARGIN, y - MARGIN));
    }
    let opts = eframe::NativeOptions {
        viewport: vp,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "fastshot-pin",
        opts,
        Box::new(move |cc| {
            crate::ui::setup_fonts(&cc.egui_ctx);
            Ok(Box::new(PinApp {
                cfg: Config::load(),
                img,
                tex: None,
                base_size: logical,
                zoom: 1.0,
                opacity: 1.0,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}

struct PinApp {
    cfg: Config,
    img: RgbaImage,
    tex: Option<TextureHandle>,
    base_size: Vec2,
    zoom: f32,
    opacity: f32,
}

impl PinApp {
    fn texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if self.tex.is_none() {
            let (w, h) = self.img.dimensions();
            let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], self.img.as_raw());
            let opts = if self.cfg.antialiasing_pin_zoom {
                TextureOptions::LINEAR
            } else {
                TextureOptions::NEAREST
            };
            self.tex = Some(ctx.load_texture("pin", ci, opts));
        }
        self.tex.as_ref().unwrap().id()
    }

    fn resize(&self, ctx: &egui::Context) {
        let size = self.base_size * self.zoom + Vec2::splat(MARGIN * 2.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    fn rotate(&mut self, ctx: &egui::Context, right: bool) {
        self.img = if right {
            image::imageops::rotate90(&self.img)
        } else {
            image::imageops::rotate270(&self.img)
        };
        self.base_size = vec2(self.base_size.y, self.base_size.x);
        self.tex = None;
        self.resize(ctx);
    }
}

impl eframe::App for PinApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let tex = self.texture(ctx);
        let (scroll, ctrl, esc) = ctx.input(|i| {
            (
                crate::ui::wheel_steps(&i.events),
                i.modifiers.command,
                i.key_pressed(egui::Key::Escape),
            )
        });
        if esc {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if scroll != 0.0 {
            let step = scroll.signum();
            if ctrl {
                self.opacity = (self.opacity + step * 0.1).clamp(0.1, 1.0);
            } else {
                self.zoom = (self.zoom * if step > 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.05, 20.0);
                self.resize(ctx);
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let full = ui.max_rect();
                let rect = full.shrink(MARGIN);
                let p = ui.painter();
                // Soft shadow + accent border like Flameshot.
                for i in 0..(MARGIN as i32) {
                    let a = (40.0 * (1.0 - i as f32 / MARGIN)) as u8;
                    p.rect_stroke(
                        rect.expand(i as f32),
                        2.0,
                        egui::Stroke::new(1.0, Color32::from_black_alpha(a)),
                        egui::StrokeKind::Outside,
                    );
                }
                p.image(
                    tex,
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::from_white_alpha((self.opacity * 255.0) as u8),
                );
                p.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, crate::ui::color32(self.cfg.ui_color)),
                    egui::StrokeKind::Outside,
                );
                let resp = ui.interact(full, egui::Id::new("pin"), Sense::click_and_drag());
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                resp.context_menu(|ui| {
                    use egui_phosphor::regular as ic;
                    if ui.button(format!("{} Copy to clipboard", ic::COPY)).clicked() {
                        match export::copy_image(&self.img, &self.cfg) {
                            Ok(()) => export::notify(&self.cfg, "Capture saved to clipboard.", None),
                            Err(e) => export::notify_error(&self.cfg, &format!("Copy failed: {e:#}")),
                        }
                        ui.close();
                    }
                    if ui.button(format!("{} Save to file", ic::FLOPPY_DISK)).clicked() {
                        ui.close();
                        match export::save_dialog(&self.img, &self.cfg) {
                            Ok(Some(p)) => export::notify(
                                &self.cfg,
                                &format!("Capture saved as {}", p.display()),
                                Some(&p),
                            ),
                            Ok(None) => {}
                            Err(e) => export::notify_error(&self.cfg, &format!("Save failed: {e:#}")),
                        }
                    }
                    ui.separator();
                    if ui.button(format!("{} Rotate Right", ic::ARROW_CLOCKWISE)).clicked() {
                        self.rotate(ctx, true);
                        ui.close();
                    }
                    if ui.button(format!("{} Rotate Left", ic::ARROW_COUNTER_CLOCKWISE)).clicked() {
                        self.rotate(ctx, false);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Increase Opacity").clicked() {
                        self.opacity = (self.opacity + 0.1).min(1.0);
                    }
                    if ui.button("Decrease Opacity").clicked() {
                        self.opacity = (self.opacity - 0.1).max(0.1);
                    }
                    if ui.button("Reset").clicked() {
                        self.opacity = 1.0;
                        self.zoom = 1.0;
                        self.resize(ctx);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(format!("{} Close", ic::X)).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                if resp.hovered() {
                    resp.on_hover_text(format!(
                        "Zoom {:.0}% · Opacity {:.0}%\nWheel: zoom · Ctrl+Wheel: opacity · Double click: close",
                        self.zoom * 100.0,
                        self.opacity * 100.0
                    ));
                }
            });
    }
}
