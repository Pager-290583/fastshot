//! Rasterisation of annotations with tiny-skia (+ ab_glyph for text).
//!
//! The same code paints the live preview and the exported image, so what
//! you see on screen is exactly what ends up in the clipboard.

use std::path::PathBuf;
use std::process::Command;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont};
use egui::{Pos2, Vec2, pos2, vec2};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};

use crate::annotation::{Annotation, Shape, TextStyle, rect_from};
use crate::config::TextAlign;

// ---------------------------------------------------------------- fonts ----

/// Font faces indexed by style: 0 regular, 1 bold, 2 italic, 3 bold italic.
/// Each style has a fallback chain so that e.g. Cyrillic or CJK still renders.
pub struct Fonts {
    faces: [Vec<FontVec>; 4],
}

fn fc_match(pattern: &str) -> Option<PathBuf> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    p.is_file().then_some(p)
}

fn load_font(path: &PathBuf) -> Option<FontVec> {
    let data = std::fs::read(path).ok()?;
    FontVec::try_from_vec(data).ok()
}

impl Fonts {
    pub fn load(family: &str) -> Self {
        let styles = ["", ":weight=bold", ":slant=italic", ":weight=bold:slant=italic"];
        let fallbacks = ["sans-serif", "DejaVu Sans", "Noto Sans", "Noto Sans CJK SC"];
        let mut faces: [Vec<FontVec>; 4] = Default::default();
        for (i, style) in styles.iter().enumerate() {
            let mut seen = Vec::new();
            for fam in std::iter::once(family).chain(fallbacks) {
                if let Some(path) = fc_match(&format!("{fam}{style}"))
                    && !seen.contains(&path)
                {
                    if let Some(mut f) = load_font(&path) {
                        // Variable fonts (Noto VF...) default to the regular instance.
                        if i & 1 == 1 {
                            f.set_variation(b"wght", 700.0);
                        }
                        if i & 2 == 2 {
                            f.set_variation(b"ital", 1.0);
                            f.set_variation(b"slnt", -10.0);
                        }
                        faces[i].push(f);
                    }
                    seen.push(path);
                }
            }
        }
        if faces[0].is_empty() {
            // No fc-match: try well known font files.
            for p in [
                "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/run/host/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
            ] {
                if let Some(f) = load_font(&PathBuf::from(p)) {
                    faces[0].push(f);
                }
            }
        }
        if faces[0].is_empty() {
            // No fonts at all: use the font bundled with egui.
            let bytes = epaint_default_fonts::UBUNTU_LIGHT.to_vec();
            if let Ok(f) = FontVec::try_from_vec(bytes) {
                faces[0].push(f);
            }
        }
        for i in 1..4 {
            if faces[i].is_empty() {
                faces[i] = faces[0]
                    .iter()
                    .map(|f| {
                        let mut f = clone_font(f);
                        if i & 1 == 1 {
                            f.set_variation(b"wght", 700.0);
                        }
                        f
                    })
                    .collect();
            }
        }
        Self { faces }
    }

    fn chain(&self, style: &TextStyle) -> &[FontVec] {
        &self.faces[style.bold as usize + 2 * style.italic as usize]
    }

