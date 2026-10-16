//! Drawing objects placed on the capture.
//!
//! All coordinates are in image pixels. `size` is expressed in logical units
//! and multiplied by the HiDPI scale when rendering, so a 3px pencil looks the
//! same on a 1x and a 2x monitor (Flameshot behaves the same way).

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::config::TextAlign;
use crate::render::Fonts;
use crate::tools::Tool;

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub align: TextAlign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Pencil(Vec<Pos2>),
    Line(Pos2, Pos2),
    Arrow(Pos2, Pos2),
    Selection(Pos2, Pos2),
    Rectangle(Pos2, Pos2),
    Circle(Pos2, Pos2),
    Marker(Pos2, Pos2),
    Pixelate(Pos2, Pos2),
    Invert(Pos2, Pos2),
    Text {
        pos: Pos2,
        text: String,
        style: TextStyle,
    },
    Counter {
        center: Pos2,
        tail: Option<Pos2>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub shape: Shape,
    pub color: [u8; 4],
    pub size: f32,
}

pub fn rect_from(a: Pos2, b: Pos2) -> Rect {
    Rect::from_two_pos(a, b)
}

impl Annotation {
    pub fn tool(&self) -> Tool {
        match self.shape {
            Shape::Pencil(_) => Tool::Pencil,
            Shape::Line(..) => Tool::Line,
            Shape::Arrow(..) => Tool::Arrow,
            Shape::Selection(..) => Tool::Selection,
            Shape::Rectangle(..) => Tool::Rectangle,
            Shape::Circle(..) => Tool::Circle,
            Shape::Marker(..) => Tool::Marker,
            Shape::Pixelate(..) => Tool::Pixelate,
            Shape::Invert(..) => Tool::Invert,
            Shape::Text { .. } => Tool::Text,
            Shape::Counter { .. } => Tool::CircleCount,
        }
    }

    /// Start a new object for `tool` at point `p`.
    pub fn start(tool: Tool, p: Pos2, color: [u8; 4], size: f32, style: TextStyle) -> Self {
        let shape = match tool {
            Tool::Pencil => Shape::Pencil(vec![p]),
            Tool::Line => Shape::Line(p, p),
            Tool::Arrow => Shape::Arrow(p, p),
            Tool::Selection => Shape::Selection(p, p),
            Tool::Rectangle => Shape::Rectangle(p, p),
            Tool::Circle => Shape::Circle(p, p),
            Tool::Marker => Shape::Marker(p, p),
            Tool::Pixelate => Shape::Pixelate(p, p),
            Tool::Invert => Shape::Invert(p, p),
            Tool::Text => Shape::Text {
                pos: p,
                text: String::new(),
                style,
            },
            Tool::CircleCount => Shape::Counter {
                center: p,
                tail: None,
            },
        };
        Self { shape, color, size }
    }

    /// Update the object while the mouse is dragged.
    /// `constrain` = Shift held: 45° lines, squares and circles.
    pub fn drag_to(&mut self, mut p: Pos2, constrain: bool) {
        match &mut self.shape {
            Shape::Pencil(pts) => {
                if pts.last().is_none_or(|l| l.distance(p) >= 1.0) {
                    pts.push(p)
                }
            }
            Shape::Line(a, b) | Shape::Arrow(a, b) | Shape::Marker(a, b) => {
                if constrain {
                    p = snap_45(*a, p);
                }
                *b = p;
            }
            Shape::Selection(a, b)
            | Shape::Rectangle(a, b)
            | Shape::Circle(a, b)
            | Shape::Pixelate(a, b)
            | Shape::Invert(a, b) => {
                if constrain {
                    let d = p - *a;
                    let m = d.x.abs().max(d.y.abs());
                    p = *a + vec2(m * d.x.signum(), m * d.y.signum());
                }
                *b = p;
            }
            Shape::Counter { center, tail } => {
                *tail = if center.distance(p) > 4.0 { Some(p) } else { None };
            }
            Shape::Text { .. } => {}
        }
    }

    /// Objects that would draw nothing are dropped instead of committed.
    pub fn is_empty(&self) -> bool {
        match &self.shape {
            Shape::Pencil(pts) => pts.is_empty(),
            Shape::Line(a, b) | Shape::Arrow(a, b) | Shape::Marker(a, b) => a.distance(*b) < 1.0,
            Shape::Selection(a, b)
            | Shape::Rectangle(a, b)
            | Shape::Circle(a, b)
            | Shape::Pixelate(a, b)
            | Shape::Invert(a, b) => (a.x - b.x).abs() < 1.0 || (a.y - b.y).abs() < 1.0,
            Shape::Text { text, .. } => text.trim().is_empty(),
            Shape::Counter { .. } => false,
        }
    }

