//! Application icon drawn at runtime (tray pixmap / window icon).

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

pub fn render(size: u32) -> Pixmap {
    let mut pix = Pixmap::new(size, size).expect("icon size");
    let s = size as f32;
    let mut paint = Paint::default();
    paint.anti_alias = true;

    // Purple rounded square.
    paint.set_color(Color::from_rgba8(0x74, 0x00, 0x96, 255));
    let r = s * 0.2;
    let mut pb = PathBuilder::new();
    let (l, t, rt, b) = (s * 0.04, s * 0.04, s * 0.96, s * 0.96);
    pb.move_to(l + r, t);
    pb.line_to(rt - r, t);
    pb.quad_to(rt, t, rt, t + r);
    pb.line_to(rt, b - r);
    pb.quad_to(rt, b, rt - r, b);
    pb.line_to(l + r, b);
    pb.quad_to(l, b, l, b - r);
    pb.line_to(l, t + r);
    pb.quad_to(l, t, l + r, t);
    pb.close();
    if let Some(p) = pb.finish() {
        pix.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }

    // White selection corners.
    paint.set_color(Color::from_rgba8(255, 255, 255, 255));
    let w = s * 0.09;
    let (a, z, len) = (s * 0.2, s * 0.8, s * 0.22);
    for (x, y, dx, dy) in [(a, a, 1.0, 1.0), (z, a, -1.0, 1.0), (a, z, 1.0, -1.0), (z, z, -1.0, -1.0)] {
        let rects = [
            tiny_skia::Rect::from_xywh(x.min(x + dx * len), y.min(y + dy * w), len, w),
            tiny_skia::Rect::from_xywh(x.min(x + dx * w), y.min(y + dy * len), w, len),
        ];
        for rr in rects.into_iter().flatten() {
            pix.fill_rect(rr, &paint, Transform::identity(), None);
        }
    }
    // Red dot (the "shot").
    paint.set_color(Color::from_rgba8(0xff, 0x45, 0x45, 255));
    if let Some(p) = PathBuilder::from_circle(s * 0.5, s * 0.5, s * 0.11) {
        pix.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    pix
}

pub fn tray_icon(size: u32) -> ksni::Icon {
    let pix = render(size);
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for px in pix.pixels() {
        let c = px.demultiply();
        data.extend_from_slice(&[c.alpha(), c.red(), c.green(), c.blue()]);
    }
    ksni::Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

pub fn icon_data(size: u32) -> std::sync::Arc<egui::IconData> {
    let pix = render(size);
    std::sync::Arc::new(egui::IconData {
        rgba: pix.clone().take_demultiplied(),
        width: size,
        height: size,
    })
}
