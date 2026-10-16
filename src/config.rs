//! Persistent settings (`~/.config/fastshot/fastshot.toml`).
//!
//! Mirrors the options of Flameshot's `flameshot.ini`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::tools::Button;

pub type Rgba = [u8; 4];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // ---- Interface ----
    /// Main UI color (buttons, selection border).
    pub ui_color: Rgba,
    /// Contrast UI color (active button, handles).
    pub contrast_color: Rgba,
    /// Opacity (0-255) of the dimmed area outside the selection.
    pub contrast_opacity: u8,
    /// Buttons shown around the selection, in order.
    pub buttons: Vec<Button>,
    pub button_size: f32,
    pub show_help: bool,
    pub show_sidepanel_button: bool,
    pub show_selection_geometry: bool,
    pub show_magnifier: bool,
    pub square_magnifier: bool,
    /// Palette shown by the right-click color wheel.
    pub user_colors: Vec<Rgba>,

    // ---- Drawing ----
    pub draw_color: Rgba,
    /// Per tool size (thickness / font size / pixel size).
    pub tool_sizes: BTreeMap<String, f32>,
    pub font_family: String,
    pub text_bold: bool,
    pub text_italic: bool,
    pub text_underline: bool,
    pub text_strikeout: bool,
    pub text_align: TextAlign,
    pub undo_limit: usize,

    // ---- General ----
    pub show_desktop_notification: bool,
    pub show_tray_icon: bool,
    pub autostart: bool,
    pub copy_on_double_click: bool,
    pub save_after_copy: bool,
    pub copy_path_after_save: bool,
    pub use_jpg_for_clipboard: bool,
    pub jpeg_quality: u8,
    pub save_last_region: bool,
    pub antialiasing_pin_zoom: bool,
    pub upload_without_confirmation: bool,
    pub copy_url_after_upload: bool,
    pub upload_history_max: usize,
    pub imgur_client_id: String,

    // ---- Filename / saving ----
    pub save_path: String,
    pub save_path_fixed: bool,
    pub filename_pattern: String,
    pub save_as_file_extension: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl Default for Config {
    fn default() -> Self {
        let pictures = dirs::picture_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            ui_color: [0x74, 0x00, 0x96, 0xff],
            contrast_color: [0x27, 0x00, 0x32, 0xff],
            contrast_opacity: 190,
            buttons: Button::default_set(),
            button_size: 32.0,
            show_help: true,
            show_sidepanel_button: true,
            show_selection_geometry: true,
            show_magnifier: false,
            square_magnifier: false,
            user_colors: vec![
                [0x80, 0x00, 0x00, 0xff],
                [0xff, 0x00, 0x00, 0xff],
                [0xff, 0xff, 0x00, 0xff],
                [0x00, 0xff, 0x00, 0xff],
                [0x00, 0x80, 0x00, 0xff],
                [0x00, 0xff, 0xff, 0xff],
                [0x00, 0x00, 0xff, 0xff],
                [0xff, 0x00, 0xff, 0xff],
                [0x80, 0x00, 0x80, 0xff],
                [0xff, 0xff, 0xff, 0xff],
                [0x00, 0x00, 0x00, 0xff],
            ],
            draw_color: [0xff, 0x00, 0x00, 0xff],
            tool_sizes: BTreeMap::new(),
            font_family: "sans-serif".into(),
            text_bold: false,
            text_italic: false,
            text_underline: false,
            text_strikeout: false,
            text_align: TextAlign::Left,
            undo_limit: 100,
            show_desktop_notification: true,
            show_tray_icon: true,
            autostart: false,
            copy_on_double_click: false,
            save_after_copy: false,
            copy_path_after_save: false,
            use_jpg_for_clipboard: false,
            jpeg_quality: 90,
            save_last_region: true,
            antialiasing_pin_zoom: true,
            upload_without_confirmation: false,
            copy_url_after_upload: true,
            upload_history_max: 25,
            imgur_client_id: String::new(),
            save_path: pictures.to_string_lossy().into_owned(),
            save_path_fixed: false,
            filename_pattern: "%F_%H-%M".into(),
            save_as_file_extension: "png".into(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fastshot")
}

pub fn cache_dir() -> PathBuf {
    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("fastshot");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn config_path() -> PathBuf {
    config_dir().join("fastshot.toml")
}

impl Config {
    pub fn load() -> Self {
        match std::fs::read_to_string(config_path()) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                eprintln!("fastshot: invalid config, using defaults: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(config_dir())?;
        std::fs::write(config_path(), toml::to_string_pretty(self)?)?;
        crate::autostart::sync(self.autostart);
        Ok(())
    }

    pub fn tool_size(&self, key: &str, default: f32) -> f32 {
        self.tool_sizes.get(key).copied().unwrap_or(default)
    }
}

/// State that is not a user preference (last selected region).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub last_region: Option<[i32; 4]>,
}

impl State {
    fn path() -> PathBuf {
        cache_dir().join("state.toml")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(s) = toml::to_string(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }
}