    pub fn translate(&mut self, d: Vec2) {
        match &mut self.shape {
            Shape::Pencil(pts) => pts.iter_mut().for_each(|p| *p += d),
            Shape::Line(a, b)
            | Shape::Arrow(a, b)
            | Shape::Selection(a, b)
            | Shape::Rectangle(a, b)
            | Shape::Circle(a, b)
            | Shape::Marker(a, b)
            | Shape::Pixelate(a, b)
            | Shape::Invert(a, b) => {
                *a += d;
                *b += d;
            }
            Shape::Text { pos, .. } => *pos += d,
            Shape::Counter { center, tail } => {
                *center += d;
                if let Some(t) = tail {
                    *t += d;
                }
            }
        }
    }

    pub fn stroke_px(&self, scale: f32) -> f32 {
        (self.size * scale).max(1.0)
    }

    pub fn counter_radius(&self, scale: f32) -> f32 {
        (self.size + 8.0) * scale
    }

    pub fn arrow_head(&self, scale: f32) -> f32 {
        (self.size * 3.0 + 10.0) * scale
    }

    /// Bounding box in image pixels, including stroke width.
    pub fn bounds(&self, scale: f32, fonts: &Fonts) -> Rect {
        let w = self.stroke_px(scale);
        match &self.shape {
            Shape::Pencil(pts) => {
                let mut r = Rect::NOTHING;
                for p in pts {
                    r.extend_with(*p);
                }
                r.expand(w)
            }
            Shape::Line(a, b) | Shape::Marker(a, b) | Shape::Selection(a, b) => {
                rect_from(*a, *b).expand(w)
            }
            Shape::Arrow(a, b) => rect_from(*a, *b).expand(w + self.arrow_head(scale)),
            Shape::Circle(a, b) => rect_from(*a, *b).expand(w),
            Shape::Rectangle(a, b) | Shape::Pixelate(a, b) | Shape::Invert(a, b) => {
                rect_from(*a, *b).expand(1.0)
            }
            Shape::Text { pos, text, style } => {
                let size = fonts.measure(text, self.size * scale, style);
                Rect::from_min_size(*pos, size).expand(2.0)
            }
            Shape::Counter { center, tail } => {
                let r = self.counter_radius(scale);
                let mut rect = Rect::from_center_size(*center, Vec2::splat(r * 2.0));
                if let Some(t) = tail {
                    rect.extend_with(*t);
                }
                rect.expand(2.0)
            }
        }
    }

    /// Hit test used to pick objects with the mouse.
    pub fn hit(&self, p: Pos2, scale: f32, fonts: &Fonts) -> bool {
        let tol = self.stroke_px(scale) / 2.0 + 5.0 * scale;
        match &self.shape {
            Shape::Pencil(pts) => {
                if pts.len() == 1 {
                    return pts[0].distance(p) <= tol;
                }
                pts.windows(2).any(|s| dist_segment(p, s[0], s[1]) <= tol)
            }
            Shape::Line(a, b) | Shape::Arrow(a, b) | Shape::Marker(a, b) => {
                dist_segment(p, *a, *b) <= tol
            }
            Shape::Selection(a, b) => {
                let r = rect_from(*a, *b);
                r.expand(tol).contains(p) && !r.shrink(tol).contains(p)
            }
            Shape::Circle(a, b) => {
                let r = rect_from(*a, *b);
                let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
                if rx < 1.0 || ry < 1.0 {
                    return r.expand(tol).contains(p);
                }
                let d = p - r.center();
                let n = ((d.x / rx).powi(2) + (d.y / ry).powi(2)).sqrt();
                (n - 1.0).abs() * rx.min(ry) <= tol
            }
            Shape::Rectangle(..) | Shape::Pixelate(..) | Shape::Invert(..) | Shape::Text { .. } => {
                self.bounds(scale, fonts).contains(p)
            }
            Shape::Counter { center, .. } => center.distance(p) <= self.counter_radius(scale),
        }
    }
}

pub fn snap_45(a: Pos2, p: Pos2) -> Pos2 {
    let d = p - a;
    let len = d.length();
    let ang = d.y.atan2(d.x);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (ang / step).round() * step;
    pos2(a.x + len * snapped.cos(), a.y + len * snapped.sin())
}

pub fn dist_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}
