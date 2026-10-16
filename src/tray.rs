//! System tray icon (StatusNotifierItem). On GNOME it needs the
//! "AppIndicator and KStatusNotifierItem Support" extension.

use std::process::Command;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{MenuItem, Tray};

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
        vec![crate::icon::tray_icon(32)]
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

pub fn run() -> anyhow::Result<()> {
    let handle = FastshotTray
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot create tray icon: {e}"))?;
    // Keep running until "Quit".
    loop {
        std::thread::park();
        if handle.is_closed() {
            break;
        }
    }
    Ok(())
}
