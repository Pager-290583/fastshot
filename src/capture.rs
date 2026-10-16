//! Screen grabbing backends.
//!
//! * Wayland: `org.freedesktop.portal.Screenshot` (works on GNOME, KDE, wlroots with xdpw).
//! * X11: direct `GetImage` on the root window.

use anyhow::{Context, Result, anyhow, bail};
use image::RgbaImage;

pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").map(|s| s == "wayland").unwrap_or(false)
}

/// Grab the whole desktop (all monitors) as one image in physical pixels.
pub fn grab_desktop() -> Result<RgbaImage> {
    if is_wayland() {
        match grab_portal().or_else(|e| retry_after_access(e)) {
            Ok(img) => return Ok(img),
            Err(e) => {
                if std::env::var_os("DISPLAY").is_none() {
                    return Err(e);
                }
                eprintln!("fastshot: portal capture failed ({e:#}), trying X11");
            }
        }
    }
    grab_x11()
}

/// GNOME only shows the "allow screenshots" dialog to the focused app. A tray
/// or command line capture has no window, so on the first denial we open a
/// small window that asks for access; GNOME remembers the answer.
fn retry_after_access(e: anyhow::Error) -> Result<RgbaImage> {
    if !crate::is_sandboxed() {
        return Err(e);
    }
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("grant-access")
        .status()?;
    if !status.success() {
        return Err(e.context("screenshot permission was not granted"));
    }
    // Let the permission window disappear before grabbing the screen.
    std::thread::sleep(std::time::Duration::from_millis(400));
    grab_portal()
}

/// Ask the portal for a screenshot and throw it away (used to obtain the permission).
pub fn probe_portal() -> Result<()> {
    grab_portal().map(|_| ())
}

fn grab_portal() -> Result<RgbaImage> {
    let uri = pollster::block_on(async {
        let request = ashpd::desktop::screenshot::Screenshot::request()
            .interactive(false)
            .modal(false)
            .send()
            .await?;
        let shot = request.response()?;
        Ok::<_, ashpd::Error>(shot.uri().as_str().to_owned())
    })
    .map_err(|e| anyhow!("screenshot portal: {e}"))?;

    let path = url::Url::parse(&uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .ok_or_else(|| anyhow!("portal returned unexpected uri {uri}"))?;
    let img = image::open(&path)
        .with_context(|| format!("reading {}", path.display()))?
        .to_rgba8();
    // The portal stores the capture in the user's Pictures folder; it is only a
    // temporary transport for us, exactly like Flameshot removes it.
    remove_portal_file(&path);
    Ok(img)
}

/// Inside Flatpak the portal hands out a document-portal path
/// (`/run/user/UID/doc/ID/Screenshot.png`); the real file lives in the
/// Pictures folder (to which the manifest grants access).
fn remove_portal_file(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    if !path.to_string_lossy().contains("/doc/") {
        return;
    }
    let (Some(name), Some(pictures)) = (path.file_name(), dirs::picture_dir()) else {
        return;
    };
    let mut candidates = vec![pictures.join(name)];
    if let Ok(dirs) = std::fs::read_dir(&pictures) {
        candidates.extend(dirs.flatten().map(|d| d.path().join(name)));
    }
    for c in candidates {
        let fresh = std::fs::metadata(&c)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|e| e.as_secs() < 30));
        if fresh {
            let _ = std::fs::remove_file(c);
            break;
        }
    }
}

fn grab_x11() -> Result<RgbaImage> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

    let (conn, screen_num) = x11rb::connect(None).context("connecting to X server")?;
    let screen = &conn.setup().roots[screen_num];
    let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, screen.root, 0, 0, w, h, !0)?
        .reply()
        .context("XGetImage")?;
    if reply.depth != 24 && reply.depth != 32 {
        bail!("unsupported X11 depth {}", reply.depth);
    }
    let data = reply.data;
    let mut img = RgbaImage::new(w as u32, h as u32);
    for (dst, src) in img.pixels_mut().zip(data.chunks_exact(4)) {
        *dst = image::Rgba([src[2], src[1], src[0], 255]);
    }
    Ok(img)
}

/// Monitor rectangles `[x, y, w, h]` in image pixels for an image of the whole
/// desktop that is `image_width` pixels wide. Primary monitor first.
/// Uses RandR (also available through XWayland).
pub fn monitors(image_width: u32) -> Vec<[u32; 4]> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt;
    let Ok((conn, n)) = x11rb::connect(None) else { return Vec::new() };
    let screen = &conn.setup().roots[n];
    let k = image_width as f32 / screen.width_in_pixels.max(1) as f32;
    let Ok(Ok(reply)) = conn.randr_get_monitors(screen.root, true).map(|c| c.reply()) else {
        return Vec::new();
    };
    let mut mons: Vec<_> = reply.monitors;
    mons.sort_by_key(|m| !m.primary);
    mons.iter()
        .map(|m| {
            [
                (m.x.max(0) as f32 * k) as u32,
                (m.y.max(0) as f32 * k) as u32,
                (m.width as f32 * k) as u32,
                (m.height as f32 * k) as u32,
            ]
        })
        .collect()
}

/// Ratio between captured pixels and logical desktop pixels, if known.
pub fn scale_hint(image_width: u32) -> f32 {
    use x11rb::connection::Connection;
    match x11rb::connect(None) {
        Ok((conn, n)) => {
            let w = conn.setup().roots[n].width_in_pixels.max(1) as f32;
            (image_width as f32 / w).max(1.0)
        }
        Err(_) => 1.0,
    }
}
