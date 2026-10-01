//! Live effects in the renderer.
//!
//! Geometry effects (object-level, then per fill/stroke) rewrite the path before painting via
//! `drawcraft-effects`. Raster effects use vello_cpu filter layers (single-threaded contexts only,
//! which is what the renderer creates):
//!
//! - Drop Shadow / Outer Glow: the object's silhouette in a `DropShadowOnly` filter layer, painted
//!   below the object with the effect's blend mode and opacity;
//! - Gaussian Blur: the object painted inside a Gaussian filter layer;
//! - Feather: like Gaussian Blur but clipped to the shape, so the edge fades inward;
//! - Inner Glow: a blurred inverse silhouette (Edge) or the blurred silhouette (Center), clipped
//!   to the shape and painted above it.

use drawcraft_doc::{AppearanceItem, Effect, Node, NodeKind};
use drawcraft_effects::{self as effects, GeomContext, RasterFx};
use drawcraft_geom::{Affine, BezPath, FillRule, Rect, Shape};
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::RenderContext;
use vello_cpu::peniko;

use crate::{Frame, Renderer, blend_mode, fill_rule, paint};

fn visible(effects: &[Effect]) -> bool {
    effects.iter().any(|e| e.visible)
}

/// Does this node need the effects path (a path or compound path with visible effects)?
pub(crate) fn has_fx(n: &Node) -> bool {
    if !matches!(n.kind, NodeKind::Path { guide: false, .. } | NodeKind::Compound { .. }) {
        return false;
    }
    visible(&n.appearance.effects)
        || n.appearance.items.iter().any(|i| match i {
            AppearanceItem::Fill(f) => visible(&f.effects),
            AppearanceItem::Stroke(s) => visible(&s.effects),
        })
}

fn node_bezpath(n: &Node) -> Option<BezPath> {
    match &n.kind {
        NodeKind::Path { path, .. } => Some(path.to_bezpath()),
        NodeKind::Compound { children, .. } => {
            let mut bp = BezPath::new();
            for c in children {
                if let Some(p) = c.path_data() {
                    bp.extend(p.to_bezpath());
                }
            }
            Some(bp)
        }
        _ => None,
    }
}

fn geom_ctx(n: &Node) -> GeomContext {
    let w = n.appearance.stroke_width();
    GeomContext { stroke_width: if w > 0.0 { w } else { 1.0 } }
}

fn apply(effects: &[Effect], bp: &BezPath, ctx: &GeomContext) -> BezPath {
    if !effects::has_geometry(effects) || bp.elements().is_empty() {
        return bp.clone();
    }
    effects::apply_geometry_bez(effects, bp, bp.bounding_box(), ctx)
}

/// The object-level effected geometry of `n` (base path `bp`).
pub(crate) fn effected_path(n: &Node, bp: &BezPath) -> BezPath {
    apply(&n.appearance.effects, bp, &geom_ctx(n))
}

fn item_effects(item: &AppearanceItem) -> &[Effect] {
    match item {
        AppearanceItem::Fill(f) => &f.effects,
        AppearanceItem::Stroke(s) => &s.effects,
    }
}

/// Visual bounds including geometry effects, stroke outsets and shadows/glows.
pub(crate) fn visual_bounds(n: &Node) -> Option<Rect> {
    let bp = node_bezpath(n)?;
    let ctx = geom_ctx(n);
    let g = effected_path(n, &bp);
    let mut r: Option<Rect> = (!g.elements().is_empty()).then(|| g.bounding_box());
    for item in &n.appearance.items {
        let fx = item_effects(item);
        if effects::has_geometry(fx) {
            let ig = apply(fx, &g, &ctx);
            if !ig.elements().is_empty() {
                let b = ig.bounding_box();
                r = Some(r.map_or(b, |r| r.union(b)));
            }
        }
    }
    let o = n.appearance.outset() + effects::outset(&n.appearance.effects);
    r.map(|r| r.inflate(o, o))
}

/// Bounds used for culling: like `Node::visual_bounds`, but aware of live effects anywhere in
/// the subtree (a shadow can reach the view while its object is outside it).
pub(crate) fn cull_bounds(n: &Node) -> Option<Rect> {
    match &n.kind {
        NodeKind::Layer { children, .. } | NodeKind::Group { children, clip: false } => {
            children.iter().fold(None, |acc, c| drawcraft_geom::union_opt(acc, cull_bounds(c)))
        }
        _ if has_fx(n) => visual_bounds(n),
        _ => n.visual_bounds(),
    }
}

