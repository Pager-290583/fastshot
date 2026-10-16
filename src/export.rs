//! What happens to a finished capture: clipboard, file, pin, upload, open-with.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::capture::is_wayland;
use crate::config::{Config, cache_dir};

// ------------------------------------------------------------ encoding ----

pub fn encode(img: &RgbaImage, format: ImageFormat, jpeg_quality: u8) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, jpeg_quality);
            rgb.write_with_encoder(enc)?;
        }
        ImageFormat::Bmp => {
            image::DynamicImage::ImageRgba8(img.clone())
                .to_rgb8()
                .write_to(&mut out, ImageFormat::Bmp)?;
        }
        f => img.write_to(&mut out, f)?,
    }
    Ok(out.into_inner())
}

fn format_for(path: &Path) -> ImageFormat {
    ImageFormat::from_path(path).unwrap_or(ImageFormat::Png)
}

// ----------------------------------------------------------- clipboard ----

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|d| d.join(cmd).is_file())
    })
}

/// Put bytes of the given MIME type in the clipboard. The data has to be
/// served by a process that outlives us: `wl-copy` forks into the background,
/// on X11 we re-exec ourselves as a small clipboard server.
fn copy_bytes(bytes: &[u8], mime: &str) -> Result<()> {
    if is_wayland() && have("wl-copy") {
        let mut child = Command::new("wl-copy")
            .args(["--type", mime])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("running wl-copy")?;
        child.stdin.take().unwrap().write_all(bytes)?;
        let status = child.wait()?;
        if status.success() {
            return Ok(());
        }
        eprintln!("fastshot: wl-copy failed, falling back to X11 clipboard");
    }
    let exe = std::env::current_exe()?;
    let mut child = Command::new(exe)
        .args(["clipboard-serve", "--mime", mime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("spawning clipboard server")?;
    child.stdin.take().unwrap().write_all(bytes)?;
    Ok(())
}

pub fn copy_image(img: &RgbaImage, cfg: &Config) -> Result<()> {
    if cfg.use_jpg_for_clipboard {
        copy_bytes(&encode(img, ImageFormat::Jpeg, cfg.jpeg_quality)?, "image/jpeg")
    } else {
        copy_bytes(&encode(img, ImageFormat::Png, 0)?, "image/png")
    }
}

pub fn copy_text(text: &str) -> Result<()> {
    copy_bytes(text.as_bytes(), "text/plain;charset=utf-8")
}

/// `fastshot clipboard-serve`: keep clipboard ownership (X11 / XWayland)
/// until another application takes it over.
pub fn clipboard_serve(mime: &str) -> Result<()> {
    use arboard::SetExtLinux;
    let mut data = Vec::new();
    std::io::stdin().read_to_end(&mut data)?;
    let mut cb = arboard::Clipboard::new()?;
    if mime.starts_with("image/") {
        let img = image::load_from_memory(&data)?.to_rgba8();
        let (w, h) = img.dimensions();
        cb.set().wait().image(arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: img.into_raw().into(),
        })?;
    } else {
        cb.set().wait().text(String::from_utf8_lossy(&data).into_owned())?;
    }
    Ok(())
}

// --------------------------------------------------------------- files ----

/// Expand the strftime-like filename pattern (`%F_%H-%M` by default).
pub fn format_filename(pattern: &str) -> String {
    use std::fmt::Write as _;
    let now = chrono::Local::now();
    let mut s = String::new();
    if write!(s, "{}", now.format(pattern)).is_err() || s.trim().is_empty() {
        s = now.format("%F_%H-%M").to_string();
    }
    s.replace('/', "_")
}

pub fn default_extension(cfg: &Config) -> &str {
    match cfg.save_as_file_extension.as_str() {
        "" => "png",
        e => e.trim_start_matches('.'),
    }
}

fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut i = 1;
    while p.exists() {
        p = dir.join(format!("{stem}_{i}.{ext}"));
        i += 1;
    }
    p
}

/// Proposed file name in the configured directory.
pub fn suggested_path(cfg: &Config) -> PathBuf {
    let dir = PathBuf::from(expand_home(&cfg.save_path));
    unique_path(&dir, &format_filename(&cfg.filename_pattern), default_extension(cfg))
}

pub fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().into_owned();
    }
    p.to_owned()
}

pub fn write_image(img: &RgbaImage, path: &Path, cfg: &Config) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let bytes = encode(img, format_for(path), cfg.jpeg_quality)?;
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// Save to `target`: a directory gets an auto-generated name, a file path is used as is.
pub fn save_to(img: &RgbaImage, target: &Path, cfg: &Config) -> Result<PathBuf> {
    let target = PathBuf::from(expand_home(&target.to_string_lossy()));
    let path = if target.is_dir() || target.to_string_lossy().ends_with('/') {
        std::fs::create_dir_all(&target).ok();
        unique_path(&target, &format_filename(&cfg.filename_pattern), default_extension(cfg))
    } else if target.extension().is_none() {
        target.with_extension(default_extension(cfg))
    } else {
        target
    };
    write_image(img, &path, cfg)?;
    Ok(path)
}

/// Ask for a file name with the portal file chooser.
pub fn save_dialog(img: &RgbaImage, cfg: &Config) -> Result<Option<PathBuf>> {
    let suggested = suggested_path(cfg);
    let dir = suggested.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = suggested
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let picked = rfd::FileDialog::new()
        .set_title("Save screenshot")
        .set_directory(&dir)
        .set_file_name(name)
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .add_filter("WebP", &["webp"])
        .add_filter("BMP", &["bmp"])
        .save_file();
    match picked {
        Some(p) => Ok(Some(save_to(img, &p, cfg)?)),
        None => Ok(None),
    }
}

