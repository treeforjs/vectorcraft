//! DrawCraft renderer: document → premultiplied RGBA pixels.
//!
//! The backend is `vello_cpu` (SIMD, sparse strips). Callers give a *view transform* mapping
//! document points to output pixels; the renderer culls by bounds, evaluates appearance stacks
//! (multiple fills/strokes, opacity, blend modes, stroke alignment, dashes), clip groups,
//! gradients, images and text (via `drawcraft-text` glyph outlines).
#![forbid(unsafe_code)]

mod brush_fx;
mod fx;
mod live;
mod paint;
mod pattern;
pub mod proof;
mod width;

use std::collections::HashMap;
use std::sync::Arc;

use drawcraft_doc::{AppearanceItem, Document, LineCap, LineJoin, Node, NodeId, NodeKind, StrokeAlign, StrokeLayer, TextObject};
use drawcraft_geom::{Affine, BezPath, FillRule, Rect, Shape};
use vello_cpu::kurbo;
use vello_cpu::peniko::{self, BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

pub use drawcraft_effects as effects;
pub use live::expand_live;
pub use pattern::render_pattern_swatch;
pub use vello_cpu;
pub use width::width_outline;

/// Rendering options.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Outline (wireframe) view: 1 px black paths, no paint.
    pub outline: bool,
    /// Pasteboard colour behind everything (premultiplied RGBA8); `None` = transparent.
    pub background: Option<[u8; 4]>,
    /// Draw artboards as white rectangles (screen view). Export sets this to false.
    pub artboards: bool,
    /// Objects not to draw (e.g. the object being edited by a live drag preview).
    pub hidden: Vec<NodeId>,
    /// Draw template layers dimmed (50%).
    pub dim_templates: bool,
    /// Soft proof / separations preview (see [`proof`]).
    pub proof: Option<proof::ProofSetup>,
    /// Overprint Preview (see [`proof`] for what overprints).
    pub overprint_preview: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { outline: false, background: None, artboards: false, hidden: vec![], dim_templates: true, proof: None, overprint_preview: false }
    }
}

/// A rendered image (premultiplied RGBA8, row-major).
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rendered {
    /// Un-premultiplied RGBA8 copy.
    pub fn to_straight(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
    /// Encode as PNG.
    pub fn to_png(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let img = image::RgbaImage::from_raw(self.width, self.height, self.to_straight()).expect("size");
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).expect("png encode");
        buf
    }
    /// Encode as JPEG (flattened on white) at `quality` 1..=100.
    pub fn to_jpeg(&self, quality: u8) -> Vec<u8> {
        let rgba = self.to_straight();
        let rgb: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                [mix(p[0]), mix(p[1]), mix(p[2])]
            })
            .collect();
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        let _ = image::ImageEncoder::write_image(enc, &rgb, self.width, self.height, image::ExtendedColorType::Rgb8);
        buf
    }
    /// Encode as lossless WebP.
    pub fn to_webp(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
        let _ = image::ImageEncoder::write_image(enc, &self.to_straight(), self.width, self.height, image::ExtendedColorType::Rgba8);
        buf
    }
    /// Straight-alpha RGBA at (x, y).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        let p = &self.pixels[i..i + 4];
        let a = p[3] as u32;
        if a == 0 {
            return [0, 0, 0, 0];
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        [un(p[0]), un(p[1]), un(p[2]), p[3]]
    }
}

/// Cached per-node geometry, keyed by `Arc` identity. Structural sharing means an unchanged node
/// keeps its allocation across edits, so a pointer match (with the Arc kept alive here so the
/// address can't be reused) is an exact cache hit — no invalidation logic needed.
struct GeomEntry {
    node: Arc<Node>,
    bounds: Option<Rect>,
    path: Option<Arc<BezPath>>,
    stamp: u64,
}