    fn pick<'a>(chain: &'a [FontVec], c: char) -> &'a FontVec {
        chain
            .iter()
            .find(|f| f.glyph_id(c).0 != 0)
            .unwrap_or(&chain[0])
    }

    /// Lay the text out. Returns per line the caret x positions (len = chars + 1).
    pub fn layout(&self, text: &str, px: f32, style: &TextStyle) -> TextLayout {
        let chain = self.chain(style);
        let scale = PxScale::from(px.max(1.0));
        let main = chain[0].as_scaled(scale);
        let ascent = main.ascent();
        let line_height = main.height() + main.line_gap();
        let mut lines = Vec::new();
        for line in text.split('\n') {
            let mut carets = vec![0.0];
            let mut x = 0.0;
            let mut prev: Option<(ab_glyph::GlyphId, &FontVec)> = None;
            let chars: Vec<char> = line.chars().collect();
            for &c in &chars {
                let font = Self::pick(chain, c);
                let sf = font.as_scaled(scale);
                let id = font.glyph_id(c);
                if let Some((pid, pf)) = prev
                    && std::ptr::eq(pf, font)
                {
                    x += sf.kern(pid, id);
                }
                x += sf.h_advance(id);
                carets.push(x);
                prev = Some((id, font));
            }
            lines.push(LineLayout {
                chars,
                carets,
                width: x,
            });
        }
        let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        TextLayout {
            lines,
            ascent,
            line_height,
            width,
        }
    }

    pub fn measure(&self, text: &str, px: f32, style: &TextStyle) -> Vec2 {
        let l = self.layout(text, px, style);
        vec2(l.width.max(px * 0.3), l.line_height * l.lines.len() as f32)
    }

    /// Paint text with its top-left corner at `pos`.
    pub fn draw(
        &self,
        pix: &mut Pixmap,
        pos: Pos2,
        text: &str,
        px: f32,
        style: &TextStyle,
        color: [u8; 4],
    ) {
        let layout = self.layout(text, px, style);
        let chain = self.chain(style);
        let scale = PxScale::from(px.max(1.0));
        for (li, line) in layout.lines.iter().enumerate() {
            let x0 = pos.x + layout.line_offset_aligned(li, style.align);
            let baseline = pos.y + layout.ascent + li as f32 * layout.line_height;
            for (ci, &c) in line.chars.iter().enumerate() {
                let font = Self::pick(chain, c);
                let glyph = font
                    .glyph_id(c)
                    .with_scale_and_position(scale, ab_glyph::point(x0 + line.carets[ci], baseline));
                if let Some(og) = font.outline_glyph(glyph) {
                    let b = og.px_bounds();
                    og.draw(|gx, gy, cov| {
                        blend_px(
                            pix,
                            b.min.x as i32 + gx as i32,
                            b.min.y as i32 + gy as i32,
                            color,
                            cov,
                        );
                    });
                }
            }
            let thick = (px / 14.0).max(1.0);
            if style.underline && line.width > 0.0 {
                let y = baseline + px * 0.12;
                fill_rect(pix, x0, y, line.width, thick, color);
            }
            if style.strikeout && line.width > 0.0 {
                let y = baseline - layout.ascent * 0.3;
                fill_rect(pix, x0, y, line.width, thick, color);
            }
        }
    }
}

fn clone_font(f: &FontVec) -> FontVec {
    FontVec::try_from_vec(f.as_slice().to_vec()).expect("font already parsed once")
}

pub struct LineLayout {
    pub chars: Vec<char>,
    pub carets: Vec<f32>,
    pub width: f32,
}

pub struct TextLayout {
    pub lines: Vec<LineLayout>,
    pub ascent: f32,
    pub line_height: f32,
    pub width: f32,
}

impl TextLayout {
    pub fn line_offset_aligned(&self, line: usize, align: TextAlign) -> f32 {
        let lw = self.lines[line].width;
        match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (self.width - lw) / 2.0,
            TextAlign::Right => self.width - lw,
        }
    }
}

/// Caret position (top of the line) for a char index in `text`, relative to the
/// text origin. Used by the text editor overlay.
pub fn caret_offset(layout: &TextLayout, align: TextAlign, char_index: usize) -> Vec2 {
    let mut remaining = char_index;
    for (li, line) in layout.lines.iter().enumerate() {
        if remaining <= line.chars.len() {
            let x = layout.line_offset_aligned(li, align) + line.carets[remaining];
            return vec2(x, li as f32 * layout.line_height);
        }
        remaining -= line.chars.len() + 1;
    }
    vec2(0.0, 0.0)
}

// -------------------------------------------------------------- shapes ----

fn sk_color(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}

fn paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(sk_color(c));
    p.anti_alias = true;
    p
}

fn blend_px(pix: &mut Pixmap, x: i32, y: i32, c: [u8; 4], cov: f32) {
    if x < 0 || y < 0 || x >= pix.width() as i32 || y >= pix.height() as i32 {
        return;
    }
    let a = cov.clamp(0.0, 1.0) * c[3] as f32 / 255.0;
    if a <= 0.0 {
        return;
    }
    let i = (y as usize * pix.width() as usize + x as usize) * 4;
    let d = &mut pix.data_mut()[i..i + 4];
    for k in 0..3 {
        d[k] = (c[k] as f32 * a + d[k] as f32 * (1.0 - a)).round() as u8;
    }
    d[3] = (255.0 * a + d[3] as f32 * (1.0 - a)).round() as u8;
}

