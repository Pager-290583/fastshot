//! System tray daemon (StatusNotifierItem). On GNOME it needs the
//! "AppIndicator and KStatusNotifierItem Support" extension.
//!
//! Only one tray runs at a time: launching Fastshot from the application
//! menu while it is already in the tray starts a capture instead.

use std::fs::File;
use std::process::Command;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{MenuItem, Tray};

use crate::config::{Config, cache_dir};
use crate::export;

fn spawn(args: &[&str]) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).args(args).spawn();
    }
}

struct FastshotTray;

impl Tray for FastshotTray {
    fn id(&self) -> String {
        "fastshot".into()
    }

    fn title(&self) -> String {
        "Fastshot".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![crate::icon::tray_icon(32), crate::icon::tray_icon(64)]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Fastshot".into(),
            description: "Left click: take screenshot".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        spawn(&["gui"]);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: &str, icon: &str, args: &'static [&'static str]| -> MenuItem<Self> {
            StandardItem {
                label: label.into(),
                icon_name: icon.into(),
                activate: Box::new(move |_| spawn(args)),
                ..Default::default()
            }
            .into()
        };
        vec![
            item("_Take Screenshot", "camera-photo", &["gui"]),
            item("_Open Launcher", "system-run", &["launcher"]),
            item("Full Screen to Clipboard", "video-display", &["full", "--clipboard"]),
            MenuItem::Separator,
            item("_Configuration", "preferences-system", &["config"]),
            item("Latest _Uploads", "document-open-recent", &["history"]),
            item("_About", "help-about", &["about"]),
            MenuItem::Separator,
            StandardItem {
                label: "_Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Global hotkey through the `GlobalShortcuts` portal (GNOME 48+, KDE).
/// The desktop asks the user to confirm the key the first time.
async fn global_shortcut() -> ashpd::Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let gs = GlobalShortcuts::new().await?;
    let session = gs.create_session(Default::default()).await?;
    let shortcuts = [NewShortcut::new("capture", "Take a screenshot").preferred_trigger("Print")];
    gs.bind_shortcuts(&session, &shortcuts, None, Default::default())
        .await?
        .response()?;
    let mut activated = gs.receive_activated().await?;
    while let Some(a) = activated.next().await {
        if a.shortcut_id() == "capture" {
            spawn(&["gui"]);
        }
    }
    Ok(())
}

pub fn run() -> anyhow::Result<()> {
    // Single instance: hold an exclusive lock for the lifetime of the tray.
    let lock = File::create(cache_dir().join("tray.lock"))?;
    if lock.try_lock().is_err() {
        spawn(&["gui"]);
        return Ok(());
    }

    let tray = FastshotTray;
    // A sandbox may not own the `org.kde.StatusNotifierItem-*` name.
    let handle = tray
        .disable_dbus_name(crate::is_sandboxed())
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot create tray icon: {e}"))?;

    // Outside Flatpak the hotkey is a regular GNOME keybinding (install-shortcut).
    if crate::is_sandboxed() {
        std::thread::spawn(|| {
            if let Err(e) = pollster::block_on(global_shortcut()) {
                eprintln!("fastshot: global shortcut unavailable: {e}");
            }
        });
    }

    let cfg = Config::load();
    export::notify(
        &cfg,
        "Hello, I'm here! Click icon in the tray to take a screenshot or right click to see more options.",
        None,
    );

    loop {
        std::thread::park();
        if handle.is_closed() {
            break;
        }
    }
    drop(lock);
    Ok(())
}