/// Reusable renderer (keeps the render context, decoded images and glyph caches between frames).
pub struct Renderer {
    texts: HashMap<usize, (Arc<Node>, Arc<TextGeom>)>,
    /// Single-threaded context used when the document has raster filters (vello's filters require it).
    ctx_st: Option<RenderContext>,
    /// Worker threads for the multithreaded rasterizer (0 = single-threaded).
    pub threads: u16,
    geom: HashMap<usize, GeomEntry>,
    stamp: u64,
    /// Opacity folded into paint alpha for the leaf being drawn (avoids a compositing layer).
    alpha: f32,
    ctx: Option<RenderContext>,
    resources: Resources,
    images: HashMap<String, Arc<Pixmap>>,
    /// Statistics of the last frame.
    pub stats: FrameStats,
    /// Brush art per brushed stroke.
    brushes: brush_fx::BrushCache,
    /// Evaluated blends/envelopes and tessellated meshes.
    live: live::LiveCache,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub drawn: usize,
    pub culled: usize,
    pub micros: u64,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

struct Frame<'a> {
    doc: &'a Document,
    view: Affine,
    /// Visible region in document coordinates (for culling).
    visible: Rect,
    /// Size of one output pixel in document units.
    px: f64,
    opts: &'a RenderOptions,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            texts: HashMap::new(),
            ctx_st: None,
            threads: default_threads(),
            geom: HashMap::new(),
            stamp: 0,
            alpha: 1.0,
            ctx: None,
            resources: Resources::new(),
            images: HashMap::new(),
            stats: FrameStats::default(),
            brushes: Default::default(),
            live: live::LiveCache::default(),
        }
    }

    /// Render `doc` into a `width`×`height` image using `view` (document → pixel transform).
    pub fn render(&mut self, doc: &Document, width: u32, height: u32, view: Affine, opts: &RenderOptions) -> Rendered {
        let start = now();
        let prepared = proof::prepare(doc, opts);
        let doc: &Document = &prepared;
        let w = width.clamp(1, u16::MAX as u32) as u16;
        let h = height.clamp(1, u16::MAX as u32) as u16;
        // Raster filters (drop shadow, glows, blur) need the single-threaded pipeline.
        let mut has_filters = false;
        doc.walk(|n| {
            if !has_filters && node_has_raster_fx(n) {
                has_filters = true;
            }
        });
        let threads = if has_filters { 0 } else { self.threads };
        let slot = if threads == 0 { self.ctx_st.take() } else { self.ctx.take() };
        let mut ctx = match slot {
            Some(mut c) if c.width() == w && c.height() == h => {
                c.reset();
                c
            }
            _ => RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: threads, ..Default::default() }),
        };
        self.stats = FrameStats::default();
        let inv = view.inverse();
        let visible = inv.transform_rect_bbox(Rect::new(0.0, 0.0, w as f64, h as f64));
        let px = 1.0 / view.determinant().abs().sqrt().max(1e-12);
        let frame = Frame { doc, view, visible, px, opts };

        if let Some(bg) = opts.background {
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_paint(peniko::Color::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
            ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
        }
        if opts.artboards && !opts.outline {
            ctx.set_transform(view);
            ctx.set_paint(peniko::Color::WHITE);
            for ab in &doc.artboards {
                ctx.fill_rect(&ab.rect);
            }
        }
        self.stamp += 1;
        if !self.draw_pattern_edit(&mut ctx, &frame) {
            for layer in &doc.layers {
                self.draw_arc(&mut ctx, &frame, layer);
            }
        }
        // Drop cache entries not seen for a few frames.
        let g = self.stamp;
        if self.geom.len() > 1024 {
            self.geom.retain(|_, e| g - e.stamp <= 3);
        }
        ctx.flush();
        let mut pm = Pixmap::new(w, h);
        ctx.render(&mut pm, &mut self.resources);
        if threads == 0 {
            self.ctx_st = Some(ctx);
        } else {
            self.ctx = Some(ctx);
        }
        let mut pixels = pm.data_as_u8_slice().to_vec();
        proof::post(&mut pixels, opts);
        self.stats.micros = now().saturating_sub(start);
        Rendered { width: w as u32, height: h as u32, pixels }
    }

    /// Render one artboard (or any document rect) at `scale` pixels per point, transparent or on white.
    pub fn render_region(&mut self, doc: &Document, region: Rect, scale: f64, white: bool) -> Rendered {
        let w = (region.width() * scale).round().max(1.0) as u32;
        let h = (region.height() * scale).round().max(1.0) as u32;
        let view = Affine::scale(scale) * Affine::translate((-region.x0, -region.y0));
        let opts = RenderOptions { background: white.then_some([255, 255, 255, 255]), ..Default::default() };
        self.render(doc, w, h, view, &opts)
    }

    /// Render a single node (thumbnails, previews) fitted into `size`×`size` pixels.
    pub fn render_thumbnail(&mut self, doc: &Document, id: NodeId, size: u32) -> Option<Rendered> {
        let n = doc.node(id)?;
        let b = brush_fx::cull_bounds(n)?;
        let s = (size as f64 - 2.0) / b.width().max(b.height()).max(1e-6);
        let view = Affine::translate((size as f64 / 2.0, size as f64 / 2.0)) * Affine::scale(s) * Affine::translate(-b.center().to_vec2());
        let w = size.clamp(1, u16::MAX as u32) as u16;
        let mut ctx = single_threaded_context(w, w);
        let frame = Frame { doc, view, visible: b.inflate(1.0, 1.0), px: 1.0 / s, opts: &RenderOptions::default() };
        self.draw_node(&mut ctx, &frame, n, true);
        ctx.flush();
        let mut pm = Pixmap::new(w, w);
        ctx.render(&mut pm, &mut self.resources);
        Some(Rendered { width: size, height: size, pixels: pm.data_as_u8_slice().to_vec() })
    }

    /// Cached cull bounds of a node (containers union their cached children).
    fn bounds_of(&mut self, a: &Arc<Node>) -> Option<Rect> {
        let key = Arc::as_ptr(a) as usize;
        if let Some(e) = self.geom.get_mut(&key)
            && Arc::ptr_eq(&e.node, a)
        {
            e.stamp = self.stamp;
            return e.bounds;
        }
        let b = match &a.kind {
            NodeKind::Layer { children, .. } | NodeKind::Group { children, clip: false } if !fx::has_fx(a) => {
                let mut acc: Option<Rect> = None;
                for c in children {
                    if c.visible {
                        acc = drawcraft_geom::union_opt(acc, self.bounds_of(c));
                    }
                }
                acc
            }
            _ => brush_fx::cull_bounds(a),
        };
        self.geom.insert(key, GeomEntry { node: a.clone(), bounds: b, path: None, stamp: self.stamp });
        b
    }

    /// Cached BezPath of a path node.
    fn path_of(&mut self, a: &Arc<Node>) -> Option<Arc<BezPath>> {
        let key = Arc::as_ptr(a) as usize;
        if let Some(e) = self.geom.get(&key)
            && Arc::ptr_eq(&e.node, a)
            && let Some(p) = &e.path
        {
            return Some(p.clone());
        }
        let p = Arc::new(a.path_data()?.to_bezpath());
        let bounds = self.bounds_of(a);
        self.geom.insert(key, GeomEntry { node: a.clone(), bounds, path: Some(p.clone()), stamp: self.stamp });
        Some(p)
    }

    fn draw_arc(&mut self, ctx: &mut RenderContext, f: &Frame, a: &Arc<Node>) {
        if !a.visible || f.opts.hidden.contains(&a.id) {
            return;
        }
        match self.bounds_of(a) {
            Some(b) => {
                let pad = f.px * 2.0;
                if !rects_overlap(b.inflate(pad, pad), f.visible) {
                    self.stats.culled += 1;
                    return;
                }
                // Level of detail: leaves smaller than a quarter pixel are invisible.
                if !a.is_container() && b.width() < f.px * 0.25 && b.height() < f.px * 0.25 {
                    self.stats.culled += 1;
                    return;
                }
            }
            None if !a.is_container() => return,
            None => {}
        }
        // Fast path for plain paths: cached geometry, opacity folded into the paint.
        if let NodeKind::Path { rule, guide: false, .. } = &a.kind
            && !f.opts.outline
            && a.blend == drawcraft_color::BlendMode::Normal
            && !a.isolate
            && !fx::has_fx(a)
            && (a.opacity >= 1.0 || painted_items(a) == 1)
            && let Some(bp) = self.path_of(a)
        {
            self.alpha = a.opacity.clamp(0.0, 1.0);
            self.draw_shape(ctx, f, a, &bp, *rule);
            self.alpha = 1.0;
            self.stats.drawn += 1;
            return;
        }
        if let NodeKind::Text(t) = &a.kind
            && a.opacity >= 1.0
            && a.blend == drawcraft_color::BlendMode::Normal
            && !fx::has_fx(a)
        {
            let g = self.text_geom_of(a, t);
            self.draw_text_geom(ctx, f, a, t, &g);
            self.stats.drawn += 1;
            return;
        }
        if drawcraft_doc::live::is_live(a) {
            return self.draw_live(ctx, f, a);
        }
        self.draw_node(ctx, f, a, true);
    }

    fn draw_node(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, force: bool) {
        if !force && (!n.visible || f.opts.hidden.contains(&n.id)) {
            return;
        }
        if force {
            // Bounds were already checked by draw_arc (or the caller wants it drawn regardless).
        } else if let Some(b) = fx::cull_bounds(n) {
            let pad = f.px * 2.0;
            if !rects_overlap(b.inflate(pad, pad), f.visible) {
                self.stats.culled += n.count();
                return;
            }
        } else if !n.is_container() {
            return;
        }
        let outline = f.opts.outline;
        let mut layers = 0;
        let template_dim = matches!(n.kind, NodeKind::Layer { template: true, .. }) && f.opts.dim_templates;
        let opacity = if template_dim { n.opacity * 0.5 } else { n.opacity };
        if !outline && (opacity < 1.0 || n.blend != drawcraft_color::BlendMode::Normal || n.isolate) {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(blend_mode(n.blend)), Some(opacity), None, None);
            layers += 1;
        }
        match &n.kind {
            NodeKind::Layer { children, .. } | NodeKind::Group { children, clip: false } => {
                for c in children {
                    self.draw_arc(ctx, f, c);
                }
            }
            NodeKind::Group { children, clip: true } => {
                let clip = children.first().and_then(|c| c.path_data()).map(|p| p.to_bezpath());
                match (clip, outline) {
                    (Some(clip), false) => {
                        ctx.set_transform(f.view);
                        ctx.set_fill_rule(peniko::Fill::NonZero);
                        ctx.push_clip_layer(&clip);
                        for c in children.iter().skip(1) {
                            self.draw_arc(ctx, f, c);
                        }
                        ctx.pop_layer();
                    }
                    _ => {
                        for c in children {
                            self.draw_node(ctx, f, c, false);
                        }
                    }
                }
            }
            NodeKind::Path { path, rule, guide, .. } => {
                let bp = path.to_bezpath();
                if *guide {
                    self.hairline(ctx, f, &bp, [0x4a, 0xd8, 0xff, 255]);
                } else {
                    self.draw_shape(ctx, f, n, &bp, *rule);
                }
            }
            NodeKind::Compound { children, rule } => {
                let mut bp = BezPath::new();
                for c in children {
                    if let Some(p) = c.path_data() {
                        bp.extend(p.to_bezpath());
                    }
                }
                self.draw_shape(ctx, f, n, &bp, *rule);
            }
            NodeKind::Text(t) => self.draw_text(ctx, f, n, t),
            NodeKind::Image(im) => self.draw_image(ctx, f, im),
            NodeKind::SymbolInstance { symbol, xf } => {
                if let Some(sym) = f.doc.symbols.iter().find(|s| &s.name == symbol) {
                    let mut art = brush_fx::instance_art(&sym.art, n);
                    art.transform(*xf, false);
                    self.draw_node(ctx, f, &art, true);
                }
            }
            NodeKind::Blend { .. } | NodeKind::Envelope { .. } | NodeKind::Mesh(_) | NodeKind::Repeat(_) => self.draw_live_node(ctx, f, n),
        }
        self.stats.drawn += 1;
        for _ in 0..layers {
            ctx.pop_layer();
        }
    }

    fn hairline(&mut self, ctx: &mut RenderContext, f: &Frame, bp: &BezPath, rgba: [u8; 4]) {
        ctx.set_transform(Affine::IDENTITY);
        let mut screen = bp.clone();
        screen.apply_affine(f.view);
        ctx.set_stroke(kurbo::Stroke::new(1.0));
        ctx.set_paint(peniko::Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]));
        ctx.stroke_path(&screen);
    }

    fn draw_shape(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, bp: &BezPath, rule: FillRule) {
        if fx::has_fx(n) {
            return self.draw_shape_fx(ctx, f, n, bp, rule);
        }
        if f.opts.outline {
            self.hairline(ctx, f, bp, [0, 0, 0, 255]);
            return;
        }
        let bounds = bp.bounding_box();
        for item in &n.appearance.items {
            match item {
                AppearanceItem::Fill(fl) => {
                    if !fl.visible || fl.paint.is_none() {
                        continue;
                    }
                    let layered = fl.opacity < 1.0 || fl.blend != drawcraft_color::BlendMode::Normal;
                    if layered {
                        ctx.set_transform(Affine::IDENTITY);
                        ctx.push_layer(None, Some(blend_mode(fl.blend)), Some(fl.opacity), None, None);
                    }
                    ctx.set_transform(f.view);
                    if paint::set_paint(ctx, &fl.paint, bounds, f.doc) {
                        self.fold_alpha(ctx, &fl.paint);
                        ctx.set_fill_rule(fill_rule(rule));
                        ctx.fill_path(bp);
                    }
                    if layered {
                        ctx.pop_layer();
                    }
                }
                AppearanceItem::Stroke(st) => {
                    if !st.visible || st.paint.is_none() || st.width <= 0.0 {
                        continue;
                    }
                    if st.brush.is_some() && self.draw_brush(ctx, f, n, bp, st) {
                        continue;
                    }
                    self.draw_stroke(ctx, f, bp, rule, st, bounds);
                }
            }
        }
    }

    /// Multiply the folded object opacity into a solid paint.
    fn fold_alpha(&self, ctx: &mut RenderContext, p: &drawcraft_color::Paint) {
        if self.alpha < 1.0
            && let drawcraft_color::Paint::Solid { color, .. } = p
        {
            let [r, g, b, a] = color.to_rgba8(self.alpha);
            ctx.set_paint(peniko::Color::from_rgba8(r, g, b, a));
        }
    }

    fn draw_stroke(&mut self, ctx: &mut RenderContext, f: &Frame, bp: &BezPath, rule: FillRule, st: &StrokeLayer, bounds: Rect) {
        let layered = st.opacity < 1.0 || st.blend != drawcraft_color::BlendMode::Normal || st.align == StrokeAlign::Outside;
        if layered {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(blend_mode(st.blend)), Some(st.opacity), None, None);
        }
        ctx.set_transform(f.view);
        let closed = bp.elements().last().is_some_and(|e| matches!(e, kurbo::PathEl::ClosePath));
        let width = match st.align {
            StrokeAlign::Center => st.width,
            _ if closed => st.width * 2.0,
            _ => st.width,
        };
        // Keep hairlines visible when zoomed far out (Illustrator shows at least ~1 device pixel).
        let width = width.max(f.px * 0.5);
        let mut stroke = kurbo::Stroke::new(width)
            .with_join(match st.join {
                LineJoin::Miter => kurbo::Join::Miter,
                LineJoin::Round => kurbo::Join::Round,
                LineJoin::Bevel => kurbo::Join::Bevel,
            })
            .with_caps(match st.cap {
                LineCap::Butt => kurbo::Cap::Butt,
                LineCap::Round => kurbo::Cap::Round,
                LineCap::Square => kurbo::Cap::Square,
            })
            .with_miter_limit(st.miter_limit);
        if let Some(d) = &st.dash
            && d.pattern.iter().any(|v| *v > 0.0)
        {
            let mut pat = d.pattern.clone();
            if pat.len() % 2 == 1 {
                pat.extend(pat.clone());
            }
            stroke = stroke.with_dashes(d.offset, pat);
        }
        ctx.set_stroke(stroke);
        let inside = st.align == StrokeAlign::Inside && closed;
        if inside {
            ctx.set_fill_rule(fill_rule(rule));
            ctx.push_clip_layer(bp);
        }
        if paint::set_paint(ctx, &st.paint, bounds.inflate(st.width / 2.0, st.width / 2.0), f.doc) {
            self.fold_alpha(ctx, &st.paint);
            if let Some(o) = width::outline_for(bp, st, f.px * 0.25) {
                ctx.set_fill_rule(peniko::Fill::NonZero);
                ctx.fill_path(&o);
            } else {
                ctx.stroke_path(bp);
            }
        }
        if inside {
            ctx.pop_layer();
        }
        if st.align == StrokeAlign::Outside && closed {
            // Punch out the interior.
            ctx.set_blend_mode(BlendMode::new(Mix::Normal, Compose::DestOut));
            ctx.set_paint(peniko::Color::BLACK);
            ctx.set_fill_rule(fill_rule(rule));
            ctx.fill_path(bp);
            ctx.set_blend_mode(BlendMode::default());
        }
        // Arrowheads.
        if st.start_arrow.is_some() || st.end_arrow.is_some() {
            for (arrow, head) in paint::arrowheads(bp, st) {
                let _ = arrow;
                if paint::set_paint(ctx, &st.paint, bounds, f.doc) {
                    ctx.set_fill_rule(peniko::Fill::NonZero);
                    ctx.fill_path(&head);
                }
            }
        }
        if layered {
            ctx.pop_layer();
        }
    }

    fn draw_text(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, t: &TextObject) {
        let g = text_geom(t);
        self.draw_text_geom(ctx, f, n, t, &g);
    }

    /// Cached glyph geometry for a text node (keyed by Arc identity like paths).
    fn text_geom_of(&mut self, a: &Arc<Node>, t: &TextObject) -> Arc<TextGeom> {
        let key = Arc::as_ptr(a) as usize;
        if let Some((node, g)) = self.texts.get(&key)
            && Arc::ptr_eq(node, a)
        {
            return g.clone();
        }
        let g = Arc::new(text_geom(t));
        if self.texts.len() > 4096 {
            self.texts.clear();
        }
        self.texts.insert(key, (a.clone(), g.clone()));
        g
    }

    fn draw_text_geom(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, t: &TextObject, g: &TextGeom) {
        let xf = f.view * t.xf;
        if f.opts.outline {
            let mut p = g.all.clone();
            p.apply_affine(xf);
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_stroke(kurbo::Stroke::new(1.0));
            ctx.set_paint(peniko::Color::BLACK);
            ctx.stroke_path(&p);
            return;
        }
        let tb = t.xf.transform_rect_bbox(g.bounds);
        // Object-level appearance fills/strokes apply on top of character fills (like Illustrator).
        for (i, run) in t.runs.iter().enumerate() {
            let Some(path) = g.runs.get(i) else { continue };
            if path.elements().is_empty() {
                continue;
            }
            ctx.set_transform(xf);
            ctx.set_fill_rule(peniko::Fill::NonZero);
            if paint::set_paint(ctx, &run.style.fill, g.bounds, f.doc) {
                self.fold_alpha(ctx, &run.style.fill);
                ctx.fill_path(path);
            }
            if !run.style.stroke.is_none() && run.style.stroke_width > 0.0 && paint::set_paint(ctx, &run.style.stroke, g.bounds, f.doc) {
                ctx.set_stroke(kurbo::Stroke::new(run.style.stroke_width));
                ctx.stroke_path(path);
            }
        }
        if !n.appearance.items.is_empty() {
            let mut all = g.all.clone();
            all.apply_affine(t.xf);
            for item in &n.appearance.items {
                ctx.set_transform(f.view);
                match item {
                    AppearanceItem::Fill(fl) if fl.visible && paint::set_paint(ctx, &fl.paint, tb, f.doc) => {
                        ctx.set_fill_rule(peniko::Fill::NonZero);
                        ctx.fill_path(&all);
                    }
                    AppearanceItem::Stroke(st) if st.visible && st.width > 0.0 && paint::set_paint(ctx, &st.paint, tb, f.doc) => {
                        ctx.set_stroke(kurbo::Stroke::new(st.width));
                        ctx.stroke_path(&all);
                    }
                    _ => {}
                }
            }
        }
    }

    fn draw_image(&mut self, ctx: &mut RenderContext, f: &Frame, im: &drawcraft_doc::ImageObject) {
        let rect = Rect::new(0.0, 0.0, im.width as f64, im.height as f64);
        if f.opts.outline {
            let mut p = rect.to_path(0.1);
            p.apply_affine(im.xf);
            self.hairline(ctx, f, &p, [0, 0, 0, 255]);
            return;
        }
        let pm = match self.images.get(&im.key) {
            Some(p) => p.clone(),
            None => {
                let Some(blob) = f.doc.images.get(&im.key) else { return };
                let Some(pm) = paint::decode_pixmap(&blob.bytes) else { return };
                let pm = Arc::new(pm);
                self.images.insert(im.key.clone(), pm.clone());
                pm
            }
        };
        let sx = im.width as f64 / pm.width().max(1) as f64;
        let sy = im.height as f64 / pm.height().max(1) as f64;
        ctx.set_transform(f.view * im.xf);
        ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(pm), sampler: peniko::ImageSampler::default() });
        ctx.set_paint_transform(Affine::scale_non_uniform(sx, sy));
        ctx.fill_rect(&rect);
        ctx.reset_paint_transform();
    }
}

