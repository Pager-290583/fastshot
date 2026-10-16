//! Auxiliary windows: launcher, configuration, upload history, about.

use std::process::Command;

use egui::{Color32, RichText, TextureHandle, Vec2};

use crate::config::{Config, TextAlign};
use crate::export;
use crate::tools::{Button, shortcut_table};
use crate::ui::{color32, setup_fonts, window_options};

fn run_window<A: eframe::App + 'static>(title: &str, size: [f32; 2], app: A) -> anyhow::Result<()> {
    eframe::run_native(
        title,
        window_options(title, size),
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}

fn spawn_self(args: &[String]) {
    if let Ok(exe) = std::env::current_exe()
        && let Err(e) = Command::new(exe).args(args).spawn()
    {
        eprintln!("fastshot: failed to start capture: {e}");
    }
}

// ------------------------------------------------------------ launcher ----

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Region,
    Full,
    Screen,
}

struct Launcher {
    mode: Mode,
    delay: f32,
    copy: bool,
    save: bool,
    pin: bool,
    upload: bool,
}

pub fn launcher() -> anyhow::Result<()> {
    run_window(
        "Fastshot Launcher",
        [380.0, 330.0],
        Launcher {
            mode: Mode::Region,
            delay: 0.0,
            copy: true,
            save: false,
            pin: false,
            upload: false,
        },
    )
}

impl eframe::App for Launcher {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(RichText::new(format!("{} Fastshot", egui_phosphor::regular::CAMERA)).size(22.0));
            ui.add_space(8.0);
            egui::Grid::new("launcher").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                ui.label("Capture Mode");
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(match self.mode {
                        Mode::Region => "Rectangular Region",
                        Mode::Full => "Full Screen (All Monitors)",
                        Mode::Screen => "Current Screen",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mode, Mode::Region, "Rectangular Region");
                        ui.selectable_value(&mut self.mode, Mode::Full, "Full Screen (All Monitors)");
                        ui.selectable_value(&mut self.mode, Mode::Screen, "Current Screen");
                    });
                ui.end_row();
                ui.label("Delay");
                ui.add(
                    egui::DragValue::new(&mut self.delay)
                        .range(0.0..=600.0)
                        .speed(0.1)
                        .custom_formatter(|v, _| {
                            if v == 0.0 { "No Delay".into() } else { format!("{v:.1} s") }
                        }),
                );
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.label(RichText::new("After capture").strong());
            ui.checkbox(&mut self.copy, "Copy to clipboard");
            ui.checkbox(&mut self.save, "Save to file (configured folder)");
            ui.checkbox(&mut self.pin, "Pin on the desktop");
            ui.checkbox(&mut self.upload, "Upload to Imgur");
            ui.add_space(12.0);
            let take = ui.add_sized(
                [ui.available_width(), 34.0],
                egui::Button::new(RichText::new("Take new screenshot").size(16.0)),
            );
            if take.clicked() {
                let mut args: Vec<String> = vec![
                    match self.mode {
                        Mode::Region => "gui",
                        Mode::Full => "full",
                        Mode::Screen => "screen",
                    }
                    .into(),
                ];
                if self.delay > 0.0 {
                    args.push("--delay".into());
                    args.push(((self.delay * 1000.0) as u64).to_string());
                }
                if self.copy {
                    args.push("--clipboard".into());
                }
                if self.save {
                    let cfg = Config::load();
                    args.push("--path".into());
                    args.push(export::expand_home(&cfg.save_path));
                }
                if self.pin && self.mode != Mode::Full {
                    args.push("--pin".into());
                }
                if self.upload {
                    args.push("--upload".into());
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                spawn_self(&args);
            }
        });
    }
}

// -------------------------------------------------------------- config ----

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Interface,
    Filename,
    General,
    Shortcuts,
}

struct ConfigApp {
    cfg: Config,
    saved: Config,
    tab: Tab,
    status: String,
}

