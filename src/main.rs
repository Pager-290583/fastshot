//! Fastshot — a Flameshot-compatible screenshot tool written in Rust.

mod annotation;
mod autostart;
mod capture;
mod config;
mod export;
mod icon;
mod overlay;
mod pin;
mod render;
mod script;
mod shortcut;
mod tools;
mod tray;
mod ui;
mod upload;
mod windows;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use image::RgbaImage;

use config::{Config, State};

/// Application ID (Flatpak, desktop entry, Wayland app_id).
pub const APP_ID: &str = "io.github.Pager_290583.Fastshot";

/// Running inside a Flatpak sandbox?
pub fn is_sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}
use overlay::{CaptureResult, ExitAction, OverlayOptions};

#[derive(Parser)]
#[command(name = "fastshot", version, about = "Powerful yet simple to use screenshot software")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start a manual capture in GUI mode
    Gui(GuiArgs),
    /// Capture the entire desktop
    Full(OutputArgs),
    /// Capture a single screen
    Screen(ScreenArgs),
    /// Open the capture launcher
    Launcher,
    /// Configure Fastshot (opens the configuration window without options)
    Config(ConfigArgs),
    /// Run in the system tray (default when no command is given)
    Tray,
    /// Pin an image file on the desktop
    Pin {
        file: PathBuf,
        /// Delete the file once loaded
        #[arg(long)]
        delete: bool,
        #[arg(long)]
        x: Option<f32>,
        #[arg(long)]
        y: Option<f32>,
        /// Image pixels per logical pixel (HiDPI scale of the source screen)
        #[arg(long)]
        scale: Option<f32>,
    },
    /// Show the history of uploaded screenshots
    History,
    /// Show information about Fastshot
    About,
    /// Set a global shortcut, e.g. `--key '<Shift><Super>4'` (empty key removes it)
    InstallShortcut {
        #[arg(long, default_value = "Print")]
        key: String,
        /// capture, full_clipboard, full_save or launcher
        #[arg(long, default_value = "capture")]
        action: String,
    },
    /// Ask the desktop for permission to take screenshots (Flatpak)
    GrantAccess,
    /// Internal: serve clipboard contents read from stdin (X11)
    #[command(hide = true)]
    ClipboardServe {
        #[arg(long, default_value = "image/png")]
        mime: String,
    },
}

#[derive(Args, Clone, Default)]
struct OutputArgs {
    /// Existing directory or new file to save to
    #[arg(short, long)]
    path: Option<PathBuf>,
    /// Save the capture to the clipboard
    #[arg(short, long)]
    clipboard: bool,
    /// Delay time in milliseconds
    #[arg(short, long, default_value_t = 0)]
    delay: u64,
    /// Print raw PNG capture to stdout
    #[arg(short, long)]
    raw: bool,
    /// Upload screenshot to Imgur
    #[arg(short, long)]
    upload: bool,
}

#[derive(Args, Clone, Default)]
struct GuiArgs {
    #[command(flatten)]
    out: OutputArgs,
    /// Print geometry of the selection in the format WxH+X+Y
    #[arg(short = 'g', long)]
    print_geometry: bool,
    /// Pin the capture to the screen
    #[arg(long)]
    pin: bool,
    /// Accept capture as soon as a selection is made
    #[arg(short = 's', long)]
    accept_on_select: bool,
    /// Initial selection: WxH+X+Y (image pixels)
    #[arg(long)]
    region: Option<String>,
    /// Repeat the last selected region
    #[arg(long)]
    last_region: bool,
}

#[derive(Args, Clone, Default)]
struct ScreenArgs {
    #[command(flatten)]
    out: OutputArgs,
    /// Define the screen to capture (starting from 0), default: primary
    #[arg(short, long)]
    number: Option<usize>,
    /// Pin the capture to the screen
    #[arg(long)]
    pin: bool,
}