/// Number of visible painted fill/stroke items (opacity folding is exact only for one).
/// A render context on the calling thread. vello_cpu's `RenderContext::new` defaults to a
/// multithreaded dispatcher, which panics on filter effects (glows, shadows, blur).
pub(crate) fn single_threaded_context(w: u16, h: u16) -> RenderContext {
    RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() })
}

/// Whether drawing `n` itself may push a raster filter (node-level or per fill/stroke effects).
fn node_has_raster_fx(n: &Node) -> bool {
    let raster = |e: &[_]| !e.is_empty() && !drawcraft_effects::raster_effects(e).is_empty();
    raster(&n.appearance.effects)
        || n.appearance.items.iter().any(|i| match i {
            AppearanceItem::Fill(f) => raster(&f.effects),
            AppearanceItem::Stroke(s) => raster(&s.effects),
        })
}

fn painted_items(n: &Node) -> usize {
    n.appearance
        .items
        .iter()
        .filter(|i| match i {
            AppearanceItem::Fill(f) => f.visible && !f.paint.is_none() && matches!(f.paint, drawcraft_color::Paint::Solid { .. }),
            AppearanceItem::Stroke(s) => s.visible && !s.paint.is_none() && s.width > 0.0 && matches!(s.paint, drawcraft_color::Paint::Solid { .. }) && s.dash.is_none(),
        })
        .count()
        .max(if n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Fill(f) if f.visible && !matches!(f.paint, drawcraft_color::Paint::Solid { .. } | drawcraft_color::Paint::None)) || matches!(i, AppearanceItem::Stroke(s) if s.visible && !matches!(s.paint, drawcraft_color::Paint::Solid { .. } | drawcraft_color::Paint::None))) { 2 } else { 0 })
}