pub fn config() -> anyhow::Result<()> {
    let cfg = Config::load();
    run_window(
        "Fastshot Configuration",
        [640.0, 600.0],
        ConfigApp {
            saved: cfg.clone(),
            cfg,
            tab: Tab::Interface,
            status: String::new(),
        },
    )
}

fn color_edit(ui: &mut egui::Ui, label: &str, c: &mut [u8; 4]) {
    ui.horizontal(|ui| {
        let mut col = color32(*c);
        if ui.color_edit_button_srgba(&mut col).changed() {
            *c = col.to_srgba_unmultiplied();
        }
        ui.label(label);
    });
}

impl ConfigApp {
    fn interface(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.cfg;
        ui.heading("UI Color Editor");
        color_edit(ui, "Main color", &mut c.ui_color);
        color_edit(ui, "Contrast color", &mut c.contrast_color);
        ui.horizontal(|ui| {
            if ui.button("Reset colors").clicked() {
                let d = Config::default();
                c.ui_color = d.ui_color;
                c.contrast_color = d.contrast_color;
            }
        });
        ui.add_space(8.0);
        ui.label("Opacity of area outside selection");
        ui.add(egui::Slider::new(&mut c.contrast_opacity, 0..=255));
        ui.label("Button size");
        ui.add(egui::Slider::new(&mut c.button_size, 20.0..=56.0).integer());
        ui.add_space(8.0);
        ui.checkbox(&mut c.show_help, "Show help message");
        ui.checkbox(&mut c.show_selection_geometry, "Show selection geometry");
        ui.checkbox(&mut c.show_magnifier, "Show magnifier");
        ui.checkbox(&mut c.square_magnifier, "Square shaped magnifier");
        ui.add_space(8.0);
        ui.heading("Button Selection");
        egui::Grid::new("buttons").num_columns(2).show(ui, |ui| {
            for (i, b) in Button::all().into_iter().enumerate() {
                let mut on = c.buttons.contains(&b);
                if ui
                    .checkbox(&mut on, format!("{}  {}", b.icon(), b.name()))
                    .on_hover_text(b.description())
                    .changed()
                {
                    if on {
                        // keep catalogue order
                        let order = Button::all();
                        c.buttons.push(b);
                        c.buttons.sort_by_key(|x| order.iter().position(|y| y == x));
                    } else {
                        c.buttons.retain(|x| *x != b);
                    }
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Select all").clicked() {
                c.buttons = Button::all();
            }
            if ui.button("Defaults").clicked() {
                c.buttons = Button::default_set();
            }
        });
        ui.add_space(8.0);
        ui.heading("Color Wheel Palette");
        ui.horizontal_wrapped(|ui| {
            let mut remove = None;
            for (i, col) in c.user_colors.iter_mut().enumerate() {
                let mut c32 = color32(*col);
                let r = ui.color_edit_button_srgba(&mut c32);
                if r.changed() {
                    *col = c32.to_srgba_unmultiplied();
                }
                if r.secondary_clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                c.user_colors.remove(i);
            }
            if ui.button("+").clicked() {
                c.user_colors.push([255, 255, 255, 255]);
            }
        });
        ui.weak("Right click a color to remove it.");
    }

    fn filename(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.cfg;
        ui.heading("Filename Editor");
        ui.label("Edit the name of your captures:");
        ui.text_edit_singleline(&mut c.filename_pattern);
        ui.label(format!(
            "Preview: {}.{}",
            export::format_filename(&c.filename_pattern),
            export::default_extension(c)
        ));
        ui.add_space(6.0);
        let tokens = [
            ("%Y", "Year (2000)"),
            ("%y", "Year (00)"),
            ("%m", "Month (01-12)"),
            ("%B", "Month name"),
            ("%b", "Abbreviated month"),
            ("%d", "Day (01-31)"),
            ("%e", "Day ( 1-31)"),
            ("%A", "Weekday"),
            ("%a", "Abbreviated weekday"),
            ("%H", "Hour (00-23)"),
            ("%I", "Hour (01-12)"),
            ("%M", "Minute (00-59)"),
            ("%S", "Second (00-59)"),
            ("%p", "AM/PM"),
            ("%j", "Day of year"),
            ("%U", "Week number"),
            ("%F", "Date (%Y-%m-%d)"),
            ("%T", "Time (%H:%M:%S)"),
            ("%D", "Date (%m/%d/%y)"),
            ("%s", "Unix timestamp"),
        ];
        egui::Grid::new("tokens").num_columns(4).striped(true).show(ui, |ui| {
            for (i, (tok, desc)) in tokens.iter().enumerate() {
                if ui.button(*tok).on_hover_text("Append to the pattern").clicked() {
                    c.filename_pattern.push_str(tok);
                }
                ui.label(*desc);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Reset").clicked() {
                c.filename_pattern = Config::default().filename_pattern;
            }
            if ui.button("Clear").clicked() {
                c.filename_pattern.clear();
            }
        });
    }

    fn general(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.cfg;
        ui.heading("General");
        ui.checkbox(&mut c.show_desktop_notification, "Show desktop notifications");
        ui.checkbox(&mut c.show_tray_icon, "Show tray icon");
        ui.checkbox(&mut c.autostart, "Launch at startup");
        ui.checkbox(&mut c.copy_on_double_click, "Copy on double click");
        ui.checkbox(&mut c.save_after_copy, "Save image after copy");
        ui.checkbox(&mut c.copy_path_after_save, "Copy file path after save");
        ui.checkbox(&mut c.use_jpg_for_clipboard, "Use JPG format for clipboard (PNG default)");
        ui.checkbox(&mut c.save_last_region, "Remember last region (--last-region)");
        ui.checkbox(&mut c.antialiasing_pin_zoom, "Anti-aliasing when zooming the pinned image");
        ui.checkbox(&mut c.upload_without_confirmation, "Upload image without confirmation");
        ui.checkbox(&mut c.copy_url_after_upload, "Copy URL after upload");
        ui.horizontal(|ui| {
            ui.label("JPEG quality");
            ui.add(egui::Slider::new(&mut c.jpeg_quality, 1..=100));
        });
        ui.horizontal(|ui| {
            ui.label("Undo limit");
            ui.add(egui::DragValue::new(&mut c.undo_limit).range(1..=1000));
        });
        ui.horizontal(|ui| {
            ui.label("Latest uploads to keep");
            ui.add(egui::DragValue::new(&mut c.upload_history_max).range(1..=200));
        });
        ui.horizontal(|ui| {
            ui.label("Imgur Client ID");
            ui.text_edit_singleline(&mut c.imgur_client_id);
        });
        ui.add_space(10.0);
        ui.heading("Saving");
        ui.horizontal(|ui| {
            ui.label("Save path");
            ui.text_edit_singleline(&mut c.save_path);
            if ui.button("Browse…").clicked()
                && let Some(p) = rfd::FileDialog::new().set_directory(export::expand_home(&c.save_path)).pick_folder()
            {
                c.save_path = p.to_string_lossy().into_owned();
            }
        });
        ui.checkbox(&mut c.save_path_fixed, "Use fixed path for screenshots to save (no dialog)");
        ui.horizontal(|ui| {
            ui.label("Preferred save file extension");
            egui::ComboBox::from_id_salt("ext")
                .selected_text(c.save_as_file_extension.clone())
                .show_ui(ui, |ui| {
                    for e in ["png", "jpg", "webp", "bmp"] {
                        ui.selectable_value(&mut c.save_as_file_extension, e.into(), e);
                    }
                });
        });
        ui.add_space(10.0);
        ui.heading("Text tool");
        ui.horizontal(|ui| {
            ui.label("Font family");
            ui.text_edit_singleline(&mut c.font_family);
        });
        ui.horizontal(|ui| {
            ui.label("Default alignment");
            ui.selectable_value(&mut c.text_align, TextAlign::Left, "Left");
            ui.selectable_value(&mut c.text_align, TextAlign::Center, "Center");
            ui.selectable_value(&mut c.text_align, TextAlign::Right, "Right");
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Open config file").clicked() {
                let _ = self.saved.save();
                export::open_path(&crate::config::config_path());
            }
            if ui.button("Reset all settings").clicked() {
                *c = Config::default();
            }
        });
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        ui.heading("Shortcuts");
        egui::Grid::new("shortcuts").num_columns(2).striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
            for (k, d) in shortcut_table() {
                ui.label(RichText::new(k).monospace().strong());
                ui.label(d);
                ui.end_row();
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Global shortcut").strong());
        ui.label(
            "Wayland does not let applications grab keys globally. Bind the Print key to \
             `fastshot gui` in your desktop settings, or let Fastshot do it for GNOME:",
        );
        if crate::is_sandboxed() {
            ui.label(
                "In the Flatpak version the tray registers the Print key through the \
                 Global Shortcuts portal; your desktop asks to confirm it on first start.",
            );
        } else if ui.button("Bind Print Screen to fastshot gui (GNOME)").clicked() {
            self.status = match crate::shortcut::install_gnome_binding("Print") {
                Ok(()) => "Shortcut installed: Print → fastshot gui".into(),
                Err(e) => format!("Failed: {e:#}"),
            };
        }
    }
}

impl eframe::App for ConfigApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let _ctx = &ui.ctx().clone();
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                use egui_phosphor::regular as ic;
                ui.selectable_value(&mut self.tab, Tab::Interface, format!("{} Interface", ic::SLIDERS));
                ui.selectable_value(&mut self.tab, Tab::Filename, format!("{} Filename Editor", ic::TEXT_T));
                ui.selectable_value(&mut self.tab, Tab::General, format!("{} General", ic::GEAR));
                ui.selectable_value(&mut self.tab, Tab::Shortcuts, format!("{} Shortcuts", ic::DOTS_NINE));
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.weak(crate::config::config_path().display().to_string());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(&self.status);
                });
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Interface => self.interface(ui),
                Tab::Filename => self.filename(ui),
                Tab::General => self.general(ui),
                Tab::Shortcuts => self.shortcuts(ui),
            });
        });
        // Settings apply immediately, like in Flameshot.
        if toml::to_string(&self.cfg).ok() != toml::to_string(&self.saved).ok() {
            if self.cfg.autostart != self.saved.autostart {
                crate::autostart::sync(self.cfg.autostart);
            }
            match self.cfg.save() {
                Ok(()) => {
                    self.saved = self.cfg.clone();
                    self.status = "Saved".into();
                }
                Err(e) => self.status = format!("Error: {e}"),
            }
        }
    }
}