fn fill_rect(pix: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: [u8; 4]) {
    if let Some(r) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        pix.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

fn stroke(width: f32, cap: LineCap, join: LineJoin) -> Stroke {
    Stroke {
        width,
        line_cap: cap,
        line_join: join,
        ..Stroke::default()
    }
}

fn stroke_line(pix: &mut Pixmap, a: Pos2, b: Pos2, w: f32, c: [u8; 4], cap: LineCap) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    if let Some(path) = pb.finish() {
        pix.stroke_path(&path, &paint(c), &stroke(w, cap, LineJoin::Round), Transform::identity(), None);
    }
}

fn rounded_rect(r: egui::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    if rad < 0.5 {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(
            r.min.x, r.min.y, r.max.x, r.max.y,
        )?));
    }
    let k = 0.552_284_8 * rad;
    let (l, t, rt, b) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let mut pb = PathBuilder::new();
    pb.move_to(l + rad, t);
    pb.line_to(rt - rad, t);
    pb.cubic_to(rt - rad + k, t, rt, t + rad - k, rt, t + rad);
    pb.line_to(rt, b - rad);
    pb.cubic_to(rt, b - rad + k, rt - rad + k, b, rt - rad, b);
    pb.line_to(l + rad, b);
    pb.cubic_to(l + rad - k, b, l, b - rad + k, l, b - rad);
    pb.line_to(l, t + rad);
    pb.cubic_to(l, t + rad - k, l + rad - k, t, l + rad, t);
    pb.close();
    pb.finish()
}

fn clamp_irect(pix: &Pixmap, r: egui::Rect) -> Option<(usize, usize, usize, usize)> {
    let x0 = r.min.x.round().max(0.0) as usize;
    let y0 = r.min.y.round().max(0.0) as usize;
    let x1 = (r.max.x.round().max(0.0) as usize).min(pix.width() as usize);
    let y1 = (r.max.y.round().max(0.0) as usize).min(pix.height() as usize);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

fn pixelate(pix: &mut Pixmap, r: egui::Rect, block: usize) {
    let Some((x0, y0, x1, y1)) = clamp_irect(pix, r) else {
        return;
    };
    let block = block.max(2);
    let w = pix.width() as usize;
    let data = pix.data_mut();
    let mut by = y0;
    while by < y1 {
        let ey = (by + block).min(y1);
        let mut bx = x0;
        while bx < x1 {
            let ex = (bx + block).min(x1);
            let mut sum = [0u32; 4];
            for y in by..ey {
                for x in bx..ex {
                    let i = (y * w + x) * 4;
                    for k in 0..4 {
                        sum[k] += data[i + k] as u32;
                    }
                }
            }
            let n = ((ey - by) * (ex - bx)) as u32;
            let avg = sum.map(|s| (s / n) as u8);
            for y in by..ey {
                for x in bx..ex {
                    let i = (y * w + x) * 4;
                    data[i..i + 4].copy_from_slice(&avg);
                }
            }
            bx = ex;
        }
        by = ey;
    }
}

fn invert(pix: &mut Pixmap, r: egui::Rect) {
    let Some((x0, y0, x1, y1)) = clamp_irect(pix, r) else {
        return;
    };
    let w = pix.width() as usize;
    let data = pix.data_mut();
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 4;
            let a = data[i + 3];
            for k in 0..3 {
                data[i + k] = a.saturating_sub(data[i + k]);
            }
        }
    }
}

pub fn contrast_text_color(c: [u8; 4]) -> [u8; 4] {
    let lum = 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
    if lum > 150.0 {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    }
}

