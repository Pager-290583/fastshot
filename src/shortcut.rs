//! Global hotkey registration through GNOME custom keybindings (gsettings).

use std::process::Command;

use anyhow::{Context, Result, bail};

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/fastshot/";

fn gsettings(args: &[&str]) -> Result<String> {
    let out = Command::new("gsettings").args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("gsettings {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Bind `accel` (e.g. "Print") to `fastshot gui`.
pub fn install_gnome_binding(accel: &str) -> Result<()> {
    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let current = gsettings(&["get", SCHEMA, "custom-keybindings"])?;
    // Value looks like "@as []" or "['/org/.../custom0/', ...]".
    let mut paths: Vec<String> = current
        .trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().trim_matches('\'').to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    if !paths.iter().any(|p| p == PATH) {
        paths.push(PATH.to_owned());
    }
    let list = format!(
        "[{}]",
        paths.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", ")
    );
    gsettings(&["set", SCHEMA, "custom-keybindings", &list])?;
    let rel = format!("{SCHEMA}.custom-keybinding:{PATH}");
    gsettings(&["set", &rel, "name", "Fastshot"])?;
    gsettings(&["set", &rel, "command", &format!("{exe} gui")])?;
    gsettings(&["set", &rel, "binding", accel])?;

    // GNOME's own screenshot UI owns Print by default; hand the key over.
    if let Ok(v) = gsettings(&["get", "org.gnome.shell.keybindings", "show-screenshot-ui"])
        && v.contains(&format!("'{accel}'"))
    {
        gsettings(&["set", "org.gnome.shell.keybindings", "show-screenshot-ui", "['<Shift><Super>Print']"])?;
    }
    Ok(())
}