// ------------------------------------------------------------- history ----

struct HistoryApp {
    entries: Vec<export::HistoryEntry>,
    thumbs: Vec<Option<TextureHandle>>,
    cfg: Config,
}

pub fn history() -> anyhow::Result<()> {
    let entries = export::load_history();
    run_window(
        "Fastshot Upload History",
        [560.0, 600.0],
        HistoryApp {
            thumbs: vec![None; entries.len()],
            entries,
            cfg: Config::load(),
        },
    )
}

impl eframe::App for HistoryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Latest Uploads");
            if self.entries.is_empty() {
                ui.label("Screenshots history is empty");
                return;
            }
            let mut remove = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, e) in self.entries.iter().enumerate() {
                    if self.thumbs[i].is_none()
                        && let Ok(img) = image::open(&e.thumbnail)
                    {
                        let img = img.to_rgba8();
                        let ci = egui::ColorImage::from_rgba_unmultiplied(
                            [img.width() as usize, img.height() as usize],
                            img.as_raw(),
                        );
                        self.thumbs[i] = Some(ctx.load_texture(format!("t{i}"), ci, Default::default()));
                    }
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            if let Some(t) = &self.thumbs[i] {
                                let s = t.size_vec2();
                                let k = (128.0 / s.x).min(96.0 / s.y);
                                ui.image((t.id(), s * k));
                            } else {
                                ui.allocate_space(Vec2::new(128.0, 96.0));
                            }
                            ui.vertical(|ui| {
                                let when = chrono::DateTime::from_timestamp(e.timestamp, 0)
                                    .map(|d| d.with_timezone(&chrono::Local).format("%F %T").to_string())
                                    .unwrap_or_default();
                                ui.label(RichText::new(when).color(Color32::GRAY));
                                ui.hyperlink(&e.url);
                                ui.horizontal(|ui| {
                                    if ui.button("Copy URL").clicked() {
                                        let _ = export::copy_text(&e.url);
                                    }
                                    if ui.button("Open").clicked() {
                                        let _ = Command::new("xdg-open").arg(&e.url).spawn();
                                    }
                                    if !e.delete_url.is_empty() && ui.button("Delete from Imgur").clicked() {
                                        let _ = Command::new("xdg-open").arg(&e.delete_url).spawn();
                                    }
                                    if ui.button("Remove").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            });
                        });
                    });
                }
            });
            if let Some(i) = remove {
                let e = self.entries.remove(i);
                self.thumbs.remove(i);
                let _ = std::fs::remove_file(e.thumbnail);
                export::save_history(&self.entries);
            }
            let _ = &self.cfg;
        });
    }
}

