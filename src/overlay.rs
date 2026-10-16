//! The interactive capture screen (Flameshot's `CaptureWidget`).
//!
//! A fullscreen window shows the frozen desktop. The user drags a selection,
//! annotates it with the tools placed around it and finally accepts, copies,
//! saves, pins, uploads or opens the result.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use egui::{
    Align2, Color32, CursorIcon, Event, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};
use image::RgbaImage;
use tiny_skia::Pixmap;

use crate::annotation::{Annotation, Shape, TextStyle};
use crate::config::{Config, State, TextAlign};
use crate::render::{self, Fonts};
use crate::tools::{Button, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitAction {
    Accept,
    Copy,
    Save,
    Upload,
    OpenApp,
    Pin,
}

pub struct CaptureResult {
    pub image: RgbaImage,
    pub action: ExitAction,
    /// x, y, w, h in image pixels.
    pub region: [i32; 4],
    /// Top-left of the selection in logical (window) coordinates.
    pub logical_pos: (f32, f32),
    /// Image pixels per logical pixel.
    pub scale: f32,
}

#[derive(Default, Clone)]
pub struct OverlayOptions {
    pub accept_on_select: bool,
    pub region: Option<[i32; 4]>,
}

pub fn run(img: RgbaImage, cfg: Config, opts: OverlayOptions) -> anyhow::Result<Option<CaptureResult>> {
    let result: Rc<RefCell<Option<CaptureResult>>> = Rc::default();
    let out = result.clone();
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fastshot")
            .with_app_id(crate::APP_ID)
            .with_icon(crate::icon::icon_data(64))
            .with_decorations(false)
            .with_fullscreen(true)
            .with_always_on_top(),
        centered: false,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "fastshot-capture",
        native,
        Box::new(move |cc| {
            crate::ui::setup_fonts(&cc.egui_ctx);
            Ok(Box::new(CaptureApp::new(img, cfg, opts, out)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;
    let r = result.borrow_mut().take();
    Ok(r)
}

// ------------------------------------------------------------- helpers ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    const ALL: [Handle; 8] = [
        Handle::TopLeft,
        Handle::Top,
        Handle::TopRight,
        Handle::Right,
        Handle::BottomRight,
        Handle::Bottom,
        Handle::BottomLeft,
        Handle::Left,
    ];

    fn pos(self, r: Rect) -> Pos2 {
        match self {
            Handle::TopLeft => r.left_top(),
            Handle::Top => r.center_top(),
            Handle::TopRight => r.right_top(),
            Handle::Right => r.right_center(),
            Handle::BottomRight => r.right_bottom(),
            Handle::Bottom => r.center_bottom(),
            Handle::BottomLeft => r.left_bottom(),
            Handle::Left => r.left_center(),
        }
    }

    fn cursor(self) -> CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
            Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
            Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
        }
    }

    fn apply(self, orig: Rect, d: Vec2) -> Rect {
        let (mut l, mut t, mut r, mut b) = (orig.min.x, orig.min.y, orig.max.x, orig.max.y);
        match self {
            Handle::TopLeft => {
                l += d.x;
                t += d.y
            }
            Handle::Top => t += d.y,
            Handle::TopRight => {
                r += d.x;
                t += d.y
            }
            Handle::Right => r += d.x,
            Handle::BottomRight => {
                r += d.x;
                b += d.y
            }
            Handle::Bottom => b += d.y,
            Handle::BottomLeft => {
                l += d.x;
                b += d.y
            }
            Handle::Left => l += d.x,
        }
        Rect::from_two_pos(pos2(l, t), pos2(r, b))
    }
}

enum Drag {
    None,
    NewSelection { origin: Pos2 },
    MoveSelection { grab: Pos2, orig: Rect },
    Resize { handle: Handle, grab: Pos2, orig: Rect },
    Draw,
    MoveObject { idx: usize, last: Pos2, before: Vec<Annotation>, moved: bool },
}

struct TextEdit {
    /// Index the text had in the list when re-editing an existing object.
    orig_idx: Option<usize>,
    before: Option<Vec<Annotation>>,
    pos: Pos2,
    text: String,
    cursor: usize,
}

#[derive(Clone, Copy)]
enum Dirty {
    Clean,
    Full,
    Region(Rect),
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn from_c32(c: Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}

fn clamp_rect(r: Rect, bounds: Rect) -> Rect {
    Rect::from_min_max(r.min.max(bounds.min), r.max.min(bounds.max))
}

fn round_rect(r: Rect) -> Rect {
    Rect::from_min_max(r.min.round(), r.max.round())
}

// ----------------------------------------------------------------- app ----

struct CaptureApp {
    cfg: Config,
    fonts: Fonts,
    opts: OverlayOptions,
    result: Rc<RefCell<Option<CaptureResult>>>,

    base: Pixmap,
    committed: Pixmap,
    display: Pixmap,
    texture: Option<TextureHandle>,
    dirty: Dirty,
    img_rect: Rect,

    /// Image pixels per egui point.
    scale: f32,
    selection: Option<Rect>,
    anns: Vec<Annotation>,
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
    live: Option<Annotation>,
    live_counter: usize,
    live_bounds: Option<Rect>,

    tool: Option<Tool>,
    color: [u8; 4],
    sizes: HashMap<Tool, f32>,
    text_style: TextStyle,
    drag: Drag,
    text_edit: Option<TextEdit>,
    selected: Option<usize>,

    side_panel: bool,
    show_help: bool,
    wheel_center: Option<Pos2>,
    grab_color: bool,
    scroll_acc: f32,
    size_feedback_until: f64,
    closing: bool,
    first_frame: bool,
    script: Option<crate::script::Script>,
}

impl CaptureApp {
    fn new(
        img: RgbaImage,
        cfg: Config,
        opts: OverlayOptions,
        result: Rc<RefCell<Option<CaptureResult>>>,
    ) -> Self {
        let base = render::pixmap_from_rgba(&img);
        let fonts = Fonts::load(&cfg.font_family);
        let all_tools = Button::all().into_iter().filter_map(|b| match b {
            Button::Tool(t) => Some(t),
            _ => None,
        });
        let sizes = all_tools
            .map(|t| (t, cfg.tool_size(t.key(), t.default_size())))
            .collect();
        let text_style = TextStyle {
            bold: cfg.text_bold,
            italic: cfg.text_italic,
            underline: cfg.text_underline,
            strikeout: cfg.text_strikeout,
            align: cfg.text_align,
        };
        let selection = opts.region.map(|[x, y, w, h]| {
            Rect::from_min_size(pos2(x as f32, y as f32), vec2(w as f32, h as f32))
        });
        Self {
            color: cfg.draw_color,
            show_help: cfg.show_help && selection.is_none(),
            fonts,
            opts,
            result,
            committed: base.clone(),
            display: base.clone(),
            img_rect: Rect::from_min_size(
                Pos2::ZERO,
                vec2(base.width() as f32, base.height() as f32),
            ),
            base,
            texture: None,
            dirty: Dirty::Full,
            scale: 1.0,
            selection,
            anns: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            live: None,
            live_counter: 1,
            live_bounds: None,
            tool: None,
            sizes,
            text_style,
            drag: Drag::None,
            text_edit: None,
            selected: None,
            side_panel: false,
            wheel_center: None,
            grab_color: false,
            scroll_acc: 0.0,
            size_feedback_until: 0.0,
            closing: false,
            first_frame: true,
            script: crate::script::Script::from_env(),
            cfg,
        }
    }

    // ------------------------------------------------------- coordinates --

    fn to_img(&self, p: Pos2) -> Pos2 {
        pos2(p.x * self.scale, p.y * self.scale)
    }

    fn to_screen(&self, p: Pos2) -> Pos2 {
        pos2(p.x / self.scale, p.y / self.scale)
    }

    fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }

    // ---------------------------------------------------------- painting --

    fn mark_dirty(&mut self, r: Rect) {
        self.dirty = match self.dirty {
            Dirty::Full => Dirty::Full,
            Dirty::Clean => Dirty::Region(r),
            Dirty::Region(o) => Dirty::Region(o.union(r)),
        };
    }

    /// Re-render all committed objects (after undo, commit, delete...).
    fn rebuild(&mut self) {
        self.committed = self.base.clone();
        render::draw_all(&mut self.committed, &self.anns, self.scale, &self.fonts);
        self.display = self.committed.clone();
        self.live_bounds = None;
        self.dirty = Dirty::Full;
        self.redraw_live();
    }

    /// Restore `display` from `committed` in `r`.
    fn restore(&mut self, r: Rect) {
        let r = round_rect(clamp_rect(r.expand(2.0), self.img_rect));
        let (x0, y0, x1, y1) = (r.min.x as usize, r.min.y as usize, r.max.x as usize, r.max.y as usize);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let w = self.base.width() as usize;
        let src = self.committed.data();
        let dst = self.display.data_mut();
        for y in y0..y1 {
            let a = (y * w + x0) * 4;
            let b = (y * w + x1) * 4;
            dst[a..b].copy_from_slice(&src[a..b]);
        }
    }

    /// Paint the object being drawn on top of the committed layer.
    fn redraw_live(&mut self) {
        if let Some(old) = self.live_bounds.take() {
            self.restore(old);
            self.mark_dirty(old.expand(2.0));
        }
        let Some(live) = self.live.clone() else { return };
        let b = live.bounds(self.scale, &self.fonts);
        self.restore(b);
        render::draw(&mut self.display, &live, self.live_counter, self.scale, &self.fonts);
        self.mark_dirty(b.expand(2.0));
        self.live_bounds = Some(b);
    }

    fn upload(&mut self, ctx: &egui::Context) {
        let (w, h) = (self.base.width() as usize, self.base.height() as usize);
        match self.dirty {
            Dirty::Clean => {}
            Dirty::Full => {
                let img = egui::ColorImage::from_rgba_premultiplied([w, h], self.display.data());
                match &mut self.texture {
                    Some(t) => t.set(img, TextureOptions::NEAREST),
                    None => {
                        self.texture = Some(ctx.load_texture("capture", img, TextureOptions::NEAREST))
                    }
                }
            }
            Dirty::Region(r) => {
                let r = round_rect(clamp_rect(r, self.img_rect));
                let (x0, y0) = (r.min.x as usize, r.min.y as usize);
                let (rw, rh) = (r.width() as usize, r.height() as usize);
                if rw > 0
                    && rh > 0
                    && let Some(t) = &mut self.texture
                {
                    let data = self.display.data();
                    let mut buf = Vec::with_capacity(rw * rh * 4);
                    for y in y0..y0 + rh {
                        let a = (y * w + x0) * 4;
                        buf.extend_from_slice(&data[a..a + rw * 4]);
                    }
                    let img = egui::ColorImage::from_rgba_premultiplied([rw, rh], &buf);
                    t.set_partial([x0, y0], img, TextureOptions::NEAREST);
                }
            }
        }
        self.dirty = Dirty::Clean;
    }

    // ----------------------------------------------------------- history --

    fn push_undo(&mut self, before: Vec<Annotation>) {
        self.undo.push(before);
        if self.undo.len() > self.cfg.undo_limit.max(1) {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        self.commit_text();
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.anns, prev));
            self.selected = None;
            self.rebuild();
        }
    }

    fn redo(&mut self) {
        self.commit_text();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.anns, next));
            self.selected = None;
            self.rebuild();
        }
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected.take()
            && i < self.anns.len()
        {
            let before = self.anns.clone();
            self.anns.remove(i);
            self.push_undo(before);
            self.rebuild();
        }
    }

    // -------------------------------------------------------- properties --

    fn size(&self, t: Tool) -> f32 {
        self.sizes.get(&t).copied().unwrap_or(t.default_size())
    }

    fn current_size_tool(&self) -> Option<Tool> {
        if let Some(i) = self.selected {
            return self.anns.get(i).map(|a| a.tool());
        }
        if self.text_edit.is_some() {
            return Some(Tool::Text);
        }
        self.tool
    }

    fn change_size(&mut self, delta: f32, now: f64) {
        let Some(t) = self.current_size_tool() else { return };
        let v = (self.size(t) + delta).clamp(t.min_size(), t.max_size());
        self.set_size(t, v);
        self.size_feedback_until = now + 1.0;
    }

    fn set_size(&mut self, t: Tool, v: f32) {
        self.sizes.insert(t, v);
        if let Some(i) = self.selected
            && let Some(a) = self.anns.get(i)
            && a.size != v
        {
            let before = self.anns.clone();
            self.anns[i].size = v;
            self.push_undo(before);
            self.rebuild();
        }
        if self.text_edit.is_some() {
            self.update_text_live();
        }
    }

    fn set_color(&mut self, c: [u8; 4]) {
        self.color = c;
        if let Some(i) = self.selected
            && let Some(a) = self.anns.get(i)
            && a.color != c
        {
            let before = self.anns.clone();
            self.anns[i].color = c;
            self.push_undo(before);
            self.rebuild();
        }
        if self.text_edit.is_some() {
            self.update_text_live();
        }
    }

    fn set_tool(&mut self, t: Option<Tool>) {
        self.commit_text();
        self.selected = None;
        self.grab_color = false;
        self.tool = if self.tool == t { None } else { t };
    }

    // -------------------------------------------------------------- text --

    fn update_text_live(&mut self) {
        if let Some(te) = &self.text_edit {
            self.live = Some(Annotation {
                shape: Shape::Text {
                    pos: te.pos,
                    text: te.text.clone(),
                    style: self.text_style.clone(),
                },
                color: self.color,
                size: self.size(Tool::Text),
            });
            self.redraw_live();
        }
    }

    fn start_text(&mut self, p: Pos2) {
        // Re-edit an existing text object when clicking on it.
        let hit = self
            .anns
            .iter()
            .rposition(|a| matches!(a.shape, Shape::Text { .. }) && a.hit(p, self.scale, &self.fonts));
        if let Some(i) = hit {
            let before = self.anns.clone();
            let a = self.anns.remove(i);
            if let Shape::Text { pos, text, style } = a.shape {
                self.color = a.color;
                self.sizes.insert(Tool::Text, a.size);
                self.text_style = style;
                let cursor = text.chars().count();
                self.text_edit = Some(TextEdit {
                    orig_idx: Some(i),
                    before: Some(before),
                    pos,
                    text,
                    cursor,
                });
            }
            self.rebuild();
        } else {
            let px = self.size(Tool::Text) * self.scale;
            self.text_edit = Some(TextEdit {
                orig_idx: None,
                before: None,
                pos: pos2(p.x, p.y - px * 0.6),
                text: String::new(),
                cursor: 0,
            });
        }
        self.live_counter = 0;
        self.update_text_live();
    }

    fn commit_text(&mut self) {
        let Some(te) = self.text_edit.take() else { return };
        let ann = self.live.take();
        let before = te.before.unwrap_or_else(|| self.anns.clone());
        if let Some(a) = ann.filter(|a| !a.is_empty()) {
            let idx = te.orig_idx.unwrap_or(self.anns.len()).min(self.anns.len());
            self.anns.insert(idx, a);
            self.push_undo(before);
        } else if te.orig_idx.is_some() {
            // Existing text was erased completely: that is a deletion.
            self.push_undo(before);
        }
        self.rebuild();
    }

    fn text_input(&mut self, events: &[Event]) -> Vec<Event> {
        // Returns the events that should still be handled as shortcuts.
        let mut rest = Vec::new();
        let mut changed = false;
        for ev in events {
            let Some(te) = self.text_edit.as_mut() else {
                rest.push(ev.clone());
                continue;
            };
            let byte = |s: &str, ci: usize| s.char_indices().nth(ci).map_or(s.len(), |(b, _)| b);
            match ev {
                Event::Text(t) | Event::Paste(t) => {
                    let t: String = t.chars().filter(|c| !c.is_control() || *c == '\n').collect();
                    let at = byte(&te.text, te.cursor);
                    te.text.insert_str(at, &t);
                    te.cursor += t.chars().count();
                    changed = true;
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if modifiers.command && *key != Key::Enter {
                        // Shortcuts like Ctrl+C finish the text first.
                        rest.push(ev.clone());
                        continue;
                    }
                    match key {
                        Key::Escape => {
                            self.commit_text();
                            return Vec::new();
                        }
                        Key::Enter if modifiers.command => {
                            self.commit_text();
                            return Vec::new();
                        }
                        Key::Enter => {
                            let at = byte(&te.text, te.cursor);
                            te.text.insert(at, '\n');
                            te.cursor += 1;
                            changed = true;
                        }
                        Key::Backspace if te.cursor > 0 => {
                            let at = byte(&te.text, te.cursor - 1);
                            te.text.remove(at);
                            te.cursor -= 1;
                            changed = true;
                        }
                        Key::Delete if te.cursor < te.text.chars().count() => {
                            let at = byte(&te.text, te.cursor);
                            te.text.remove(at);
                            changed = true;
                        }
                        Key::ArrowLeft => te.cursor = te.cursor.saturating_sub(1),
                        Key::ArrowRight => te.cursor = (te.cursor + 1).min(te.text.chars().count()),
                        Key::Home => {
                            let before: Vec<char> = te.text.chars().take(te.cursor).collect();
                            te.cursor = before.iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
                        }
                        Key::End => {
                            let chars: Vec<char> = te.text.chars().collect();
                            te.cursor = chars[te.cursor..]
                                .iter()
                                .position(|c| *c == '\n')
                                .map_or(chars.len(), |i| te.cursor + i);
                        }
                        _ => {}
                    }
                }
                Event::Copy | Event::Cut => rest.push(ev.clone()),
                _ => {}
            }
        }
        if changed {
            self.update_text_live();
        }
        rest
    }

    // ------------------------------------------------------------- mouse --

    fn handle_at(&self, sp: Pos2) -> Option<Handle> {
        let sel = self.rect_to_screen(self.selection?);
        let r = 9.0;
        Handle::ALL
            .into_iter()
            .find(|h| h.pos(sel).distance(sp) <= r)
            .or_else(|| {
                // Grabbing the border itself also resizes.
                if self.tool.is_some() {
                    return None;
                }
                let m = 4.0;
                let inside_v = sp.y > sel.min.y + r && sp.y < sel.max.y - r;
                let inside_h = sp.x > sel.min.x + r && sp.x < sel.max.x - r;
                if inside_v && (sp.x - sel.min.x).abs() <= m {
                    Some(Handle::Left)
                } else if inside_v && (sp.x - sel.max.x).abs() <= m {
                    Some(Handle::Right)
                } else if inside_h && (sp.y - sel.min.y).abs() <= m {
                    Some(Handle::Top)
                } else if inside_h && (sp.y - sel.max.y).abs() <= m {
                    Some(Handle::Bottom)
                } else {
                    None
                }
            })
    }

    fn object_at(&self, p: Pos2) -> Option<usize> {
        self.anns.iter().rposition(|a| a.hit(p, self.scale, &self.fonts))
    }

    fn on_press(&mut self, sp: Pos2) {
        let p = self.to_img(sp);
        if self.grab_color {
            self.grab_color = false;
            let (x, y) = (p.x as u32, p.y as u32);
            if let Some(px) = self.display.pixel(x, y) {
                let c = px.demultiply();
                self.set_color([c.red(), c.green(), c.blue(), 255]);
            }
            return;
        }
        if let Some(te) = &self.text_edit {
            let px = self.size(Tool::Text) * self.scale;
            let size = self.fonts.measure(&te.text, px, &self.text_style);
            if Rect::from_min_size(te.pos, size).expand(4.0).contains(p) {
                return;
            }
            self.commit_text();
            return;
        }
        if let Some(h) = self.handle_at(sp) {
            let orig = self.selection.unwrap();
            self.drag = Drag::Resize { handle: h, grab: p, orig };
            return;
        }
        if let Some(i) = self.selected
            && self.anns.get(i).is_some_and(|a| a.hit(p, self.scale, &self.fonts))
        {
            self.begin_move_object(i, p);
            return;
        }
        self.selected = None;
        let sel = self.selection;
        match (self.tool, sel) {
            (Some(Tool::Text), Some(_)) => self.start_text(p),
            (Some(t), Some(_)) => {
                let ann = Annotation::start(t, p, self.color, self.size(t), self.text_style.clone());
                self.live_counter = render::counter_numbers(&self.anns).last().copied().unwrap_or(0) + 1;
                self.live = Some(ann);
                self.drag = Drag::Draw;
                self.redraw_live();
            }
            (None, Some(s)) if s.contains(p) => {
                if let Some(i) = self.object_at(p) {
                    self.selected = Some(i);
                    self.begin_move_object(i, p);
                } else {
                    self.drag = Drag::MoveSelection { grab: p, orig: s };
                }
            }
            _ => {
                self.show_help = false;
                self.selection = Some(Rect::from_min_max(p, p));
                self.drag = Drag::NewSelection { origin: p };
            }
        }
    }

    fn begin_move_object(&mut self, i: usize, p: Pos2) {
        let before = self.anns.clone();
        let nums = render::counter_numbers(&self.anns);
        let ann = self.anns.remove(i);
        self.live_counter = nums[i];
        self.rebuild();
        self.live = Some(ann);
        self.redraw_live();
        self.drag = Drag::MoveObject { idx: i, last: p, before, moved: false };
    }

    fn on_drag(&mut self, sp: Pos2, shift: bool) {
        let p = self.to_img(sp);
        let bounds = self.img_rect;
        match &mut self.drag {
            Drag::None => {}
            Drag::NewSelection { origin } => {
                self.selection = Some(clamp_rect(Rect::from_two_pos(*origin, p), bounds));
            }
            Drag::MoveSelection { grab, orig } => {
                let mut d = p - *grab;
                d.x = d.x.clamp(bounds.min.x - orig.min.x, bounds.max.x - orig.max.x);
                d.y = d.y.clamp(bounds.min.y - orig.min.y, bounds.max.y - orig.max.y);
                self.selection = Some(round_rect(orig.translate(d)));
            }
            Drag::Resize { handle, grab, orig } => {
                let r = handle.apply(*orig, p - *grab);
                self.selection = Some(clamp_rect(r, bounds));
            }
            Drag::Draw => {
                if let Some(l) = &mut self.live {
                    l.drag_to(p, shift);
                }
                self.redraw_live();
            }
            Drag::MoveObject { last, moved, .. } => {
                let d = p - *last;
                if d != Vec2::ZERO {
                    *last = p;
                    *moved = true;
                    if let Some(l) = &mut self.live {
                        l.translate(d);
                    }
                    self.redraw_live();
                }
            }
        }
    }

    fn on_release(&mut self) {
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::NewSelection { .. } => {
                if let Some(s) = self.selection {
                    if s.width() < 2.0 || s.height() < 2.0 {
                        self.selection = None;
                        self.show_help = self.cfg.show_help;
                    } else {
                        self.selection = Some(round_rect(s));
                        if self.opts.accept_on_select {
                            self.finish(ExitAction::Accept);
                        }
                    }
                }
            }
            Drag::Resize { .. } | Drag::MoveSelection { .. } => {
                self.selection = self.selection.map(round_rect);
            }
            Drag::Draw => {
                if let Some(a) = self.live.take() {
                    if !a.is_empty() {
                        let before = self.anns.clone();
                        self.anns.push(a);
                        self.push_undo(before);
                    }
                    self.rebuild();
                }
            }
            Drag::MoveObject { idx, before, moved, .. } => {
                if let Some(a) = self.live.take() {
                    let idx = idx.min(self.anns.len());
                    self.anns.insert(idx, a);
                    self.selected = Some(idx);
                    if moved {
                        self.push_undo(before);
                    }
                    self.rebuild();
                }
            }
            Drag::None => {}
        }
    }

    // ----------------------------------------------------------- actions --

    fn select_all(&mut self) {
        self.show_help = false;
        self.selection = Some(self.img_rect);
    }

    fn nudge(&mut self, key: Key, shift: bool) {
        let Some(s) = self.selection else { return };
        let step = self.scale.round().max(1.0);
        let d = match key {
            Key::ArrowLeft => vec2(-step, 0.0),
            Key::ArrowRight => vec2(step, 0.0),
            Key::ArrowUp => vec2(0.0, -step),
            _ => vec2(0.0, step),
        };
        let r = if shift {
            Rect::from_min_max(s.min, (s.max + d).max(s.min + Vec2::splat(1.0)))
        } else {
            let mut d = d;
            d.x = d.x.clamp(-s.min.x, self.img_rect.max.x - s.max.x);
            d.y = d.y.clamp(-s.min.y, self.img_rect.max.y - s.max.y);
            s.translate(d)
        };
        self.selection = Some(clamp_rect(r, self.img_rect));
    }

    fn finish(&mut self, action: ExitAction) {
        if self.closing {
            return;
        }
        self.commit_text();
        let sel = self
            .selection
            .map(|s| round_rect(clamp_rect(s, self.img_rect)))
            .filter(|s| s.width() >= 1.0 && s.height() >= 1.0)
            .unwrap_or(self.img_rect);
        let region = [
            sel.min.x as i32,
            sel.min.y as i32,
            sel.width() as i32,
            sel.height() as i32,
        ];
        let full = render::rgba_from_pixmap(&self.committed);
        let image = image::imageops::crop_imm(
            &full,
            region[0] as u32,
            region[1] as u32,
            region[2] as u32,
            region[3] as u32,
        )
        .to_image();
        let lp = self.to_screen(sel.min);
        *self.result.borrow_mut() = Some(CaptureResult {
            image,
            action,
            region,
            logical_pos: (lp.x, lp.y),
            scale: self.scale,
        });
        if self.cfg.save_last_region {
            State { last_region: Some(region) }.save();
        }
        self.closing = true;
        self.save_prefs();
    }

    fn cancel(&mut self) {
        self.closing = true;
        self.save_prefs();
    }

    fn save_prefs(&mut self) {
        let mut cfg = crate::config::Config::load();
        cfg.draw_color = self.color;
        for (t, s) in &self.sizes {
            cfg.tool_sizes.insert(t.key().to_owned(), *s);
        }
        cfg.text_bold = self.text_style.bold;
        cfg.text_italic = self.text_style.italic;
        cfg.text_underline = self.text_style.underline;
        cfg.text_strikeout = self.text_style.strikeout;
        cfg.text_align = self.text_style.align;
        let _ = cfg.save();
    }

    fn press_button(&mut self, b: Button, now: f64) {
        match b {
            Button::Tool(t) => self.set_tool(Some(t)),
            Button::SelectionIndicator => {}
            Button::Move => self.set_tool(None),
            Button::Undo => self.undo(),
            Button::Redo => self.redo(),
            Button::Copy => self.finish(ExitAction::Copy),
            Button::Save => self.finish(ExitAction::Save),
            Button::Exit => self.cancel(),
            Button::ImageUploader => self.finish(ExitAction::Upload),
            Button::OpenApp => self.finish(ExitAction::OpenApp),
            Button::Pin => self.finish(ExitAction::Pin),
            Button::SizeIncrease => self.change_size(1.0, now),
            Button::SizeDecrease => self.change_size(-1.0, now),
            Button::Accept => self.finish(ExitAction::Accept),
        }
    }

    fn handle_keys(&mut self, events: Vec<Event>, now: f64) {
        for ev in events {
            if matches!(ev, Event::Copy) {
                // egui-winit turns Ctrl+C into a Copy event.
                self.finish(ExitAction::Copy);
                continue;
            }
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let m: Modifiers = modifiers;
            if m.command {
                match key {
                    Key::C => self.finish(ExitAction::Copy),
                    Key::S => self.finish(ExitAction::Save),
                    Key::Q => self.cancel(),
                    Key::U => self.finish(ExitAction::Upload),
                    Key::O => self.finish(ExitAction::OpenApp),
                    Key::P => self.finish(ExitAction::Pin),
                    Key::Z if m.shift => self.redo(),
                    Key::Z => self.undo(),
                    Key::Y => self.redo(),
                    Key::M => self.set_tool(None),
                    Key::A => self.select_all(),
                    Key::Enter => self.commit_text(),
                    _ => {}
                }
                continue;
            }
            match key {
                Key::Escape => {
                    if self.grab_color {
                        self.grab_color = false;
                    } else if self.wheel_center.is_some() {
                        self.wheel_center = None;
                    } else {
                        self.cancel();
                    }
                }
                Key::Enter => self.finish(ExitAction::Accept),
                Key::Space => self.side_panel = !self.side_panel,
                Key::Delete | Key::Backspace => self.delete_selected(),
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                    self.nudge(key, m.shift)
                }
                Key::Plus | Key::Equals => self.change_size(1.0, now),
                Key::Minus => self.change_size(-1.0, now),
                Key::P => self.set_tool(Some(Tool::Pencil)),
                Key::D => self.set_tool(Some(Tool::Line)),
                Key::A => self.set_tool(Some(Tool::Arrow)),
                Key::S => self.set_tool(Some(Tool::Selection)),
                Key::R => self.set_tool(Some(Tool::Rectangle)),
                Key::C => self.set_tool(Some(Tool::Circle)),
                Key::M => self.set_tool(Some(Tool::Marker)),
                Key::T => self.set_tool(Some(Tool::Text)),
                Key::B => self.set_tool(Some(Tool::Pixelate)),
                Key::N => self.set_tool(Some(Tool::CircleCount)),
                Key::I => self.set_tool(Some(Tool::Invert)),
                _ => {}
            }
        }
    }

    // ---------------------------------------------------------------- UI --

    fn ui_color(&self) -> Color32 {
        c32(self.cfg.ui_color)
    }

    fn contrast_color(&self) -> Color32 {
        c32(self.cfg.contrast_color)
    }

    fn paint_scene(&self, painter: &egui::Painter, screen: Rect, now: f64, pointer: Option<Pos2>) {
        if let Some(tex) = &self.texture {
            painter.image(
                tex.id(),
                self.rect_to_screen(self.img_rect),
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let dim = Color32::from_black_alpha(self.cfg.contrast_opacity);
        let ui = self.ui_color();
        match self.selection.map(|s| self.rect_to_screen(s)) {
            None => {
                painter.rect_filled(screen, 0.0, dim);
            }
            Some(sel) => {
                let parts = [
                    Rect::from_min_max(screen.min, pos2(screen.max.x, sel.min.y)),
                    Rect::from_min_max(pos2(screen.min.x, sel.max.y), screen.max),
                    Rect::from_min_max(pos2(screen.min.x, sel.min.y), pos2(sel.min.x, sel.max.y)),
                    Rect::from_min_max(pos2(sel.max.x, sel.min.y), pos2(screen.max.x, sel.max.y)),
                ];
                for r in parts {
                    if r.is_positive() {
                        painter.rect_filled(r, 0.0, dim);
                    }
                }
                painter.rect_stroke(sel, 0.0, Stroke::new(1.0, ui), StrokeKind::Outside);
                if self.text_edit.is_none() {
                    for h in Handle::ALL {
                        let c = h.pos(sel);
                        painter.circle_filled(c, 4.5, ui);
                        painter.circle_stroke(c, 4.5, Stroke::new(1.0, Color32::WHITE));
                    }
                }
                let resizing = matches!(
                    self.drag,
                    Drag::NewSelection { .. } | Drag::Resize { .. } | Drag::MoveSelection { .. }
                );
                if self.cfg.show_selection_geometry && resizing {
                    let s = self.selection.unwrap();
                    let txt = format!(
                        "{}×{}  ({}, {})",
                        s.width() as i32,
                        s.height() as i32,
                        s.min.x as i32,
                        s.min.y as i32
                    );
                    let font = FontId::proportional(13.0);
                    let galley = painter.layout_no_wrap(txt, font, Color32::WHITE);
                    let mut pos = sel.min + vec2(4.0, 4.0);
                    if sel.height() < galley.size().y + 12.0 || sel.width() < galley.size().x + 12.0 {
                        pos = pos2(sel.min.x, sel.min.y - galley.size().y - 8.0).max(screen.min);
                    }
                    let bg = Rect::from_min_size(pos, galley.size() + vec2(8.0, 4.0));
                    painter.rect_filled(bg, 3.0, Color32::from_black_alpha(170));
                    painter.galley(pos + vec2(4.0, 2.0), galley, Color32::WHITE);
                }
            }
        }

        // Selected object outline.
        let dashed = |r: Rect| {
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, ui), 4.0, 3.0));
        };
        if let Some(i) = self.selected
            && let Some(a) = self.anns.get(i)
        {
            dashed(self.rect_to_screen(a.bounds(self.scale, &self.fonts)));
        }
        if matches!(self.drag, Drag::MoveObject { .. })
            && let Some(l) = &self.live
        {
            dashed(self.rect_to_screen(l.bounds(self.scale, &self.fonts)));
        }

        // Text editor frame and caret.
        if let Some(te) = &self.text_edit {
            let px = self.size(Tool::Text) * self.scale;
            let layout = self.fonts.layout(&te.text, px, &self.text_style);
            let size = self.fonts.measure(&te.text, px, &self.text_style);
            dashed(self.rect_to_screen(Rect::from_min_size(te.pos, size).expand(3.0)));
            let off = render::caret_offset(&layout, self.text_style.align, te.cursor);
            let top = self.to_screen(te.pos + off);
            let blink = (now * 2.0) as i64 % 2 == 0;
            if blink {
                painter.line_segment(
                    [top, top + vec2(0.0, layout.line_height / self.scale)],
                    Stroke::new(1.5, c32(self.color)),
                );
            }
        }

        // Tool size feedback.
        if now < self.size_feedback_until
            && let (Some(p), Some(t)) = (pointer, self.current_size_tool())
        {
            let s = self.size(t);
            let r = (s / 2.0).max(1.0);
            painter.circle_stroke(p, r, Stroke::new(1.0, c32(self.color)));
            painter.text(
                p + vec2(r + 6.0, -r - 6.0),
                Align2::LEFT_BOTTOM,
                format!("{}", s as i32),
                FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
    }

    fn paint_magnifier(&self, painter: &egui::Painter, screen: Rect, p: Pos2) {
        let Some(tex) = &self.texture else { return };
        let size = 140.0;
        let zoom = 8.0;
        let mut pos = p + vec2(24.0, 24.0);
        if pos.x + size > screen.max.x {
            pos.x = p.x - 24.0 - size;
        }
        if pos.y + size > screen.max.y {
            pos.y = p.y - 24.0 - size;
        }
        let rect = Rect::from_min_size(pos, Vec2::splat(size));
        let ip = self.to_img(p);
        let half = size / zoom / 2.0 * self.scale;
        let (w, h) = (self.img_rect.width(), self.img_rect.height());
        let uv = Rect::from_min_max(
            pos2((ip.x.floor() - half) / w, (ip.y.floor() - half) / h),
            pos2((ip.x.floor() + half) / w, (ip.y.floor() + half) / h),
        );
        let round = if self.cfg.square_magnifier { 0.0 } else { size / 2.0 };
        painter.rect_filled(rect.expand(2.0), round, self.ui_color());
        painter.image(tex.id(), rect, uv, Color32::WHITE);
        let c = rect.center();
        let cell = size / (2.0 * half);
        painter.rect_stroke(
            Rect::from_center_size(c + vec2(cell / 2.0, cell / 2.0), Vec2::splat(cell)),
            0.0,
            Stroke::new(1.0, Color32::WHITE),
            StrokeKind::Outside,
        );
        if let Some(px) = self.display.pixel(ip.x as u32, ip.y as u32) {
            let txt = format!("#{:02X}{:02X}{:02X}", px.red(), px.green(), px.blue());
            painter.text(
                rect.center_bottom() + vec2(0.0, 6.0),
                Align2::CENTER_TOP,
                txt,
                FontId::monospace(12.0),
                Color32::WHITE,
            );
        }
    }

    fn paint_help(&self, ctx: &egui::Context, screen: Rect) {
        let lines = [
            "Press Enter to capture the screen.",
            "Press Right Click to show the color picker.",
            "Use the Mouse Wheel to change the thickness of your tool.",
            "Press Space to open the side panel.",
            "",
            "Drag to select an area · Esc to quit",
        ];
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, "help".into()));
        let font = FontId::proportional(16.0);
        let galleys: Vec<_> = lines
            .iter()
            .map(|l| painter.layout_no_wrap(l.to_string(), font.clone(), Color32::WHITE))
            .collect();
        let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 48.0;
        let lh = 24.0;
        let h = lh * lines.len() as f32 + 32.0;
        let rect = Rect::from_center_size(screen.center(), vec2(w, h));
        painter.rect_filled(rect, 10.0, Color32::from_black_alpha(200));
        painter.rect_stroke(rect, 10.0, Stroke::new(1.0, self.ui_color()), StrokeKind::Inside);
        for (i, g) in galleys.into_iter().enumerate() {
            let pos = pos2(rect.center().x - g.size().x / 2.0, rect.min.y + 16.0 + i as f32 * lh);
            painter.galley(pos, g, Color32::WHITE);
        }
    }

    fn buttons_ui(&mut self, ctx: &egui::Context, screen: Rect, now: f64) {
        let Some(sel) = self.selection.map(|s| self.rect_to_screen(s)) else { return };
        if matches!(self.drag, Drag::NewSelection { .. } | Drag::Resize { .. } | Drag::MoveSelection { .. }) {
            return;
        }
        let mut buttons: Vec<Button> = self.cfg.buttons.clone();
        if buttons.is_empty() {
            return;
        }
        if self.cfg.show_sidepanel_button {
            // nothing: the side panel has its own toggle in the panel corner
        }
        buttons.dedup();
        let size = self.cfg.button_size;
        let positions = layout_buttons(sel, screen, buttons.len(), size);
        let ui_color = self.ui_color();
        let contrast = self.contrast_color();
        let mut clicked = None;
        for (b, center) in buttons.iter().zip(positions) {
            let active = match b {
                Button::Tool(t) => self.tool == Some(*t),
                Button::Move => self.tool.is_none(),
                _ => false,
            };
            egui::Area::new(egui::Id::new(("btn", *b)))
                .order(egui::Order::Foreground)
                .fixed_pos(center - Vec2::splat(size / 2.0))
                .show(ctx, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
                    let p = ui.painter();
                    let hovered = resp.hovered();
                    let fill = if active {
                        contrast
                    } else if hovered {
                        ui_color.gamma_multiply(1.0).lerp_to_gamma(Color32::WHITE, 0.15)
                    } else {
                        ui_color
                    };
                    p.circle_filled(rect.center() + vec2(0.0, 1.5), size / 2.0, Color32::from_black_alpha(90));
                    p.circle_filled(rect.center(), size / 2.0, fill);
                    if *b == Button::SelectionIndicator {
                        let s = self.selection.unwrap();
                        p.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            format!("{}\n{}", s.width() as i32, s.height() as i32),
                            FontId::proportional(9.0),
                            Color32::WHITE,
                        );
                    } else {
                        p.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            b.icon(),
                            FontId::proportional(size * 0.55),
                            Color32::WHITE,
                        );
                    }
                    let tip = if b.shortcut().is_empty() {
                        b.description().to_string()
                    } else {
                        format!("{} ({})", b.description(), b.shortcut())
                    };
                    if resp.on_hover_text(tip).clicked() {
                        clicked = Some(*b);
                    }
                });
        }
        if let Some(b) = clicked {
            self.press_button(b, now);
        }
    }

    fn side_panel_ui(&mut self, ctx: &egui::Context, screen: Rect, now: f64) {
        if !self.side_panel {
            return;
        }
        let tool = self.current_size_tool();
        let mut new_size = None;
        let mut new_color = None;
        let mut style = self.text_style.clone();
        let mut grab = false;
        let mut undo = false;
        let mut redo = false;
        let mut close = false;
        egui::Area::new("side_panel".into())
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(30, 30, 30, 235))
                    .inner_margin(14.0)
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.spacing_mut().slider_width = 180.0;
                        ui.set_min_height(screen.height() - 28.0);
                        ui.horizontal(|ui| {
                            let name = tool.map(|t| Button::Tool(t).name()).unwrap_or("Move / Select");
                            ui.heading(name);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(egui_phosphor::regular::X).on_hover_text("Close (Space)").clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.separator();
                        if let Some(t) = tool {
                            let label = match t {
                                Tool::Text => "Font size",
                                Tool::Pixelate => "Pixel size",
                                Tool::Rectangle => "Corner radius",
                                _ => "Active thickness",
                            };
                            ui.label(label);
                            let mut v = self.size(t);
                            if ui
                                .add(egui::Slider::new(&mut v, t.min_size()..=t.max_size()).integer())
                                .changed()
                            {
                                new_size = Some((t, v));
                            }
                            if t == Tool::Text {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    use egui_phosphor::regular as ic;
                                    ui.toggle_value(&mut style.bold, ic::TEXT_B).on_hover_text("Bold");
                                    ui.toggle_value(&mut style.italic, ic::TEXT_ITALIC).on_hover_text("Italic");
                                    ui.toggle_value(&mut style.underline, ic::TEXT_UNDERLINE)
                                        .on_hover_text("Underline");
                                    ui.toggle_value(&mut style.strikeout, ic::TEXT_STRIKETHROUGH)
                                        .on_hover_text("Strikeout");
                                    ui.separator();
                                    ui.selectable_value(&mut style.align, TextAlign::Left, ic::TEXT_ALIGN_LEFT);
                                    ui.selectable_value(&mut style.align, TextAlign::Center, ic::TEXT_ALIGN_CENTER);
                                    ui.selectable_value(&mut style.align, TextAlign::Right, ic::TEXT_ALIGN_RIGHT);
                                });
                            }
                            ui.add_space(8.0);
                        }
                        ui.label("Color");
                        let mut c = c32(self.color);
                        if egui::color_picker::color_picker_color32(
                            ui,
                            &mut c,
                            egui::color_picker::Alpha::OnlyBlend,
                        ) {
                            new_color = Some(from_c32(c));
                        }
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for uc in self.cfg.user_colors.clone() {
                                let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                                ui.painter().circle_filled(r.center(), 9.0, c32(uc));
                                ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.0, Color32::GRAY));
                                if resp.clicked() {
                                    new_color = Some(uc);
                                }
                            }
                        });
                        ui.add_space(8.0);
                        if ui
                            .button(format!("{} Grab Color", egui_phosphor::regular::EYEDROPPER))
                            .clicked()
                        {
                            grab = true;
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            undo = ui
                                .add_enabled(!self.undo.is_empty(), egui::Button::new(format!("{} Undo", egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE)))
                                .clicked();
                            redo = ui
                                .add_enabled(!self.redo.is_empty(), egui::Button::new(format!("{} Redo", egui_phosphor::regular::ARROW_CLOCKWISE)))
                                .clicked();
                        });
                        ui.label(format!("Objects: {}", self.anns.len()));
                        ui.add_space(12.0);
                        ui.weak("Space — toggle this panel\nRight click — color wheel\nWheel — thickness");
                    });
            });
        if let Some((t, v)) = new_size {
            self.set_size(t, v);
            self.size_feedback_until = now + 0.5;
        }
        if let Some(c) = new_color {
            self.set_color(c);
        }
        if style != self.text_style {
            self.text_style = style;
            if let Some(i) = self.selected
                && let Some(Annotation { shape: Shape::Text { style: s, .. }, .. }) = self.anns.get_mut(i)
            {
                *s = self.text_style.clone();
                self.rebuild();
            }
            self.update_text_live();
        }
        if grab {
            self.grab_color = true;
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if close {
            self.side_panel = false;
        }
    }

    fn wheel_items(&self, center: Pos2) -> Vec<(Pos2, [u8; 4])> {
        let colors = &self.cfg.user_colors;
        let n = colors.len().max(1) as f32;
        let radius = (n * 34.0 / std::f32::consts::TAU).max(56.0);
        colors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 / n * std::f32::consts::TAU;
                (center + vec2(a.cos(), a.sin()) * radius, *c)
            })
            .collect()
    }

    fn wheel_hover(&self, center: Pos2, p: Pos2) -> Option<[u8; 4]> {
        self.wheel_items(center)
            .into_iter()
            .map(|(pos, c)| (pos.distance(p), c))
            .filter(|(d, _)| *d < 30.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, c)| c)
    }

    fn paint_wheel(&self, ctx: &egui::Context, center: Pos2, pointer: Option<Pos2>) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, "wheel".into()));
        let hovered = pointer.and_then(|p| self.wheel_hover(center, p));
        let items = self.wheel_items(center);
        let radius = items.first().map(|(p, _)| p.distance(center)).unwrap_or(56.0);
        painter.circle_filled(center, radius + 22.0, Color32::from_black_alpha(110));
        for (pos, c) in items {
            let r = if Some(c) == hovered { 16.0 } else { 12.0 };
            painter.circle_filled(pos, r, c32(c));
            painter.circle_stroke(pos, r, Stroke::new(1.5, Color32::WHITE));
        }
        let cur = hovered.unwrap_or(self.color);
        painter.circle_filled(center, 20.0, c32(cur));
        painter.circle_stroke(center, 20.0, Stroke::new(2.0, Color32::WHITE));
    }

    fn cursor_for(&self, sp: Pos2) -> CursorIcon {
        if self.grab_color {
            return CursorIcon::Crosshair;
        }
        match &self.drag {
            Drag::MoveSelection { .. } | Drag::MoveObject { .. } => return CursorIcon::Grabbing,
            Drag::Resize { handle, .. } => return handle.cursor(),
            Drag::NewSelection { .. } | Drag::Draw => return CursorIcon::Crosshair,
            Drag::None => {}
        }
        if self.text_edit.is_none()
            && let Some(h) = self.handle_at(sp)
        {
            return h.cursor();
        }
        let p = self.to_img(sp);
        if let Some(i) = self.selected
            && self.anns.get(i).is_some_and(|a| a.hit(p, self.scale, &self.fonts))
        {
            return CursorIcon::Move;
        }
        match (self.tool, self.selection) {
            (Some(Tool::Text), Some(_)) => CursorIcon::Text,
            (Some(_), Some(_)) => CursorIcon::Crosshair,
            (None, Some(s)) if s.contains(p) => {
                if self.object_at(p).is_some() {
                    CursorIcon::PointingHand
                } else {
                    CursorIcon::Grab
                }
            }
            _ => CursorIcon::Crosshair,
        }
    }
}

