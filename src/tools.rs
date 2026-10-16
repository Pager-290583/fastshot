//! Tool / button catalogue (equivalent of Flameshot's `CaptureTool::Type`).

use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

/// Drawing tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Pencil,
    Line,
    Arrow,
    Selection,
    Rectangle,
    Circle,
    Marker,
    Text,
    Pixelate,
    CircleCount,
    Invert,
}

impl Tool {
    pub fn key(self) -> &'static str {
        match self {
            Tool::Pencil => "pencil",
            Tool::Line => "line",
            Tool::Arrow => "arrow",
            Tool::Selection => "selection",
            Tool::Rectangle => "rectangle",
            Tool::Circle => "circle",
            Tool::Marker => "marker",
            Tool::Text => "text",
            Tool::Pixelate => "pixelate",
            Tool::CircleCount => "circlecount",
            Tool::Invert => "invert",
        }
    }

    /// Default "thickness" of the tool. For text it is the font size, for the
    /// pixelate tool the size of a pixel block, for the filled rectangle the
    /// corner radius.
    pub fn default_size(self) -> f32 {
        match self {
            Tool::Marker => 14.0,
            Tool::Text => 22.0,
            Tool::Pixelate => 10.0,
            Tool::Rectangle => 0.0,
            Tool::CircleCount => 4.0,
            _ => 3.0,
        }
    }

    pub fn min_size(self) -> f32 {
        match self {
            Tool::Rectangle => 0.0,
            Tool::Text => 6.0,
            Tool::Pixelate => 2.0,
            _ => 1.0,
        }
    }

    pub fn max_size(self) -> f32 {
        match self {
            Tool::Text => 200.0,
            _ => 100.0,
        }
    }
}

/// Everything that can appear as a button around the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Button {
    Tool(Tool),
    SelectionIndicator,
    Move,
    Undo,
    Redo,
    Copy,
    Save,
    Exit,
    ImageUploader,
    OpenApp,
    Pin,
    SizeIncrease,
    SizeDecrease,
    Accept,
}

impl Button {
    pub fn all() -> Vec<Button> {
        use Tool::*;
        vec![
            Button::Tool(Pencil),
            Button::Tool(Line),
            Button::Tool(Arrow),
            Button::Tool(Selection),
            Button::Tool(Rectangle),
            Button::Tool(Circle),
            Button::Tool(Marker),
            Button::Tool(Text),
            Button::Tool(Pixelate),
            Button::Tool(CircleCount),
            Button::Tool(Invert),
            Button::SelectionIndicator,
            Button::Move,
            Button::Undo,
            Button::Redo,
            Button::Copy,
            Button::Save,
            Button::Exit,
            Button::ImageUploader,
            Button::OpenApp,
            Button::Pin,
            Button::SizeIncrease,
            Button::SizeDecrease,
            Button::Accept,
        ]
    }

    pub fn default_set() -> Vec<Button> {
        Self::all()
            .into_iter()
            .filter(|b| !matches!(b, Button::Tool(Tool::Invert) | Button::Accept))
            .collect()
    }

