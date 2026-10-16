//! Imgur upload window (Flameshot's `ImgUploaderBase`): uploads in the
//! background, then offers to copy / open / delete the link.

use std::sync::mpsc;

use egui::{RichText, TextureHandle};
use image::RgbaImage;

use crate::config::Config;
use crate::export;

enum State {
    Uploading,
    Done(String),
    Failed(String),
}

struct UploadApp {
    cfg: Config,
    img: RgbaImage,
    tex: Option<TextureHandle>,
    rx: mpsc::Receiver<anyhow::Result<String>>,
    state: State,
    info: String,
}

pub fn run(img: RgbaImage, cfg: Config) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();
    {
        let img = img.clone();
        let cfg = cfg.clone();
        std::thread::spawn(move || {
            let _ = tx.send(export::upload_imgur(&img, &cfg));
        });
    }
    eframe::run_native(
        "Fastshot Upload",
        crate::ui::window_options("Upload to Imgur", [420.0, 400.0]),
        Box::new(move |cc| {
            crate::ui::setup_fonts(&cc.egui_ctx);
            Ok(Box::new(UploadApp {
                cfg,
                img,
                tex: None,
                rx,
                state: State::Uploading,
                info: String::new(),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}

impl eframe::App for UploadApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        if let Ok(r) = self.rx.try_recv() {
            self.state = match r {
                Ok(url) => {
                    if self.cfg.copy_url_after_upload && export::copy_text(&url).is_ok() {
                        self.info = "URL copied to clipboard.".into();
                        export::notify(&self.cfg, "URL copied to clipboard.", None);
                    }
                    State::Done(url)
                }
                Err(e) => State::Failed(format!("{e:#}")),
            };
        }
        if self.tex.is_none() {
            let (w, h) = self.img.dimensions();
            let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], self.img.as_raw());
            self.tex = Some(ctx.load_texture("upload", ci, Default::default()));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            let tex = self.tex.as_ref().unwrap();
            let s = tex.size_vec2();
            let k = (ui.available_width() / s.x).min(220.0 / s.y).min(1.0);
            ui.vertical_centered(|ui| ui.image((tex.id(), s * k)));
            ui.add_space(10.0);
            match &self.state {
                State::Uploading => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Uploading Image");
                    });
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                State::Failed(e) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, RichText::new("Upload failed").strong());
                    ui.label(e);
                }
                State::Done(url) => {
                    ui.hyperlink(url);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Copy URL").clicked() {
                            let _ = export::copy_text(url);
                            self.info = "URL copied to clipboard.".into();
                        }
                        if ui.button("Open URL").clicked() {
                            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                        }
                        if let Some(e) = export::load_history().first()
                            && e.url == *url
                            && !e.delete_url.is_empty()
                            && ui.button("Delete image").clicked()
                        {
                            let _ = std::process::Command::new("xdg-open").arg(&e.delete_url).spawn();
                        }
                        if ui.button("Image to Clipboard").clicked() {
                            let _ = export::copy_image(&self.img, &self.cfg);
                            self.info = "Screenshot copied to clipboard.".into();
                        }
                    });
                }
            }
            if !self.info.is_empty() {
                ui.weak(&self.info);
            }
        });
    }
}