/// Place `n` round buttons around the selection, Flameshot style: first a row
/// below, then a column on the right, a row above, a column on the left, and
/// further rings outwards if needed. Falls back to inside the selection when
/// it covers the whole screen.
fn layout_buttons(sel: Rect, screen: Rect, n: usize, size: f32) -> Vec<Pos2> {
    let gap = 6.0;
    let step = size + gap;
    let half = size / 2.0;
    let mut out: Vec<Pos2> = Vec::with_capacity(n);
    let fits = |p: Pos2| screen.shrink(2.0).contains_rect(Rect::from_center_size(p, Vec2::splat(size)));
    let push_line = |out: &mut Vec<Pos2>, from: Pos2, dir: Vec2, len: f32| {
        let cap = ((len + gap) / step).floor().max(0.0) as usize;
        let k = cap.min(n - out.len());
        if k == 0 {
            return;
        }
        let total = (k - 1) as f32 * step;
        let start = from + dir * (len / 2.0 - total / 2.0);
        for i in 0..k {
            let p = start + dir * (i as f32 * step);
            if fits(p) && out.iter().all(|q| q.distance(p) > size * 0.9) {
                out.push(p);
            }
        }
    };
    for ring in 0..8 {
        if out.len() >= n {
            break;
        }
        let r = sel.expand(ring as f32 * step);
        let off = gap + half;
        // bottom, right, top, left
        push_line(&mut out, pos2(r.min.x, r.max.y + off), vec2(1.0, 0.0), r.width());
        if out.len() < n {
            push_line(&mut out, pos2(r.max.x + off, r.min.y), vec2(0.0, 1.0), r.height());
        }
        if out.len() < n {
            push_line(&mut out, pos2(r.min.x, r.min.y - off), vec2(1.0, 0.0), r.width());
        }
        if out.len() < n {
            push_line(&mut out, pos2(r.min.x - off, r.min.y), vec2(0.0, 1.0), r.height());
        }
    }
    // Not enough room outside: rows inside the bottom of the selection.
    let inner = sel.intersect(screen).shrink(gap);
    let mut row = 0;
    while out.len() < n && row < 20 {
        let y = inner.max.y - half - row as f32 * step;
        let before = out.len();
        push_line(&mut out, pos2(inner.min.x, y), vec2(1.0, 0.0), inner.width());
        if out.len() == before && y < inner.min.y {
            break;
        }
        row += 1;
    }
    out
}