    pub fn icon(self) -> &'static str {
        match self {
            Button::Tool(t) => match t {
                Tool::Pencil => icon::PENCIL_SIMPLE,
                Tool::Line => icon::LINE_SEGMENT,
                Tool::Arrow => icon::ARROW_UP_RIGHT,
                Tool::Selection => icon::SQUARE,
                Tool::Rectangle => icon::RECTANGLE,
                Tool::Circle => icon::CIRCLE,
                Tool::Marker => icon::HIGHLIGHTER,
                Tool::Text => icon::TEXT_T,
                Tool::Pixelate => icon::CHECKERBOARD,
                Tool::CircleCount => icon::NUMBER_CIRCLE_ONE,
                Tool::Invert => icon::CIRCLE_HALF,
            },
            Button::SelectionIndicator => icon::FRAME_CORNERS,
            Button::Move => icon::ARROWS_OUT_CARDINAL,
            Button::Undo => icon::ARROW_COUNTER_CLOCKWISE,
            Button::Redo => icon::ARROW_CLOCKWISE,
            Button::Copy => icon::COPY,
            Button::Save => icon::FLOPPY_DISK,
            Button::Exit => icon::X,
            Button::ImageUploader => icon::UPLOAD,
            Button::OpenApp => icon::APP_WINDOW,
            Button::Pin => icon::PUSH_PIN,
            Button::SizeIncrease => icon::PLUS,
            Button::SizeDecrease => icon::MINUS,
            Button::Accept => icon::CHECK,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Button::Tool(t) => match t {
                Tool::Pencil => "Pencil",
                Tool::Line => "Line",
                Tool::Arrow => "Arrow",
                Tool::Selection => "Rectangular Selection",
                Tool::Rectangle => "Rectangle",
                Tool::Circle => "Circle",
                Tool::Marker => "Marker",
                Tool::Text => "Text",
                Tool::Pixelate => "Pixelate",
                Tool::CircleCount => "Circle Counter",
                Tool::Invert => "Invert",
            },
            Button::SelectionIndicator => "Selection Size Indicator",
            Button::Move => "Move",
            Button::Undo => "Undo",
            Button::Redo => "Redo",
            Button::Copy => "Copy",
            Button::Save => "Save",
            Button::Exit => "Exit",
            Button::ImageUploader => "Image Uploader",
            Button::OpenApp => "App Launcher",
            Button::Pin => "Pin Tool",
            Button::SizeIncrease => "Increase Tool Size",
            Button::SizeDecrease => "Decrease Tool Size",
            Button::Accept => "Accept",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Button::Tool(t) => match t {
                Tool::Pencil => "Set the Pencil as the paint tool",
                Tool::Line => "Set the Line as the paint tool",
                Tool::Arrow => "Set the Arrow as the paint tool",
                Tool::Selection => "Set Selection as the paint tool",
                Tool::Rectangle => "Set the Rectangle as the paint tool",
                Tool::Circle => "Set the Circle as the paint tool",
                Tool::Marker => "Set the Marker as the paint tool",
                Tool::Text => "Add text to your capture",
                Tool::Pixelate => "Set Pixelate as the paint tool",
                Tool::CircleCount => "Add an autoincrementing counter bubble",
                Tool::Invert => "Set Inverter as the paint tool",
            },
            Button::SelectionIndicator => "Show the dimensions of the selection (X Y)",
            Button::Move => "Move the selection area",
            Button::Undo => "Undo the last modification",
            Button::Redo => "Redo the next modification",
            Button::Copy => "Copy selection to clipboard",
            Button::Save => "Save screenshot to a file",
            Button::Exit => "Leave the capture screen",
            Button::ImageUploader => "Upload the selection to Imgur",
            Button::OpenApp => "Choose an app to open the capture",
            Button::Pin => "Pin image on the desktop",
            Button::SizeIncrease => "Increase the tool's thickness",
            Button::SizeDecrease => "Decrease the tool's thickness",
            Button::Accept => "Accept the capture",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Button::Tool(t) => match t {
                Tool::Pencil => "P",
                Tool::Line => "D",
                Tool::Arrow => "A",
                Tool::Selection => "S",
                Tool::Rectangle => "R",
                Tool::Circle => "C",
                Tool::Marker => "M",
                Tool::Text => "T",
                Tool::Pixelate => "B",
                Tool::CircleCount => "N",
                Tool::Invert => "I",
            },
            Button::SelectionIndicator => "",
            Button::Move => "Ctrl+M",
            Button::Undo => "Ctrl+Z",
            Button::Redo => "Ctrl+Shift+Z",
            Button::Copy => "Ctrl+C",
            Button::Save => "Ctrl+S",
            Button::Exit => "Ctrl+Q",
            Button::ImageUploader => "Ctrl+U",
            Button::OpenApp => "Ctrl+O",
            Button::Pin => "Ctrl+P",
            Button::SizeIncrease => "+",
            Button::SizeDecrease => "-",
            Button::Accept => "Enter",
        }
    }
}

/// Full list of keyboard shortcuts, for the help/config screens.
pub fn shortcut_table() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&str, &str)> = Button::all()
        .into_iter()
        .filter(|b| !b.shortcut().is_empty())
        .map(|b| (b.shortcut(), b.description()))
        .collect();
    v.extend_from_slice(&[
        ("Esc", "Quit capture"),
        ("Enter", "Accept capture (copy to clipboard / requested action)"),
        ("Ctrl+Enter", "Commit current text"),
        ("Space", "Toggle visibility of sidebar with options of the selected tool"),
        ("Ctrl+A", "Select entire screen"),
        ("←↓↑→", "Move selection 1px"),
        ("Shift+←↓↑→", "Resize selection 1px"),
        ("Delete", "Delete the selected drawing object"),
        ("Mouse Wheel", "Change the tool's thickness"),
        ("Right Click", "Show color picker"),
        ("Shift+Drag", "Constrain lines to 45° / square shapes"),
    ]);
    v
}
