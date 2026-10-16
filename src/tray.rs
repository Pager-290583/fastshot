//! System tray daemon (StatusNotifierItem). On GNOME it needs the
//! "AppIndicator and KStatusNotifierItem Support" extension.
//!
//! Only one tray runs at a time: launching Fastshot from the application
//! menu while it is already in the tray starts a capture instead.

use std::collections::BTreeMap;
use std::fs::File;
use std::process::Command;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{MenuItem, Tray};

use crate::config::{Config, cache_dir};
use crate::{export, shortcut};

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
            item("Keyboard _Shortcuts…", "preferences-desktop-keyboard-shortcuts", &["config", "--shortcuts"]),
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

/// Portal trigger syntax ("LOGO+SHIFT+4") of a GTK accelerator.
fn portal_trigger(accel: &str) -> String {
    let (mods, key) = shortcut::split(accel);
    let mut parts: Vec<&str> = mods
        .iter()
        .map(|m| match *m {
            "<Control>" => "CTRL",
            "<Alt>" => "ALT",
            "<Shift>" => "SHIFT",
            _ => "LOGO",
        })
        .collect();
    parts.push(&key);
    parts.join("+")
}

/// Hotkeys through the `GlobalShortcuts` portal, for desktops other than GNOME.
/// The desktop asks the user to confirm the keys the first time.
async fn portal_hotkeys(hotkeys: BTreeMap<String, String>) -> ashpd::Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let triggers: Vec<(&'static str, &'static str, String)> = shortcut::ACTIONS
        .iter()
        .filter_map(|a| {
            let accel = hotkeys.get(a.id).filter(|k| !k.is_empty())?;
            Some((a.id, a.label, portal_trigger(accel)))
        })
        .collect();
    if triggers.is_empty() {
        return Ok(());
    }
    let shortcuts: Vec<NewShortcut> = triggers
        .iter()
        .map(|(id, label, t)| NewShortcut::new(*id, *label).preferred_trigger(t.as_str()))
        .collect();
    let gs = GlobalShortcuts::new().await?;
    let session = gs.create_session(Default::default()).await?;
    gs.bind_shortcuts(&session, &shortcuts, None, Default::default())
        .await?
        .response()?;
    let mut activated = gs.receive_activated().await?;
    while let Some(a) = activated.next().await {
        if let Some(action) = shortcut::action(a.shortcut_id()) {
            spawn(action.args);
        }
    }
    Ok(())
}

/// GNOME keybindings when possible (they work without the tray), else the portal.
fn register_hotkeys(hotkeys: BTreeMap<String, String>) {
    if shortcut::gnome_available() {
        if let Err(e) = shortcut::apply(&hotkeys) {
            eprintln!("fastshot: cannot register shortcuts: {e:#}");
        }
    } else if let Err(e) = pollster::block_on(portal_hotkeys(hotkeys)) {
        eprintln!("fastshot: global shortcuts unavailable: {e}");
    }
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

    let cfg = Config::load();
    let hotkeys = cfg.hotkeys.clone();
    std::thread::spawn(move || register_hotkeys(hotkeys));

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