/// Save following the configuration (fixed path or dialog).
pub fn save_interactive(img: &RgbaImage, cfg: &Config) -> Result<Option<PathBuf>> {
    if cfg.save_path_fixed {
        let p = suggested_path(cfg);
        write_image(img, &p, cfg)?;
        Ok(Some(p))
    } else {
        save_dialog(img, cfg)
    }
}

pub fn write_raw_stdout(img: &RgbaImage) -> Result<()> {
    let bytes = encode(img, ImageFormat::Png, 0)?;
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)?;
    out.flush()?;
    Ok(())
}

// ------------------------------------------------------- notifications ----

pub fn notify(cfg: &Config, body: &str, image: Option<&Path>) {
    if !cfg.show_desktop_notification {
        return;
    }
    let mut n = notify_rust::Notification::new();
    n.appname("Fastshot")
        .summary("Fastshot Info")
        .body(body)
        .icon("camera-photo")
        .timeout(notify_rust::Timeout::Milliseconds(4000));
    if let Some(p) = image {
        n.image_path(&p.to_string_lossy());
    }
    if let Err(e) = n.show() {
        eprintln!("fastshot: notification failed: {e}");
    }
}

pub fn notify_error(cfg: &Config, body: &str) {
    eprintln!("fastshot: {body}");
    notify(cfg, body, None);
}

// ---------------------------------------------------------------- misc ----

fn temp_png(img: &RgbaImage, sub: &str) -> Result<PathBuf> {
    let dir = cache_dir().join(sub);
    std::fs::create_dir_all(&dir)?;
    let name = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let path = dir.join(format!("fastshot-{name}.png"));
    std::fs::write(&path, encode(img, ImageFormat::Png, 0)?)?;
    Ok(path)
}

/// Pin the capture on the desktop in its own process.
/// `pos` is the logical position of the selection, used where the windowing
/// system allows placing windows (X11).
pub fn spawn_pin(img: &RgbaImage, pos: Option<(f32, f32)>, scale: f32) -> Result<()> {
    let path = temp_png(img, "pins")?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("pin").arg(&path).arg("--delete");
    cmd.args(["--scale", &scale.to_string()]);
    if let Some((x, y)) = pos {
        cmd.args(["--x", &x.to_string(), "--y", &y.to_string()]);
    }
    cmd.stdin(Stdio::null()).spawn().context("spawning pin window")?;
    Ok(())
}

/// Let the user choose an application (portal "Open with" dialog).
pub fn open_with(img: &RgbaImage) -> Result<()> {
    let path = temp_png(img, "open")?;
    let file = std::fs::File::open(&path)?;
    let res = pollster::block_on(async {
        ashpd::desktop::open_uri::OpenFileRequest::default()
            .ask(true)
            .send_file(&file)
            .await
    });
    if let Err(e) = res {
        eprintln!("fastshot: open-with portal failed ({e}), using xdg-open");
        Command::new("xdg-open").arg(&path).spawn()?;
    }
    Ok(())
}

pub fn open_path(path: &Path) {
    let _ = Command::new("xdg-open")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

// -------------------------------------------------------------- upload ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: i64,
    pub url: String,
    pub delete_url: String,
    pub thumbnail: PathBuf,
}

fn history_path() -> PathBuf {
    cache_dir().join("history.json")
}

pub fn load_history() -> Vec<HistoryEntry> {
    std::fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_history(h: &[HistoryEntry]) {
    if let Ok(s) = serde_json::to_string_pretty(h) {
        let _ = std::fs::write(history_path(), s);
    }
}

fn add_history(cfg: &Config, img: &RgbaImage, url: &str, delete_url: &str) {
    let dir = cache_dir().join("history");
    let _ = std::fs::create_dir_all(&dir);
    let ts = chrono::Local::now().timestamp();
    let thumb = dir.join(format!("{ts}.png"));
    let small = image::imageops::thumbnail(img, 256.min(img.width()), {
        let h = img.height() as u64 * 256.min(img.width()) as u64 / img.width().max(1) as u64;
        h.max(1) as u32
    });
    let _ = small.save(&thumb);
    let mut h = load_history();
    h.insert(
        0,
        HistoryEntry {
            timestamp: ts,
            url: url.into(),
            delete_url: delete_url.into(),
            thumbnail: thumb,
        },
    );
    while h.len() > cfg.upload_history_max.max(1) {
        if let Some(old) = h.pop() {
            let _ = std::fs::remove_file(old.thumbnail);
        }
    }
    save_history(&h);
}

/// Anonymous upload to Imgur. Returns the public URL.
pub fn upload_imgur(img: &RgbaImage, cfg: &Config) -> Result<String> {
    use base64::Engine as _;
    if cfg.imgur_client_id.trim().is_empty() {
        bail!(
            "Imgur Client ID is not set. Register an application at \
             https://api.imgur.com/oauth2/addclient and put its Client ID into \
             the Fastshot configuration (General → Imgur Client ID)."
        );
    }
    let png = encode(img, ImageFormat::Png, 0)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let mut resp = ureq::post("https://api.imgur.com/3/image")
        .header("Authorization", &format!("Client-ID {}", cfg.imgur_client_id.trim()))
        .send_form([("image", b64.as_str()), ("type", "base64")])
        .map_err(|e| anyhow!("upload failed: {e}"))?;
    let json: serde_json::Value = resp.body_mut().read_json()?;
    let data = &json["data"];
    let url = data["link"]
        .as_str()
        .ok_or_else(|| anyhow!("unexpected Imgur response: {json}"))?
        .to_owned();
    let delete_url = data["deletehash"]
        .as_str()
        .map(|h| format!("https://imgur.com/delete/{h}"))
        .unwrap_or_default();
    add_history(cfg, img, &url, &delete_url);
    Ok(url)
}