#[derive(Args, Clone, Default)]
struct ConfigArgs {
    /// Enable or disable run at startup
    #[arg(long)]
    autostart: Option<bool>,
    /// Set the filename pattern
    #[arg(short, long)]
    filename: Option<String>,
    /// Enable or disable the tray icon
    #[arg(short, long)]
    trayicon: Option<bool>,
    /// Show the help message in the capture mode
    #[arg(short, long)]
    showhelp: Option<bool>,
    /// Define the main UI color (#RRGGBB)
    #[arg(short, long)]
    maincolor: Option<String>,
    /// Define the contrast UI color (#RRGGBB)
    #[arg(short = 'k', long)]
    contrastcolor: Option<String>,
    /// Check the configuration for errors
    #[arg(long)]
    check: bool,
    /// Open the window on the global shortcuts page
    #[arg(long)]
    shortcuts: bool,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("fastshot: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command.unwrap_or(Cmd::Tray) {
        Cmd::Gui(a) => gui(a),
        Cmd::Full(a) => full(a),
        Cmd::Screen(a) => screen(a),
        Cmd::Launcher => windows::launcher(),
        Cmd::Config(a) => config_cmd(a),
        Cmd::Tray => {
            let cfg = Config::load();
            if !cfg.show_tray_icon {
                eprintln!("fastshot: tray icon disabled in the configuration");
                return Ok(());
            }
            tray::run()
        }
        Cmd::Pin { file, delete, x, y, scale } => {
            pin::run(&file, delete, x.zip(y), scale)
        }
        Cmd::History => windows::history(),
        Cmd::About => windows::about(),
        Cmd::InstallShortcut { key, action } => {
            let a = shortcut::action(&action).ok_or_else(|| anyhow!("unknown action {action}"))?;
            let mut cfg = Config::load();
            if key.is_empty() {
                cfg.hotkeys.remove(a.id);
            } else {
                cfg.hotkeys.insert(a.id.into(), shortcut::normalize(&key));
            }
            cfg.save()?;
            if shortcut::gnome_available() {
                shortcut::apply(&cfg.hotkeys)?;
                match shortcut::display(&key) {
                    k if k.is_empty() => println!("{}: removed", a.label),
                    k => println!("{}: {k}", a.label),
                }
            } else {
                println!("Saved; the tray registers it through the Global Shortcuts portal on next start.");
            }
            Ok(())
        }
        Cmd::GrantAccess => windows::grant_access(),
        Cmd::ClipboardServe { mime } => export::clipboard_serve(&mime),
    }
}

fn parse_region(s: &str) -> Result<[i32; 4]> {
    // WxH+X+Y
    let err = || anyhow!("invalid region {s:?}, expected WxH+X+Y");
    let (wh, xy) = s.split_once('+').ok_or_else(err)?;
    let (w, h) = wh.split_once('x').ok_or_else(err)?;
    let (x, y) = xy.split_once('+').ok_or_else(err)?;
    Ok([x.parse()?, y.parse()?, w.parse()?, h.parse()?])
}

fn grab(delay_ms: u64) -> Result<RgbaImage> {
    if delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
    capture::grab_desktop()
}

fn gui(a: GuiArgs) -> Result<()> {
    let cfg = Config::load();
    let mut img = grab(a.out.delay)?;
    // The fullscreen overlay covers one monitor; with several, use the primary one.
    let monitors = capture::monitors(img.width());
    if monitors.len() > 1 {
        let m = monitors[0];
        img = image::imageops::crop_imm(&img, m[0], m[1], m[2], m[3]).to_image();
    }
    let region = match (&a.region, a.last_region) {
        (Some(r), _) => Some(parse_region(r)?),
        (None, true) => State::load().last_region,
        _ => None,
    };
    let opts = OverlayOptions {
        accept_on_select: a.accept_on_select,
        region,
    };
    let Some(res) = overlay::run(img, cfg.clone(), opts)? else {
        eprintln!("Screenshot aborted.");
        std::process::exit(1);
    };
    // Reload: the capture screen stores the last used color/sizes.
    let cfg = Config::load();
    if a.print_geometry {
        let [x, y, w, h] = res.region;
        println!("{w}x{h}+{x}+{y}");
    }
    dispatch(&cfg, res, &a.out, a.pin, a.print_geometry)
}

/// Perform what the user asked for when leaving the capture screen.
fn dispatch(cfg: &Config, res: CaptureResult, out: &OutputArgs, pin: bool, geometry: bool) -> Result<()> {
    let img = &res.image;
    match res.action {
        ExitAction::Accept => {
            let explicit = out.path.is_some() || out.clipboard || out.raw || out.upload || pin || geometry;
            if !explicit {
                return do_copy(cfg, img);
            }
            if let Some(p) = &out.path {
                do_save_to(cfg, img, p)?;
            }
            if out.clipboard {
                do_copy(cfg, img)?;
            }
            if out.raw {
                export::write_raw_stdout(img)?;
            }
            if pin {
                export::spawn_pin(img, Some(res.logical_pos), res.scale)?;
            }
            if out.upload {
                upload::run(img.clone(), cfg.clone())?;
            }
            Ok(())
        }
        ExitAction::Copy => do_copy(cfg, img),
        ExitAction::Save => match &out.path {
            Some(p) => do_save_to(cfg, img, p),
            None => match export::save_interactive(img, cfg) {
                Ok(Some(p)) => after_save(cfg, &p),
                Ok(None) => Ok(()),
                Err(e) => {
                    export::notify_error(cfg, &format!("Error trying to save the capture: {e:#}"));
                    Err(e)
                }
            },
        },
        ExitAction::Upload => upload::run(img.clone(), cfg.clone()),
        ExitAction::OpenApp => export::open_with(img),
        ExitAction::Pin => export::spawn_pin(img, Some(res.logical_pos), res.scale),
    }
}