fn shadow_filter(dx: f64, dy: f64, blur: f64, color: peniko::Color) -> Filter {
    Filter::from_primitive(FilterPrimitive::DropShadowOnly {
        dx: dx as f32,
        dy: dy as f32,
        std_deviation: (blur / 2.0).max(0.0) as f32,
        color,
        edge_mode: EdgeMode::None,
    })
}

fn blur_filter(sigma: f64) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur { std_deviation: sigma.max(0.0) as f32, edge_mode: EdgeMode::None })
}

fn pcolor(c: &drawcraft_doc::color::Color) -> peniko::Color {
    let [r, g, b] = c.to_rgb();
    peniko::Color::new([r, g, b, 1.0])
}

impl Renderer {
    /// `draw_shape` for nodes with live effects.
    pub(crate) fn draw_shape_fx(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, bp: &BezPath, rule: FillRule) {
        let g = effected_path(n, bp);
        if g.elements().is_empty() {
            return;
        }
        if f.opts.outline {
            self.hairline(ctx, f, &g, [0, 0, 0, 255]);
            return;
        }
        let gctx = geom_ctx(n);
        let rfx = effects::raster_effects(&n.appearance.effects);
        // Below the object: shadows and outer glows.
        for fx in rfx.iter().filter(|x| x.is_below()) {
            // The offset is applied to the geometry rather than in the filter: vello_cpu drops
            // layer content that lies entirely outside the viewport before filtering.
            let (mode, opacity, dx, dy, filter) = match fx {
                RasterFx::DropShadow { mode, opacity, dx, dy, blur, color } => {
                    (*mode, *opacity, *dx, *dy, shadow_filter(0.0, 0.0, *blur, pcolor(color)))
                }
                RasterFx::OuterGlow { mode, opacity, blur, color } => (*mode, *opacity, 0.0, 0.0, shadow_filter(0.0, 0.0, *blur, pcolor(color))),
                _ => continue,
            };
            let mut gs = g.clone();
            gs.apply_affine(Affine::translate((dx, dy)));
            ctx.set_transform(f.view);
            ctx.push_layer(None, Some(blend_mode(mode)), Some(opacity), None, Some(filter));
            self.paint_items(ctx, f, n, &gs, rule, &gctx);
            ctx.pop_layer();
        }
        // The object itself (blurred / feathered).
        let mut layers = 0;
        for fx in &rfx {
            match fx {
                RasterFx::Feather { radius } if *radius > 0.0 => {
                    ctx.set_transform(f.view);
                    ctx.set_fill_rule(fill_rule(rule));
                    ctx.push_clip_layer(&g);
                    ctx.push_layer(None, None, None, None, Some(blur_filter(radius / 2.0)));
                    layers += 2;
                }
                RasterFx::GaussianBlur { radius } if *radius > 0.0 => {
                    ctx.set_transform(f.view);
                    ctx.push_layer(None, None, None, None, Some(blur_filter(radius / 2.0)));
                    layers += 1;
                }
                _ => {}
            }
        }
        self.paint_items(ctx, f, n, &g, rule, &gctx);
        for _ in 0..layers {
            ctx.pop_layer();
        }
        // Above the object: inner glows, clipped to the shape.
        for fx in &rfx {
            let RasterFx::InnerGlow { mode, opacity, blur, color, center } = fx else { continue };
            ctx.set_transform(f.view);
            ctx.set_fill_rule(fill_rule(rule));
            ctx.push_clip_layer(&g);
            ctx.push_layer(None, Some(blend_mode(*mode)), Some(*opacity), None, Some(shadow_filter(0.0, 0.0, *blur, pcolor(color))));
            ctx.set_paint(peniko::Color::BLACK);
            if *center {
                ctx.set_fill_rule(fill_rule(rule));
                ctx.fill_path(&g);
            } else {
                // Everything outside the shape, so the glow bleeds in from the edges.
                let pad = blur * 2.0 + 4.0 * f.px;
                let mut inv = g.bounding_box().inflate(pad, pad).to_path(0.1);
                inv.extend(g.iter());
                ctx.set_fill_rule(peniko::Fill::EvenOdd);
                ctx.fill_path(&inv);
            }
            ctx.pop_layer();
            ctx.pop_layer();
        }
        ctx.set_transform(Affine::IDENTITY);
    }

