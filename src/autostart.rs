//! "Launch at startup" via an XDG autostart entry.

use std::path::PathBuf;

fn entry_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join("fastshot.desktop"))
}

pub fn sync(enabled: bool) {
    let Some(path) = entry_path() else { return };
    if enabled {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "fastshot".into());
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Fastshot\n\
             Comment=Screenshot tool (tray)\nExec={exe} tray\nIcon=camera-photo\n\
             Terminal=false\nX-GNOME-Autostart-enabled=true\n"
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, content);
    } else if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}