fn do_copy(cfg: &Config, img: &RgbaImage) -> Result<()> {
    match export::copy_image(img, cfg) {
        Ok(()) => export::notify(cfg, "Capture saved to clipboard.", None),
        Err(e) => {
            export::notify_error(cfg, &format!("Unable to write to the clipboard: {e:#}"));
            return Err(e);
        }
    }
    if cfg.save_after_copy {
        let p = export::suggested_path(cfg);
        export::write_image(img, &p, cfg)?;
        after_save(cfg, &p)?;
    }
    Ok(())
}

fn do_save_to(cfg: &Config, img: &RgbaImage, target: &std::path::Path) -> Result<()> {
    let p = export::save_to(img, target, cfg)?;
    after_save(cfg, &p)
}

fn after_save(cfg: &Config, p: &std::path::Path) -> Result<()> {
    export::notify(cfg, &format!("Capture saved as {}", p.display()), Some(p));
    if cfg.copy_path_after_save {
        export::copy_text(&p.to_string_lossy())?;
    }
    Ok(())
}

/// Non-interactive captures (`full`, `screen`).
fn finish_noninteractive(img: RgbaImage, full_width: u32, out: &OutputArgs, pin: bool) -> Result<()> {
    let cfg = Config::load();
    let res = CaptureResult {
        region: [0, 0, img.width() as i32, img.height() as i32],
        image: img,
        action: ExitAction::Accept,
        logical_pos: (0.0, 0.0),
        scale: capture::scale_hint(full_width),
    };
    let explicit = out.path.is_some() || out.clipboard || out.raw || out.upload || pin;
    if !explicit {
        // Flameshot saves to the configured folder by default.
        let p = export::suggested_path(&cfg);
        export::write_image(&res.image, &p, &cfg)?;
        return after_save(&cfg, &p);
    }
    dispatch(&cfg, res, out, pin, false)
}

fn full(a: OutputArgs) -> Result<()> {
    let img = grab(a.delay)?;
    let w = img.width();
    finish_noninteractive(img, w, &a, false)
}

fn screen(a: ScreenArgs) -> Result<()> {
    let img = grab(a.out.delay)?;
    let full_width = img.width();
    let monitors = capture::monitors(img.width());
    let img = match monitors.get(a.number.unwrap_or(0)) {
        Some(m) => image::imageops::crop_imm(&img, m[0], m[1], m[2], m[3]).to_image(),
        None => {
            if let Some(n) = a.number {
                return Err(anyhow!("screen {n} not found"));
            }
            img
        }
    };
    finish_noninteractive(img, full_width, &a.out, a.pin)
}

fn parse_color(s: &str) -> Result<[u8; 4]> {
    let h = s.trim_start_matches('#');
    if h.len() != 6 {
        return Err(anyhow!("invalid color {s:?}, expected #RRGGBB"));
    }
    let v = u32::from_str_radix(h, 16)?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

fn config_cmd(a: ConfigArgs) -> Result<()> {
    let any = a.autostart.is_some()
        || a.filename.is_some()
        || a.trayicon.is_some()
        || a.showhelp.is_some()
        || a.maincolor.is_some()
        || a.contrastcolor.is_some()
        || a.check;
    if !any {
        return windows::config(a.shortcuts);
    }
    if a.check {
        let s = std::fs::read_to_string(config::config_path()).unwrap_or_default();
        match toml::from_str::<Config>(&s) {
            Ok(_) => println!("The configuration is OK ({})", config::config_path().display()),
            Err(e) => return Err(anyhow!("configuration error: {e}")),
        }
    }
    let mut cfg = Config::load();
    if let Some(v) = a.autostart {
        cfg.autostart = v;
        autostart::sync(v);
    }
    if let Some(v) = a.filename {
        cfg.filename_pattern = v;
    }
    if let Some(v) = a.trayicon {
        cfg.show_tray_icon = v;
    }
    if let Some(v) = a.showhelp {
        cfg.show_help = v;
    }
    if let Some(v) = a.maincolor {
        cfg.ui_color = parse_color(&v)?;
    }
    if let Some(v) = a.contrastcolor {
        cfg.contrast_color = parse_color(&v)?;
    }
    cfg.save()
}