impl eframe::App for CaptureApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(s) = &mut self.script {
            s.feed(raw);
            if s.active() {
                ctx.request_repaint();
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        if self.closing {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let screen = ctx.content_rect();
        if std::env::var_os("FASTSHOT_DEBUG").is_some() {
            eprintln!(
                "screen={screen:?} ppp={} scale={} tex={} dirty={}",
                ctx.pixels_per_point(),
                self.scale,
                self.texture.is_some(),
                !matches!(self.dirty, Dirty::Clean)
            );
        }
        // Image pixels per point. The window covers the monitor 1:1.
        let new_scale = self.base.width() as f32 / screen.width().max(1.0);
        if (new_scale - self.scale).abs() > 1e-3 {
            self.scale = new_scale;
            self.rebuild();
        }
        if self.first_frame {
            self.first_frame = false;
            if let Some(s) = self.selection {
                self.selection = Some(clamp_rect(s, self.img_rect));
            }
        }

        let (events, pointer, pressed, released, down, sec_pressed, sec_released, scroll, mods, now, dbl) =
            ctx.input(|i| {
                (
                    i.events.clone(),
                    i.pointer.latest_pos(),
                    i.pointer.primary_pressed(),
                    i.pointer.primary_released(),
                    i.pointer.primary_down(),
                    i.pointer.secondary_pressed(),
                    i.pointer.secondary_released(),
                    crate::ui::wheel_steps(&i.events),
                    i.modifiers,
                    i.time,
                    i.pointer.button_double_clicked(egui::PointerButton::Primary),
                )
            });

        // Canvas: a full screen interactive area below every other widget.
        let canvas_hovered = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let resp = ui.interact(screen, egui::Id::new("canvas"), Sense::click_and_drag());
                let hovered = resp.hovered() || resp.is_pointer_button_down_on();
                self.upload(ctx);
                self.paint_scene(ui.painter(), screen, now, pointer);
                hovered
            })
            .inner;