/// Draw one annotation. `counter` is the number shown by counter bubbles.
pub fn draw(pix: &mut Pixmap, ann: &Annotation, counter: usize, scale: f32, fonts: &Fonts) {
    let w = ann.stroke_px(scale);
    let c = ann.color;
    match &ann.shape {
        Shape::Pencil(pts) => {
            if pts.len() == 1 {
                if let Some(path) = PathBuilder::from_circle(pts[0].x, pts[0].y, w / 2.0) {
                    pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
                }
                return;
            }
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].x, pts[0].y);
            for p in &pts[1..] {
                pb.line_to(p.x, p.y);
            }
            if let Some(path) = pb.finish() {
                pix.stroke_path(
                    &path,
                    &paint(c),
                    &stroke(w, LineCap::Round, LineJoin::Round),
                    Transform::identity(),
                    None,
                );
            }
        }
        Shape::Line(a, b) => stroke_line(pix, *a, *b, w, c, LineCap::Round),
        Shape::Marker(a, b) => {
            let mc = [c[0], c[1], c[2], (c[3] as u16 * 120 / 255) as u8];
            stroke_line(pix, *a, *b, w, mc, LineCap::Round);
        }
        Shape::Arrow(a, b) => {
            let d = *b - *a;
            let len = d.length();
            if len < 1.0 {
                return;
            }
            let u = d / len;
            let n = vec2(-u.y, u.x);
            let head = ann.arrow_head(scale).min(len);
            let base = *b - u * head;
            stroke_line(pix, *a, base + u * (head * 0.3), w, c, LineCap::Round);
            let half = head * 0.5;
            let mut pb = PathBuilder::new();
            pb.move_to(b.x, b.y);
            let l = base + n * half;
            let r = base - n * half;
            pb.line_to(l.x, l.y);
            pb.line_to(r.x, r.y);
            pb.close();
            if let Some(path) = pb.finish() {
                pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
            }
        }
        Shape::Selection(a, b) => {
            let r = rect_from(*a, *b);
            if let Some(rr) = tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y) {
                let path = PathBuilder::from_rect(rr);
                pix.stroke_path(
                    &path,
                    &paint(c),
                    &stroke(w, LineCap::Square, LineJoin::Miter),
                    Transform::identity(),
                    None,
                );
            }
        }
        Shape::Rectangle(a, b) => {
            if let Some(path) = rounded_rect(rect_from(*a, *b), ann.size * scale) {
                pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
            }
        }
        Shape::Circle(a, b) => {
            let r = rect_from(*a, *b);
            let mut pb = PathBuilder::new();
            if let Some(rr) = tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y) {
                pb.push_oval(rr);
            }
            if let Some(path) = pb.finish() {
                pix.stroke_path(
                    &path,
                    &paint(c),
                    &stroke(w, LineCap::Round, LineJoin::Round),
                    Transform::identity(),
                    None,
                );
            }
        }
        Shape::Pixelate(a, b) => pixelate(pix, rect_from(*a, *b), (ann.size * scale) as usize),
        Shape::Invert(a, b) => invert(pix, rect_from(*a, *b)),
        Shape::Text { pos, text, style } => {
            fonts.draw(pix, *pos, text, ann.size * scale, style, c);
        }
        Shape::Counter { center, tail } => {
            let r = ann.counter_radius(scale);
            if let Some(t) = tail {
                // Speech-bubble like pointer from the circle to `t`.
                let d = *t - *center;
                let len = d.length();
                if len > r {
                    let u = d / len;
                    let n = vec2(-u.y, u.x);
                    let p1 = *center + n * (r * 0.7);
                    let p2 = *center - n * (r * 0.7);
                    let mut pb = PathBuilder::new();
                    pb.move_to(p1.x, p1.y);
                    pb.line_to(t.x, t.y);
                    pb.line_to(p2.x, p2.y);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
            if let Some(path) = PathBuilder::from_circle(center.x, center.y, r) {
                pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
            }
            let style = TextStyle {
                bold: true,
                italic: false,
                underline: false,
                strikeout: false,
                align: TextAlign::Left,
            };
            let label = counter.to_string();
            let px = r * if label.len() > 2 { 1.0 } else { 1.35 };
            let size = fonts.measure(&label, px, &style);
            let layout = fonts.layout(&label, px, &style);
            // Center on the visual glyph box rather than the line box.
            let cap = layout.ascent * 0.72;
            let top = center.y - cap / 2.0 - (layout.ascent - cap);
            fonts.draw(
                pix,
                pos2(center.x - size.x / 2.0, top),
                &label,
                px,
                &style,
                contrast_text_color(c),
            );
        }
    }
}

/// Counter number of each annotation (1-based index among counters).
pub fn counter_numbers(anns: &[Annotation]) -> Vec<usize> {
    let mut n = 0;
    anns.iter()
        .map(|a| {
            if matches!(a.shape, Shape::Counter { .. }) {
                n += 1;
            }
            n
        })
        .collect()
}

pub fn draw_all(pix: &mut Pixmap, anns: &[Annotation], scale: f32, fonts: &Fonts) {
    let nums = counter_numbers(anns);
    for (a, n) in anns.iter().zip(nums) {
        draw(pix, a, n, scale, fonts);
    }
}

