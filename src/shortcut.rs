//! Global hotkeys, configured like the "Screenshots" shortcuts on macOS:
//! every action gets its own key combination (`Config::hotkeys`).
//!
//! On GNOME they become custom keybindings (gsettings), so they work even when
//! the tray is not running. Inside Flatpak gsettings runs on the host through
//! `flatpak-spawn --host`. On other desktops the tray registers them through
//! the Global Shortcuts portal instead.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, bail};

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";

pub struct Action {
    /// Key in `Config::hotkeys`, also the portal shortcut id.
    pub id: &'static str,
    pub label: &'static str,
    pub args: &'static [&'static str],
    /// Last component of the GNOME custom keybinding path.
    gnome_name: &'static str,
}

pub const ACTIONS: &[Action] = &[
    Action { id: "capture", label: "Capture region", args: &["gui"], gnome_name: "fastshot" },
    Action {
        id: "full_clipboard",
        label: "Copy full screen to clipboard",
        args: &["full", "--clipboard"],
        gnome_name: "fastshot-full-clipboard",
    },
    Action { id: "full_save", label: "Save full screen to file", args: &["full"], gnome_name: "fastshot-full-save" },
    Action { id: "launcher", label: "Open launcher", args: &["launcher"], gnome_name: "fastshot-launcher" },
];

pub fn default_hotkeys() -> BTreeMap<String, String> {
    BTreeMap::from([("capture".to_owned(), "Print".to_owned())])
}

pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

// ------------------------------------------------------------ accels ----

const MODIFIERS: [(&str, &str); 4] =
    [("<Control>", "Ctrl"), ("<Alt>", "Alt"), ("<Shift>", "Shift"), ("<Super>", "Super")];

/// Split a GTK accelerator ("<Shift><Super>4") into modifiers and key name.
pub fn split(accel: &str) -> (Vec<&'static str>, String) {
    let mut mods = Vec::new();
    let mut rest = accel.trim();
    while let Some(end) = rest.strip_prefix('<').and_then(|r| r.find('>')) {
        let m = &rest[1..end + 1];
        let m = match m.to_ascii_lowercase().as_str() {
            "control" | "ctrl" | "primary" => "<Control>",
            "alt" | "mod1" => "<Alt>",
            "shift" => "<Shift>",
            "super" | "meta" | "mod4" => "<Super>",
            _ => "",
        };
        if !m.is_empty() && !mods.contains(&m) {
            mods.push(m);
        }
        rest = &rest[end + 2..];
    }
    (mods, rest.to_owned())
}

/// Canonical form: modifiers in a fixed order, then the key.
pub fn join(mods: &[&str], key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    let mut s: String = MODIFIERS.iter().filter(|(m, _)| mods.contains(m)).map(|(m, _)| *m).collect();
    s.push_str(key);
    s
}

pub fn normalize(accel: &str) -> String {
    let (mods, key) = split(accel);
    let key = if key.chars().count() == 1 { key.to_lowercase() } else { key };
    join(&mods, &key)
}