// --------------------------------------------------------------- about ----

struct About;

pub fn about() -> anyhow::Result<()> {
    run_window("About Fastshot", [460.0, 420.0], About)
}

impl eframe::App for About {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let _ctx = &ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(egui_phosphor::regular::CAMERA).size(48.0));
                ui.heading(format!("Fastshot {}", env!("CARGO_PKG_VERSION")));
                ui.label("Powerful yet simple to use screenshot software.");
                ui.label("A Rust re-implementation of the Flameshot workflow.");
                ui.add_space(8.0);
                ui.label(format!("License: {}", env!("CARGO_PKG_LICENSE")));
            });
            ui.add_space(10.0);
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("about_sc").num_columns(2).striped(true).show(ui, |ui| {
                    for (k, d) in shortcut_table() {
                        ui.label(RichText::new(k).monospace());
                        ui.label(d);
                        ui.end_row();
                    }
                });
            });
        });
    }
}

// -------------------------------------------------------- grant access ----

struct GrantAccess {
    state: GrantState,
}

#[derive(PartialEq)]
enum GrantState {
    Idle,
    Requested,
    Failed(String),
}

/// Exits with status 0 once the screenshot portal accepted a request.
pub fn grant_access() -> anyhow::Result<()> {
    let granted = std::rc::Rc::new(std::cell::Cell::new(false));
    let out = granted.clone();
    eframe::run_native(
        "Fastshot",
        window_options("Fastshot — screenshot permission", [440.0, 230.0]),
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(GrantApp { app: GrantAccess { state: GrantState::Idle }, granted: out }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;
    if granted.get() {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

struct GrantApp {
    app: GrantAccess,
    granted: std::rc::Rc<std::cell::Cell<bool>>,
}

impl eframe::App for GrantApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        if self.app.state == GrantState::Requested {
            // Runs while our window is focused, so GNOME may show its dialog.
            match crate::capture::probe_portal() {
                Ok(()) => {
                    self.granted.set(true);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(e) => self.app.state = GrantState::Failed(format!("{e:#}")),
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(egui_phosphor::regular::CAMERA).size(40.0));
                ui.vertical(|ui| {
                    ui.heading("Allow Fastshot to take screenshots");
                    ui.label(
                        "Your desktop asks for permission once. Press the button and \
                         choose “Allow” in the dialog that appears.",
                    );
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let busy = self.app.state == GrantState::Requested;
                if ui.add_enabled(!busy, egui::Button::new("Grant access")).clicked() {
                    self.app.state = GrantState::Requested;
                    ctx.request_repaint();
                }
                if ui.button("Cancel").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            if let GrantState::Failed(e) = &self.app.state {
                ui.add_space(8.0);
                ui.colored_label(Color32::LIGHT_RED, format!("Not granted: {e}"));
            }
        });
    }
}