/// Preferred rasterizer thread count set by the app (Preferences → Performance); negative = automatic.
static THREADS_OVERRIDE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Override the worker-thread count used by renderers created from now on (`None` = automatic).
pub fn set_default_threads(n: Option<u16>) {
    THREADS_OVERRIDE.store(n.map_or(-1, i32::from), std::sync::atomic::Ordering::Relaxed);
}

/// Rasterizer worker threads: 0 on wasm; otherwise up to 4 (vello's sweet spot), leaving a core free.
pub fn default_threads() -> u16 {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let o = THREADS_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed);
        if o >= 0 {
            return o.min(64) as u16;
        }
        if std::env::var_os("DRAWCRAFT_RENDER_THREADS").is_some() {
            return std::env::var("DRAWCRAFT_RENDER_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        }
        std::thread::available_parallelism().map(|n| (n.get().saturating_sub(1)).min(4) as u16).unwrap_or(0)
    }
}

/// Glyph outlines grouped by run, plus the whole text as one path.
struct TextGeom {
    runs: Vec<BezPath>,
    all: BezPath,
    bounds: Rect,
}

fn text_geom(t: &TextObject) -> TextGeom {
    let layout = drawcraft_text::layout(drawcraft_text::FontDb::global(), t);
    let mut runs = vec![BezPath::new(); t.runs.len()];
    let mut all = BezPath::new();
    for g in &layout.glyphs {
        if let Some(r) = runs.get_mut(g.run) {
            r.extend(g.outline.iter());
        }
        all.extend(g.outline.iter());
    }
    TextGeom { runs, all, bounds: layout.bounds }
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

fn fill_rule(r: FillRule) -> peniko::Fill {
    match r {
        FillRule::NonZero => peniko::Fill::NonZero,
        FillRule::EvenOdd => peniko::Fill::EvenOdd,
    }
}

pub(crate) fn blend_mode(b: drawcraft_color::BlendMode) -> BlendMode {
    use drawcraft_color::BlendMode as B;
    let mix = match b {
        B::Normal => Mix::Normal,
        B::Darken => Mix::Darken,
        B::Multiply => Mix::Multiply,
        B::ColorBurn => Mix::ColorBurn,
        B::Lighten => Mix::Lighten,
        B::Screen => Mix::Screen,
        B::ColorDodge => Mix::ColorDodge,
        B::Overlay => Mix::Overlay,
        B::SoftLight => Mix::SoftLight,
        B::HardLight => Mix::HardLight,
        B::Difference => Mix::Difference,
        B::Exclusion => Mix::Exclusion,
        B::Hue => Mix::Hue,
        B::Saturation => Mix::Saturation,
        B::Color => Mix::Color,
        B::Luminosity => Mix::Luminosity,
    };
    BlendMode::new(mix, Compose::SrcOver)
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}
#[cfg(target_arch = "wasm32")]
fn now() -> u64 {
    0
}

#[cfg(test)]
mod tests;