/// Human readable form, e.g. "Shift + Super + 4".
pub fn display(accel: &str) -> String {
    let (mods, key) = split(accel);
    if key.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = MODIFIERS
        .iter()
        .filter(|(m, _)| mods.contains(m))
        .map(|(_, n)| (*n).to_owned())
        .collect();
    parts.push(match key.as_str() {
        "Print" => "PrtSc".to_owned(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.replace('_', " "),
    });
    parts.join(" + ")
}

/// GTK key name of an egui key (layout independent, from the physical key).
pub fn gtk_key_name(key: egui::Key) -> Option<String> {
    use egui::Key as K;
    let s = match key {
        K::Space => "space",
        K::Enter => "Return",
        K::Tab => "Tab",
        K::Backspace => "BackSpace",
        K::Insert => "Insert",
        K::Delete => "Delete",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "Page_Up",
        K::PageDown => "Page_Down",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::ArrowUp => "Up",
        K::ArrowDown => "Down",
        K::Minus => "minus",
        K::Equals => "equal",
        K::Plus => "plus",
        K::Comma => "comma",
        K::Period => "period",
        K::Slash => "slash",
        K::Backslash => "backslash",
        K::Semicolon => "semicolon",
        K::Quote => "apostrophe",
        K::Backtick => "grave",
        K::OpenBracket => "bracketleft",
        K::CloseBracket => "bracketright",
        K::Escape => return None,
        k => {
            let n = k.name();
            return match n.len() {
                1 => Some(n.to_lowercase()),
                _ if n.starts_with('F') && n[1..].parse::<u8>().is_ok() => Some(n.to_owned()),
                _ => None,
            };
        }
    };
    Some(s.to_owned())
}

// ---------------------------------------------------------- gsettings ----

fn gsettings(args: &[&str]) -> Result<String> {
    let mut cmd = if crate::is_sandboxed() {
        let mut c = Command::new("flatpak-spawn");
        c.args(["--host", "gsettings"]);
        c
    } else {
        Command::new("gsettings")
    };
    let out = cmd.args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("gsettings {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Parse a GVariant string array as printed by gsettings ("@as []", "['a', 'b']").
fn parse_strv(v: &str) -> Vec<String> {
    v.trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().trim_matches('\'').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn format_strv(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", "))
}

/// Can hotkeys be registered as GNOME keybindings (GNOME, host gsettings reachable)?
pub fn gnome_available() -> bool {
    gsettings(&["get", SCHEMA, "custom-keybindings"]).is_ok()
}

fn quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:".contains(c)) {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

fn command_for(action: &Action) -> String {
    let mut parts: Vec<String> = if crate::is_sandboxed() {
        vec!["flatpak".into(), "run".into(), crate::APP_ID.into()]
    } else {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "fastshot".into());
        vec![exe]
    };
    parts.extend(action.args.iter().map(|s| (*s).to_owned()));
    parts.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ")
}

/// GNOME's own screenshot keys and their defaults. Ours take precedence: a key
/// we use is removed from them, and given back once we stop using it.
const GNOME_SCREENSHOT_KEYS: [(&str, &str, &str); 3] = [
    ("show-screenshot-ui", "Print", "<Shift><Super>Print"),
    ("screenshot", "<Shift>Print", ""),
    ("screenshot-window", "<Alt>Print", ""),
];

fn sync_gnome_screenshot_keys(used: &[String]) -> Result<()> {
    const SHELL: &str = "org.gnome.shell.keybindings";
    for (key, default, fallback) in GNOME_SCREENSHOT_KEYS {
        let Ok(cur) = gsettings(&["get", SHELL, key]) else { continue };
        let cur = parse_strv(&cur);
        let mut kept: Vec<String> =
            cur.iter().filter(|a| !used.contains(&normalize(a))).cloned().collect();
        if kept.len() != cur.len() {
            if kept.is_empty() && !fallback.is_empty() && !used.contains(&normalize(fallback)) {
                kept.push(fallback.to_owned());
            }
            gsettings(&["set", SHELL, key, &format_strv(&kept)])?;
        } else if !used.contains(&normalize(default)) {
            // Restore what we took away earlier (only our own replacement value).
            let ours = if fallback.is_empty() { vec![] } else { vec![fallback.to_owned()] };
            if cur == ours {
                gsettings(&["reset", SHELL, key])?;
            }
        }
    }
    Ok(())
}

/// Write the hotkeys as GNOME custom keybindings; empty ones are removed.
pub fn apply(hotkeys: &BTreeMap<String, String>) -> Result<()> {
    let mut list = parse_strv(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    let mut used = Vec::new();
    for action in ACTIONS {
        let path = format!("{BASE}{}/", action.gnome_name);
        let rel = format!("{SCHEMA}.custom-keybinding:{path}");
        let accel = hotkeys.get(action.id).map(|a| normalize(a)).unwrap_or_default();
        if accel.is_empty() {
            if list.contains(&path) {
                list.retain(|p| p != &path);
                for k in ["name", "command", "binding"] {
                    gsettings(&["reset", &rel, k])?;
                }
            }
            continue;
        }
        if !list.contains(&path) {
            list.push(path.clone());
        }
        gsettings(&["set", &rel, "name", &format!("Fastshot: {}", action.label)])?;
        gsettings(&["set", &rel, "command", &command_for(action)])?;
        gsettings(&["set", &rel, "binding", &accel])?;
        used.push(accel);
    }
    gsettings(&["set", SCHEMA, "custom-keybindings", &format_strv(&list)])?;
    sync_gnome_screenshot_keys(&used)
}

const CONFLICT_SCHEMAS: [&str; 5] = [
    "org.gnome.desktop.wm.keybindings",
    "org.gnome.shell.keybindings",
    "org.gnome.mutter.keybindings",
    "org.gnome.mutter.wayland.keybindings",
    "org.gnome.settings-daemon.plugins.media-keys",
];

/// Every accelerator bound in GNOME (normalized) → the setting that owns it.
/// Fastshot's own keybindings and the screenshot keys it takes over are skipped.
pub fn gnome_bindings() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for schema in CONFLICT_SCHEMAS {
        let Ok(out) = gsettings(&["list-recursively", schema]) else { continue };
        for line in out.lines() {
            let mut it = line.splitn(3, ' ');
            let (Some(_), Some(key), Some(value)) = (it.next(), it.next(), it.next()) else { continue };
            if GNOME_SCREENSHOT_KEYS.iter().any(|(k, ..)| *k == key) {
                continue;
            }
            let value = value.trim();
            let accels = if value.starts_with('[') || value.starts_with("@as") {
                parse_strv(value)
            } else {
                vec![value.trim_matches('\'').to_owned()]
            };
            for a in accels {
                let n = normalize(&a);
                if !split(&n).1.is_empty() {
                    map.entry(n).or_insert_with(|| key.replace('-', " "));
                }
            }
        }
    }
    // Other custom shortcuts the user made in GNOME Settings.
    if let Ok(list) = gsettings(&["get", SCHEMA, "custom-keybindings"]) {
        for path in parse_strv(&list) {
            if ACTIONS.iter().any(|a| path == format!("{BASE}{}/", a.gnome_name)) {
                continue;
            }
            let rel = format!("{SCHEMA}.custom-keybinding:{path}");
            if let (Ok(b), Ok(name)) = (gsettings(&["get", &rel, "binding"]), gsettings(&["get", &rel, "name"])) {
                let n = normalize(b.trim_matches('\''));
                if !n.is_empty() {
                    map.entry(n).or_insert_with(|| name.trim_matches('\'').to_owned());
                }
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_roundtrip() {
        assert_eq!(normalize("<Super><shift>4"), "<Shift><Super>4");
        assert_eq!(normalize("<Primary>A"), "<Control>a");
        assert_eq!(display("<Shift><Super>4"), "Shift + Super + 4");
        assert_eq!(display("Print"), "PrtSc");
        assert_eq!(split("<Control><Alt>Page_Up"), (vec!["<Control>", "<Alt>"], "Page_Up".to_owned()));
        assert_eq!(parse_strv("@as []"), Vec::<String>::new());
        assert_eq!(parse_strv("['Print', '<Super>p']"), vec!["Print", "<Super>p"]);
        assert_eq!(quote("/home/a b/fastshot"), "'/home/a b/fastshot'");
    }
}

