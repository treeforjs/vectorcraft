//! The VectorCraft brand mark (original artwork, drawn in code): a warm diagonal-gradient rounded
//! square with a white pen nib — the same design as the app icon rendered by `cargo xtask bundle`.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Painter, Pos2, Rect, Shape, pos2};

const TOP: Color32 = Color32::from_rgb(0xff, 0x9a, 0x3c);
const BOTTOM: Color32 = Color32::from_rgb(0xd4, 0x14, 0x5a);

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Paint the mark into `r` (square).
pub fn paint_mark(p: &Painter, r: Rect) {
    let rad = r.width() * 0.22;
    // Rounded-square outline as a fan so each vertex gets its diagonal-gradient colour.
    let mut outline = vec![];
    for (c, a0) in [
        (pos2(r.max.x - rad, r.min.y + rad), -90.0f32),
        (pos2(r.max.x - rad, r.max.y - rad), 0.0),
        (pos2(r.min.x + rad, r.max.y - rad), 90.0),
        (pos2(r.min.x + rad, r.min.y + rad), 180.0),
    ] {
        for i in 0..=6 {
            let a = (a0 + 90.0 * i as f32 / 6.0).to_radians();
            outline.push(c + egui::vec2(a.cos(), a.sin()) * rad);
        }
    }
    let colour = |q: Pos2| {
        let t = ((q.x - r.min.x) + (q.y - r.min.y)) / (r.width() + r.height());
        lerp(TOP, BOTTOM, t.clamp(0.0, 1.0))
    };
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex { pos: r.center(), uv: WHITE_UV, color: colour(r.center()) });
    for q in &outline {
        mesh.vertices.push(Vertex { pos: *q, uv: WHITE_UV, color: colour(*q) });
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(mesh));
    // Pen nib (same proportions as the app icon, 1024-unit design space).
    let s = r.width() / 1024.0;
    let at = |x: f32, y: f32| pos2(r.min.x + x * s, r.min.y + y * s);
    let nib = vec![at(512.0, 770.0), at(352.0, 540.0), at(417.0, 290.0), at(607.0, 290.0), at(672.0, 540.0)];
    p.add(Shape::convex_polygon(nib, Color32::WHITE, egui::Stroke::NONE));
    p.circle_filled(at(512.0, 490.0), 40.0 * s, colour(at(512.0, 490.0)));
    p.line_segment([at(512.0, 525.0), at(512.0, 765.0)], egui::Stroke::new((18.0 * s).max(1.0), colour(at(512.0, 650.0))));
}
