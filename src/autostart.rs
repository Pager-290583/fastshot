//! "Launch at startup": an XDG autostart entry, or the Background portal
//! when running inside Flatpak (the sandbox cannot write ~/.config/autostart).

use std::path::PathBuf;

fn entry_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join("fastshot.desktop"))
}

fn sync_portal(enabled: bool) {
    let res = pollster::block_on(async {
        ashpd::desktop::background::Background::request()
            .reason("Keep Fastshot in the tray to take screenshots")
            .auto_start(enabled)
            .command(["fastshot", "tray"])
            .dbus_activatable(false)
            .send()
            .await?
            .response()
    });
    if let Err(e) = res {
        eprintln!("fastshot: background portal: {e}");
    }
}

pub fn sync(enabled: bool) {
    if crate::is_sandboxed() {
        sync_portal(enabled);
        return;
    }
    let Some(path) = entry_path() else { return };
    if enabled {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "fastshot".into());
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Fastshot\n\
             Comment=Screenshot tool (tray)\nExec={exe} tray\nIcon={}\n\
             Terminal=false\nX-GNOME-Autostart-enabled=true\n",
            crate::APP_ID
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, content);
    } else if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}