    /// Paint the fills and strokes of `n` on geometry `g`, applying per-item geometry effects.
    fn paint_items(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, g: &BezPath, rule: FillRule, gctx: &GeomContext) {
        let bounds = g.bounding_box();
        for item in &n.appearance.items {
            let ig = apply(item_effects(item), g, gctx);
            let ib = if effects::has_geometry(item_effects(item)) && !ig.elements().is_empty() { ig.bounding_box() } else { bounds };
            match item {
                AppearanceItem::Fill(fl) => {
                    if !fl.visible || fl.paint.is_none() {
                        continue;
                    }
                    let layered = fl.opacity < 1.0 || fl.blend != drawcraft_doc::color::BlendMode::Normal;
                    if layered {
                        ctx.set_transform(Affine::IDENTITY);
                        ctx.push_layer(None, Some(blend_mode(fl.blend)), Some(fl.opacity), None, None);
                    }
                    ctx.set_transform(f.view);
                    if paint::set_paint(ctx, &fl.paint, ib, f.doc) {
                        ctx.set_fill_rule(fill_rule(rule));
                        ctx.fill_path(&ig);
                    }
                    if layered {
                        ctx.pop_layer();
                    }
                }
                AppearanceItem::Stroke(st) => {
                    if !st.visible || st.paint.is_none() || st.width <= 0.0 {
                        continue;
                    }
                    self.draw_stroke(ctx, f, &ig, rule, st, ib);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use drawcraft_doc::color::{Color, Paint};
    use drawcraft_doc::{Appearance, Document, Effect, Node};
    use drawcraft_geom::{Affine, Rect, shapes};
    use serde_json::json;

    use crate::{RenderOptions, Rendered, Renderer};

    fn doc_with(r: Rect, fill: Color, effects: Vec<(&str, serde_json::Value)>) -> Document {
        let mut d = Document::new(100.0, 100.0);
        let id = d.alloc_id();
        let mut n = Node::path(id, shapes::rectangle(r), Appearance::basic(Paint::solid(fill), Paint::None, 0.0));
        n.appearance.effects = effects.into_iter().map(|(id, p)| Effect { id: id.into(), params: p, visible: true }).collect();
        let l = d.layers[0].id;
        d.insert(Some(l), 0, n).unwrap();
        d
    }

    fn render(d: &Document) -> Rendered {
        Renderer::new().render(d, 100, 100, Affine::IDENTITY, &RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() })
    }

    fn lum(p: [u8; 4]) -> u32 {
        p[0] as u32 + p[1] as u32 + p[2] as u32
    }

    const WHITE: u32 = 765;

    #[test]
    fn thumbnails_of_glowing_objects_render_without_panicking() {
        // Regression: thumbnails used vello's default (multithreaded) context, which panics on filters.
        let d = doc_with(
            Rect::new(20.0, 20.0, 60.0, 60.0),
            Color::BLACK,
            vec![("stylize.outerGlow", json!({"color": "#ff0000", "opacity": 100, "blur": 6}))],
        );
        let id = d.layers[0].children().unwrap()[0].id;
        let t = Renderer::new().render_thumbnail(&d, id, 48).unwrap();
        assert_eq!((t.width, t.height), (48, 48));
    }

    #[test]
    fn per_stroke_raster_effects_use_the_single_threaded_pipeline() {
        let mut d = doc_with(Rect::new(20.0, 20.0, 60.0, 60.0), Color::BLACK, vec![]);
        let l = d.layers[0].id;
        let id = d.layers[0].children().unwrap()[0].id;
        let mut n = (*d.node(id).unwrap()).clone();
        n.appearance = Appearance::basic(Paint::None, Paint::solid(Color::BLACK), 4.0);
        if let Some(drawcraft_doc::AppearanceItem::Stroke(s)) =
            n.appearance.items.iter_mut().find(|i| matches!(i, drawcraft_doc::AppearanceItem::Stroke(_)))
        {
            s.effects =
                vec![Effect { id: "stylize.outerGlow".into(), params: json!({"color": "#ff0000", "opacity": 100, "blur": 6}), visible: true }];
        }
        d.remove(id).unwrap();
        d.insert(Some(l), 0, n).unwrap();
        let mut r = Renderer::new();
        r.threads = 4;
        let _ = r.render(&d, 100, 100, Affine::IDENTITY, &RenderOptions::default());
    }

    #[test]
    fn drop_shadow_darkens_outside_the_shape() {
        let r = Rect::new(20.0, 20.0, 60.0, 60.0);
        let plain = render(&doc_with(r, Color::rgb(1.0, 0.0, 0.0), vec![]));
        assert_eq!(lum(plain.pixel(66, 66)), WHITE);
        let d = doc_with(r, Color::rgb(1.0, 0.0, 0.0), vec![("stylize.dropShadow", json!({"x": 10, "y": 10, "blur": 2, "opacity": 100}))]);
        let img = render(&d);
        assert!(lum(img.pixel(66, 66)) < 200, "shadow below-right: {:?}", img.pixel(66, 66));
        // The object stays on top and unshadowed; the far side has no shadow.
        assert_eq!(img.pixel(40, 40), [255, 0, 0, 255]);
        assert_eq!(lum(img.pixel(15, 15)), WHITE);
    }

    #[test]
    fn shadow_of_offscreen_object_is_not_culled() {
        let d = doc_with(
            Rect::new(-60.0, 20.0, -5.0, 60.0),
            Color::WHITE,
            vec![("stylize.dropShadow", json!({"x": 30, "y": 0, "blur": 0, "opacity": 100}))],
        );
        let img = render(&d);
        let row: Vec<u32> = (0..10).map(|i| lum(img.pixel(i * 10, 40))).collect();
        assert!(lum(img.pixel(10, 40)) < 100, "{row:?}");
    }

    #[test]
    fn geometry_effect_is_rendered() {
        let r = Rect::new(30.0, 30.0, 70.0, 70.0);
        let d = doc_with(r, Color::BLACK, vec![("path.offsetPath", json!({"offset": 10}))]);
        let img = render(&d);
        assert_eq!(lum(img.pixel(25, 50)), 0, "offset grows the fill");
        let d = doc_with(r, Color::BLACK, vec![("distort.transform", json!({"moveH": -25}))]);
        let img = render(&d);
        assert_eq!(lum(img.pixel(10, 50)), 0);
        assert_eq!(lum(img.pixel(60, 50)), WHITE);
    }

    #[test]
    fn per_fill_effects_apply_to_that_fill_only() {
        let mut d = doc_with(Rect::new(30.0, 30.0, 70.0, 70.0), Color::BLACK, vec![]);
        let id = d.layers[0].children().unwrap()[0].id;
        let n = d.node_mut(id).unwrap();
        n.appearance.fill_mut().unwrap().effects.push(Effect { id: "distort.transform".into(), params: json!({"moveH": 20}), visible: true });
        let img = render(&d);
        assert_eq!(lum(img.pixel(85, 50)), 0);
        assert_eq!(lum(img.pixel(40, 50)), WHITE);
    }

    #[test]
    fn blur_glow_and_feather_soften() {
        let r = Rect::new(30.0, 30.0, 70.0, 70.0);
        let blur = render(&doc_with(r, Color::BLACK, vec![("blur.gaussian", json!({"radius": 6}))]));
        let edge = lum(blur.pixel(29, 50));
        assert!(edge > 0 && edge < WHITE, "blur spreads over the edge: {edge}");
        let glow =
            render(&doc_with(r, Color::BLACK, vec![("stylize.outerGlow", json!({"color": "#ff0000", "mode": "normal", "opacity": 100, "blur": 4}))]));
        let g = glow.pixel(27, 50);
        assert!(g[0] > g[1] + 20, "outer glow is red: {g:?}");
        let inner =
            render(&doc_with(r, Color::BLACK, vec![("stylize.innerGlow", json!({"color": "#ffffff", "mode": "normal", "opacity": 100, "blur": 4}))]));
        assert!(lum(inner.pixel(31, 50)) > 60, "inner glow lightens the edge: {:?}", inner.pixel(31, 50));
        assert!(lum(inner.pixel(50, 50)) < 30, "centre stays dark");
        let feather = render(&doc_with(r, Color::BLACK, vec![("stylize.feather", json!({"radius": 8}))]));
        assert!(lum(feather.pixel(31, 50)) > 20, "feather fades the edge inward");
        assert_eq!(lum(feather.pixel(26, 50)), WHITE, "feather stays inside");
    }

    #[test]
    fn hidden_effects_are_ignored_and_outline_mode_uses_effected_geometry() {
        let r = Rect::new(30.0, 30.0, 70.0, 70.0);
        let mut d = doc_with(r, Color::BLACK, vec![("distort.transform", json!({"moveH": -25}))]);
        let id = d.layers[0].children().unwrap()[0].id;
        let opts = RenderOptions { background: Some([255, 255, 255, 255]), outline: true, ..Default::default() };
        let img = Renderer::new().render(&d, 100, 100, Affine::IDENTITY, &opts);
        assert!(lum(img.pixel(5, 50)) < 400, "outline follows the moved path");
        d.node_mut(id).unwrap().appearance.effects[0].visible = false;
        let img = render(&d);
        assert_eq!(lum(img.pixel(50, 50)), 0);
        assert_eq!(lum(img.pixel(10, 50)), WHITE);
    }

    #[test]
    fn visual_bounds_include_shadow() {
        let d = doc_with(Rect::new(0.0, 0.0, 10.0, 10.0), Color::BLACK, vec![("stylize.dropShadow", json!({"x": 7, "y": 7, "blur": 5}))]);
        let n = &d.layers[0].children().unwrap()[0];
        let b = super::visual_bounds(n).unwrap();
        assert!(b.x1 >= 10.0 + 7.0 + 7.5 - 1e-9);
    }
}