pub fn pixmap_from_rgba(img: &image::RgbaImage) -> Pixmap {
    let size = tiny_skia::IntSize::from_wh(img.width(), img.height()).expect("non-empty image");
    // Screenshots are opaque, so straight == premultiplied.
    Pixmap::from_vec(img.as_raw().clone(), size).expect("valid size")
}

pub fn rgba_from_pixmap(pix: &Pixmap) -> image::RgbaImage {
    image::RgbaImage::from_raw(pix.width(), pix.height(), pix.clone().take_demultiplied())
        .expect("valid size")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, Shape, TextStyle};
    use crate::tools::Tool;

    fn style() -> TextStyle {
        TextStyle { bold: false, italic: false, underline: false, strikeout: false, align: TextAlign::Left }
    }

    /// Renders every tool into a PNG (path from FASTSHOT_TEST_OUT) for visual inspection.
    #[test]
    fn render_all_tools() {
        let mut img = image::RgbaImage::new(900, 600);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let v = if (x / 20 + y / 20) % 2 == 0 { 230 } else { 180 };
            *p = image::Rgba([v, (x % 256) as u8, (y % 256) as u8, 255]);
        }
        let mut pix = pixmap_from_rgba(&img);
        let fonts = Fonts::load("sans-serif");
        let red = [255, 0, 0, 255];
        let blue = [0, 80, 255, 255];
        let p = |x: f32, y: f32| pos2(x, y);
        let mut anns = vec![
            Annotation { shape: Shape::Pencil(vec![p(20., 20.), p(60., 80.), p(100., 30.), p(140., 90.)]), color: red, size: 3. },
            Annotation { shape: Shape::Line(p(170., 20.), p(300., 90.)), color: blue, size: 4. },
            Annotation { shape: Shape::Arrow(p(330., 90.), p(460., 20.)), color: red, size: 3. },
            Annotation { shape: Shape::Selection(p(490., 20.), p(620., 100.)), color: blue, size: 3. },
            Annotation { shape: Shape::Rectangle(p(650., 20.), p(780., 100.)), color: red, size: 10. },
            Annotation { shape: Shape::Circle(p(20., 130.), p(160., 230.)), color: blue, size: 3. },
            Annotation { shape: Shape::Marker(p(190., 180.), p(400., 180.)), color: [255, 255, 0, 255], size: 14. },
            Annotation { shape: Shape::Pixelate(p(430., 120.), p(600., 240.)), color: red, size: 10. },
            Annotation { shape: Shape::Invert(p(630., 120.), p(780., 240.)), color: red, size: 1. },
        ];
        for (i, (c, tail)) in [([255, 0, 0, 255], None), ([255, 255, 0, 255], Some(p(160., 360.))), ([0, 0, 0, 255], None)].into_iter().enumerate() {
            let mut a = Annotation::start(Tool::CircleCount, p(40. + i as f32 * 60., 290.), c, 4., style());
            if let Some(t) = tail { a.drag_to(t, false); }
            anns.push(a);
        }
        anns.push(Annotation {
            shape: Shape::Text { pos: p(250., 270.), text: "Hello, мир! Текст 123\nвторая строка".into(), style: style() },
            color: blue, size: 28.,
        });
        let mut st = style(); st.bold = true; st.italic = true; st.underline = true; st.align = TextAlign::Center;
        anns.push(Annotation {
            shape: Shape::Text { pos: p(250., 400.), text: "Bold Italic\nunderlined centered".into(), style: st },
            color: red, size: 24.,
        });
        let mut st = style(); st.strikeout = true; st.align = TextAlign::Right;
        anns.push(Annotation {
            shape: Shape::Text { pos: p(600., 400.), text: "strike\nright aligned".into(), style: st },
            color: [0, 0, 0, 255], size: 22.,
        });
        draw_all(&mut pix, &anns, 1.0, &fonts);
        for a in &anns {
            assert!(!a.is_empty());
            let b = a.bounds(1.0, &fonts);
            assert!(b.is_positive(), "{:?}", a.tool());
        }
        if let Some(out) = std::env::var_os("FASTSHOT_TEST_OUT") {
            rgba_from_pixmap(&pix).save(out).unwrap();
        }
    }
}