        // Keyboard.
        let rest = if self.text_edit.is_some() { self.text_input(&events) } else { events };
        // A focused widget in the side panel (slider value, color hex...) owns the keyboard.
        let widget_focused = ctx.egui_wants_keyboard_input();
        if widget_focused {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.memory_mut(|m| m.stop_text_input());
            }
        } else {
            self.handle_keys(rest, now);
        }

        // Mouse.
        if let Some(p) = pointer {
            if let Some(center) = self.wheel_center {
                if sec_released {
                    if let Some(c) = self.wheel_hover(center, p) {
                        self.set_color(c);
                    }
                    self.wheel_center = None;
                }
            } else if sec_pressed && canvas_hovered && matches!(self.drag, Drag::None) {
                self.wheel_center = Some(p);
            } else {
                if pressed && canvas_hovered && matches!(self.drag, Drag::None) {
                    self.on_press(p);
                } else if down && !matches!(self.drag, Drag::None) {
                    self.on_drag(p, mods.shift);
                }
                if released && !matches!(self.drag, Drag::None) {
                    self.on_drag(p, mods.shift);
                    self.on_release();
                }
                if dbl
                    && canvas_hovered
                    && self.cfg.copy_on_double_click
                    && self.tool.is_none()
                    && self.selection.is_some_and(|s| s.contains(self.to_img(p)))
                {
                    self.finish(ExitAction::Copy);
                }
            }
            if canvas_hovered && scroll != 0.0 {
                self.scroll_acc += scroll;
                while self.scroll_acc.abs() >= 1.0 {
                    let d = self.scroll_acc.signum();
                    self.change_size(d, now);
                    self.scroll_acc -= d;
                }
            }
            if canvas_hovered {
                ctx.set_cursor_icon(self.cursor_for(p));
            }
        }

        // Overlays.
        self.buttons_ui(ctx, screen, now);
        self.side_panel_ui(ctx, screen, now);
        if self.show_help && self.selection.is_none() {
            self.paint_help(ctx, screen);
        }
        if let (Some(center), p) = (self.wheel_center, pointer) {
            self.paint_wheel(ctx, center, p);
        }
        let magnify = self.grab_color
            || (self.cfg.show_magnifier
                && matches!(self.drag, Drag::None | Drag::NewSelection { .. } | Drag::Resize { .. })
                && (self.selection.is_none() || !matches!(self.drag, Drag::None)));
        if magnify && let Some(p) = pointer {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, "magnifier".into()));
            self.paint_magnifier(&painter, screen, p);
        }

        // Upload changes made by input handling this frame.
        if !matches!(self.dirty, Dirty::Clean) {
            ctx.request_repaint();
        }
        if self.text_edit.is_some() || now < self.size_feedback_until {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.closing {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
