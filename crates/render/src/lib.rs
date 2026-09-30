//! DrawCraft renderer: document → premultiplied RGBA pixels.
//!
//! The backend is `vello_cpu` (SIMD, sparse strips). Callers give a *view transform* mapping
//! document points to output pixels; the renderer culls by bounds, evaluates appearance stacks
//! (multiple fills/strokes, opacity, blend modes, stroke alignment, dashes), clip groups,
//! gradients, images and text (via `drawcraft-text` glyph outlines).
#![forbid(unsafe_code)]

mod paint;

use std::collections::HashMap;
use std::sync::Arc;

use drawcraft_doc::{AppearanceItem, Document, LineCap, LineJoin, Node, NodeId, NodeKind, StrokeAlign, StrokeLayer, TextObject};
use drawcraft_geom::{Affine, BezPath, FillRule, Rect, Shape};
use vello_cpu::kurbo;
use vello_cpu::peniko::{self, BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

pub use vello_cpu;

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
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { outline: false, background: None, artboards: false, hidden: vec![], dim_templates: true }
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

/// Reusable renderer (keeps the render context, decoded images and glyph caches between frames).
pub struct Renderer {
    ctx: Option<RenderContext>,
    resources: Resources,
    images: HashMap<String, Arc<Pixmap>>,
    /// Statistics of the last frame.
    pub stats: FrameStats,
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
        Self { ctx: None, resources: Resources::new(), images: HashMap::new(), stats: FrameStats::default() }
    }

    /// Render `doc` into a `width`×`height` image using `view` (document → pixel transform).
    pub fn render(&mut self, doc: &Document, width: u32, height: u32, view: Affine, opts: &RenderOptions) -> Rendered {
        let start = now();
        let w = width.clamp(1, u16::MAX as u32) as u16;
        let h = height.clamp(1, u16::MAX as u32) as u16;
        let mut ctx = match self.ctx.take() {
            Some(mut c) if c.width() == w && c.height() == h => {
                c.reset();
                c
            }
            _ => RenderContext::new(w, h),
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
        for layer in &doc.layers {
            self.draw_node(&mut ctx, &frame, layer, false);
        }
        ctx.flush();
        let mut pm = Pixmap::new(w, h);
        ctx.render(&mut pm, &mut self.resources);
        self.ctx = Some(ctx);
        self.stats.micros = now().saturating_sub(start);
        Rendered { width: w as u32, height: h as u32, pixels: pm.data_as_u8_slice().to_vec() }
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
        let b = n.visual_bounds()?;
        let s = (size as f64 - 2.0) / b.width().max(b.height()).max(1e-6);
        let view = Affine::translate((size as f64 / 2.0, size as f64 / 2.0)) * Affine::scale(s) * Affine::translate(-b.center().to_vec2());
        let w = size.clamp(1, u16::MAX as u32) as u16;
        let mut ctx = RenderContext::new(w, w);
        let frame = Frame { doc, view, visible: b.inflate(1.0, 1.0), px: 1.0 / s, opts: &RenderOptions::default() };
        self.draw_node(&mut ctx, &frame, n, true);
        ctx.flush();
        let mut pm = Pixmap::new(w, w);
        ctx.render(&mut pm, &mut self.resources);
        Some(Rendered { width: size, height: size, pixels: pm.data_as_u8_slice().to_vec() })
    }

    fn draw_node(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, force: bool) {
        if !force && (!n.visible || f.opts.hidden.contains(&n.id)) {
            return;
        }
        if let Some(b) = n.visual_bounds() {
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
                    self.draw_node(ctx, f, c, false);
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
                            self.draw_node(ctx, f, c, false);
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
                    let mut art = (*sym.art).clone();
                    art.transform(*xf, false);
                    self.draw_node(ctx, f, &art, true);
                }
            }
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
                    self.draw_stroke(ctx, f, bp, rule, st, bounds);
                }
            }
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
            ctx.stroke_path(bp);
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
        let layout = drawcraft_text::layout(drawcraft_text::FontDb::global(), t);
        let xf = f.view * t.xf;
        if f.opts.outline {
            for g in &layout.glyphs {
                let mut p = g.outline.clone();
                p.apply_affine(xf);
                ctx.set_transform(Affine::IDENTITY);
                ctx.set_stroke(kurbo::Stroke::new(1.0));
                ctx.set_paint(peniko::Color::BLACK);
                ctx.stroke_path(&p);
            }
            return;
        }
        let tb = t.xf.transform_rect_bbox(layout.bounds);
        // Object-level appearance fills/strokes apply on top of character fills (like Illustrator).
        for (i, run) in t.runs.iter().enumerate() {
            let mut path = BezPath::new();
            for g in layout.glyphs.iter().filter(|g| g.run == i) {
                path.extend(g.outline.iter());
            }
            if path.elements().is_empty() {
                continue;
            }
            ctx.set_transform(xf);
            ctx.set_fill_rule(peniko::Fill::NonZero);
            if paint::set_paint(ctx, &run.style.fill, layout.bounds, f.doc) {
                ctx.fill_path(&path);
            }
            if !run.style.stroke.is_none() && run.style.stroke_width > 0.0 && paint::set_paint(ctx, &run.style.stroke, layout.bounds, f.doc) {
                ctx.set_stroke(kurbo::Stroke::new(run.style.stroke_width));
                ctx.stroke_path(&path);
            }
        }
        if !n.appearance.items.is_empty() {
            let mut all = BezPath::new();
            for g in &layout.glyphs {
                all.extend(g.outline.iter());
            }
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
