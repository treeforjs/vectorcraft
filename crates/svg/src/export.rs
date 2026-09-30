//! The SVG writer.

use std::collections::{HashMap, HashSet};

use drawcraft_color::{BlendMode, GradientKind, GradientPaint, Paint};
use drawcraft_doc::{AppearanceItem, Document, FillLayer, LineCap, LineJoin, Node, NodeId, NodeKind, StrokeAlign, StrokeLayer, TextKind, TextObject};
use drawcraft_doc::{CharStyle, Justify};
use drawcraft_geom::{Affine, FillRule, PathData, Point, Rect, Vec2};

use crate::{ExportOptions, Styling, base64_encode, fmt_num, xml_escape};

type Props = Vec<(&'static str, String)>;

pub(crate) fn export(doc: &Document, opts: &ExportOptions) -> String {
    let rect = opts
        .artboard
        .and_then(|i| doc.artboards.get(i))
        .map(|a| a.rect)
        .or_else(|| doc.art_bounds())
        .or_else(|| doc.artboards.first().map(|a| a.rect))
        .unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
    let mut w = Writer {
        doc,
        opts,
        xf: Affine::translate((-rect.x0, -rect.y0)),
        body: String::new(),
        defs: String::new(),
        classes: Vec::new(),
        used_ids: HashSet::new(),
        names: HashMap::new(),
        depth: 1,
    };
    w.assign_name_ids();
    for l in &doc.layers {
        w.node(l);
    }

    let nl = if opts.minify { "" } else { "\n" };
    let mut out = String::new();
    if !opts.minify {
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    }
    let (ww, hh) = (w.num(rect.width()), w.num(rect.height()));
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
    if !opts.responsive {
        out.push_str(&format!(" width=\"{ww}\" height=\"{hh}\""));
    }
    out.push_str(&format!(" viewBox=\"0 0 {ww} {hh}\">{nl}"));
    if !w.defs.is_empty() || !w.classes.is_empty() {
        out.push_str(&w.indent(1));
        out.push_str(&format!("<defs>{nl}"));
        if !w.classes.is_empty() {
            out.push_str(&w.indent(2));
            out.push_str("<style>");
            for (i, c) in w.classes.iter().enumerate() {
                out.push_str(&format!("{nl}{}.cls-{}{{{}}}", w.indent(3), i + 1, xml_escape(c)));
            }
            out.push_str(&format!("{nl}{}</style>{nl}", w.indent(2)));
        }
        out.push_str(&w.defs);
        out.push_str(&w.indent(1));
        out.push_str(&format!("</defs>{nl}"));
    }
    if !doc.title.is_empty() {
        out.push_str(&w.indent(1));
        out.push_str(&format!("<title>{}</title>{nl}", xml_escape(&doc.title)));
    }
    out.push_str(&w.body);
    out.push_str("</svg>");
    out.push_str(nl);
    out
}

struct Writer<'a> {
    doc: &'a Document,
    opts: &'a ExportOptions,
    /// Document → SVG user space (artboard origin translation, symbol instance transforms).
    xf: Affine,
    body: String,
    defs: String,
    /// Unique CSS declaration blocks; class name is `cls-{index + 1}`.
    classes: Vec<String>,
    used_ids: HashSet<String>,
    names: HashMap<NodeId, String>,
    depth: usize,
}

pub(crate) fn blend_css(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Normal => "normal",
        BlendMode::Darken => "darken",
        BlendMode::Multiply => "multiply",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::Lighten => "lighten",
        BlendMode::Screen => "screen",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "soft-light",
        BlendMode::HardLight => "hard-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    }
}

/// Turn an object name into a valid, readable XML id.
fn sanitize_id(name: &str) -> String {
    let mut s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    if s.is_empty() || !s.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        s.insert(0, '_');
    }
    s
}

impl Writer<'_> {
    fn num(&self, v: f64) -> String {
        fmt_num(v, self.opts.decimals)
    }
    fn indent(&self, depth: usize) -> String {
        if self.opts.minify { String::new() } else { "  ".repeat(depth) }
    }
    fn line(&mut self, s: &str) {
        let ind = self.indent(self.depth);
        self.body.push_str(&ind);
        self.body.push_str(s);
        if !self.opts.minify {
            self.body.push('\n');
        }
    }
    fn def(&mut self, depth: usize, s: &str) {
        let ind = self.indent(depth + 1);
        self.defs.push_str(&ind);
        self.defs.push_str(s);
        if !self.opts.minify {
            self.defs.push('\n');
        }
    }
    fn fresh_id(&mut self, prefix: &str) -> String {
        let mut i = 1;
        loop {
            let id = format!("{prefix}-{i}");
            if self.used_ids.insert(id.clone()) {
                return id;
            }
            i += 1;
        }
    }
    /// Reserve ids for every named object up front so generated def ids never collide with them.
    fn assign_name_ids(&mut self) {
        if !self.opts.object_ids {
            return;
        }
        let mut named: Vec<(NodeId, String)> = Vec::new();
        self.doc.walk(|n| {
            if let Some(name) = &n.name {
                named.push((n.id, sanitize_id(name)));
            }
        });
        for (id, base) in named {
            let mut cand = base.clone();
            let mut i = 2;
            while !self.used_ids.insert(cand.clone()) {
                cand = format!("{base}-{i}");
                i += 1;
            }
            self.names.insert(id, cand);
        }
    }
    fn id_attr(&self, n: &Node) -> String {
        self.names.get(&n.id).map(|id| format!(" id=\"{}\"", xml_escape(id))).unwrap_or_default()
    }
    fn matrix(&self, m: Affine) -> String {
        let c = m.as_coeffs();
        format!("matrix({} {} {} {} {} {})", self.num(c[0]), self.num(c[1]), self.num(c[2]), self.num(c[3]), self.num(c[4]), self.num(c[5]))
    }

    /// Style properties as attributes, an inline style, or a CSS class, per the styling option.
    fn attrs(&mut self, props: &Props) -> String {
        if props.is_empty() {
            return String::new();
        }
        let css = |ps: &[&(&str, String)]| ps.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(";");
        match self.opts.styling {
            Styling::PresentationAttributes => {
                // CSS-only properties go in a style attribute.
                let (style, attrs): (Vec<_>, Vec<_>) = props.iter().partition(|(k, _)| matches!(*k, "mix-blend-mode" | "isolation"));
                let mut s: String = attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", xml_escape(v))).collect();
                if !style.is_empty() {
                    s.push_str(&format!(" style=\"{}\"", xml_escape(&css(&style))));
                }
                s
            }
            Styling::InlineStyle => format!(" style=\"{}\"", xml_escape(&css(&props.iter().collect::<Vec<_>>()))),
            Styling::InternalCss => {
                let decl = css(&props.iter().collect::<Vec<_>>());
                let i = match self.classes.iter().position(|c| *c == decl) {
                    Some(i) => i,
                    None => {
                        self.classes.push(decl);
                        self.classes.len() - 1
                    }
                };
                format!(" class=\"cls-{}\"", i + 1)
            }
        }
    }

    fn node_props(n: &Node) -> Props {
        let mut p = Props::new();
        if n.opacity < 1.0 {
            p.push(("opacity", fmt_num(n.opacity as f64, 3)));
        }
        if n.blend != BlendMode::Normal {
            p.push(("mix-blend-mode", blend_css(n.blend).into()));
        }
        if n.isolate {
            p.push(("isolation", "isolate".into()));
        }
        p
    }

    fn path_d(&self, p: &PathData, m: Affine) -> String {
        let mut s = String::new();
        let pt = |p: Point| {
            let p = m * p;
            format!("{} {}", self.num(p.x), self.num(p.y))
        };
        for sp in &p.subpaths {
            let n = sp.anchors.len();
            if n == 0 {
                continue;
            }
            if !s.is_empty() {
                s.push(' ');
            }
            s.push('M');
            s.push_str(&pt(sp.anchors[0].p));
            for i in 0..sp.segment_count() {
                let (a, b) = (&sp.anchors[i], &sp.anchors[(i + 1) % n]);
                if sp.segment_is_line(i) {
                    if sp.closed && i == n - 1 {
                        break;
                    }
                    s.push_str(&format!(" L{}", pt(b.p)));
                } else {
                    s.push_str(&format!(" C{} {} {}", pt(a.h_out), pt(b.h_in), pt(b.p)));
                }
            }
            if sp.closed {
                s.push_str(" Z");
            }
        }
        s
    }

    fn paint(&mut self, p: &Paint, bounds: Option<Rect>) -> String {
        match p {
            Paint::None | Paint::Pattern { .. } => "none".into(),
            Paint::Solid { color, .. } => color.to_hex(),
            Paint::Gradient(g) => format!("url(#{})", self.gradient_def(g, bounds.unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0)))),
        }
    }

    fn gradient_def(&mut self, g: &GradientPaint, bounds: Rect) -> String {
        let mut geom = g.resolve(bounds);
        geom.transform(self.xf);
        let (s, e) = (geom.start, geom.end);
        let head = match g.gradient.kind {
            GradientKind::Radial => {
                let id = self.fresh_id("radial-gradient");
                let r = (e - s).hypot();
                if (geom.aspect - 1.0).abs() < 1e-9 {
                    (
                        id.clone(),
                        format!(
                            "<radialGradient id=\"{id}\" cx=\"{}\" cy=\"{}\" r=\"{}\" gradientUnits=\"userSpaceOnUse\">",
                            self.num(s.x),
                            self.num(s.y),
                            self.num(r)
                        ),
                        "radialGradient",
                    )
                } else {
                    let v = e - s;
                    let m = Affine::translate(s.to_vec2()) * Affine::rotate(v.y.atan2(v.x)) * Affine::scale_non_uniform(1.0, geom.aspect);
                    (
                        id.clone(),
                        format!(
                            "<radialGradient id=\"{id}\" cx=\"0\" cy=\"0\" r=\"{}\" gradientTransform=\"{}\" gradientUnits=\"userSpaceOnUse\">",
                            self.num(r),
                            self.matrix(m)
                        ),
                        "radialGradient",
                    )
                }
            }
            GradientKind::Linear | GradientKind::Freeform => {
                let id = self.fresh_id("linear-gradient");
                (
                    id.clone(),
                    format!(
                        "<linearGradient id=\"{id}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" gradientUnits=\"userSpaceOnUse\">",
                        self.num(s.x),
                        self.num(s.y),
                        self.num(e.x),
                        self.num(e.y)
                    ),
                    "linearGradient",
                )
            }
        };
        let (id, open, tag) = head;
        self.def(1, &open);
        for (off, c, o) in g.gradient.expanded_stops() {
            let op = if o < 1.0 { format!(" stop-opacity=\"{}\"", fmt_num(o as f64, 3)) } else { String::new() };
            self.def(2, &format!("<stop offset=\"{}\" stop-color=\"{}\"{op}/>", fmt_num(off as f64, 4), c.to_hex()));
        }
        self.def(1, &format!("</{tag}>"));
        id
    }

    fn fill_props(&mut self, f: &FillLayer, rule: FillRule, bounds: Option<Rect>, p: &mut Props) {
        let paint = self.paint(&f.paint, bounds);
        p.push(("fill", paint));
        if f.opacity < 1.0 {
            p.push(("fill-opacity", fmt_num(f.opacity as f64, 3)));
        }
        if rule == FillRule::EvenOdd {
            p.push(("fill-rule", "evenodd".into()));
        }
    }

    fn stroke_props(&mut self, s: &StrokeLayer, width: f64, bounds: Option<Rect>, p: &mut Props) {
        let paint = self.paint(&s.paint, bounds);
        p.push(("stroke", paint));
        if s.opacity < 1.0 {
            p.push(("stroke-opacity", fmt_num(s.opacity as f64, 3)));
        }
        if (width - 1.0).abs() > 1e-9 {
            p.push(("stroke-width", self.num(width)));
        }
        match s.cap {
            LineCap::Butt => {}
            LineCap::Round => p.push(("stroke-linecap", "round".into())),
            LineCap::Square => p.push(("stroke-linecap", "square".into())),
        }
        match s.join {
            LineJoin::Miter => {
                if (s.miter_limit - 4.0).abs() > 1e-9 {
                    p.push(("stroke-miterlimit", self.num(s.miter_limit)));
                }
            }
            LineJoin::Round => p.push(("stroke-linejoin", "round".into())),
            LineJoin::Bevel => p.push(("stroke-linejoin", "bevel".into())),
        }
        if let Some(d) = &s.dash
            && !d.pattern.is_empty()
            && d.pattern.iter().any(|v| *v > 0.0)
        {
            p.push(("stroke-dasharray", d.pattern.iter().map(|v| self.num(*v)).collect::<Vec<_>>().join(" ")));
            if d.offset != 0.0 {
                p.push(("stroke-dashoffset", self.num(d.offset)));
            }
        }
    }

    /// Paint a shape (`d`) with a node's appearance stack.
    fn shape(&mut self, n: &Node, d: &str, rule: FillRule, bounds: Option<Rect>) {
        let id = self.id_attr(n);
        let items: Vec<&AppearanceItem> = n
            .appearance
            .items
            .iter()
            .filter(|i| match i {
                AppearanceItem::Fill(f) => f.visible && !f.paint.is_none(),
                AppearanceItem::Stroke(s) => s.visible && !s.paint.is_none() && s.width > 0.0,
            })
            .collect();
        let fills = items.iter().filter(|i| matches!(i, AppearanceItem::Fill(_))).count();
        let strokes = items.len() - fills;
        let simple = fills <= 1
            && strokes <= 1
            && items.iter().all(|i| match i {
                AppearanceItem::Fill(f) => f.blend == BlendMode::Normal,
                AppearanceItem::Stroke(s) => s.blend == BlendMode::Normal && s.align == StrokeAlign::Center,
            });
        if simple {
            let mut p = Props::new();
            match items.iter().find_map(|i| if let AppearanceItem::Fill(f) = i { Some(f) } else { None }) {
                Some(f) => self.fill_props(f, rule, bounds, &mut p),
                None => {
                    p.push(("fill", "none".into()));
                    if rule == FillRule::EvenOdd {
                        p.push(("fill-rule", "evenodd".into()));
                    }
                }
            }
            if let Some(s) = items.iter().find_map(|i| if let AppearanceItem::Stroke(s) = i { Some(s) } else { None }) {
                self.stroke_props(s, s.width, bounds, &mut p);
                if matches!(items.first(), Some(AppearanceItem::Stroke(_))) && fills == 1 {
                    p.push(("paint-order", "stroke".into()));
                }
            }
            p.extend(Self::node_props(n));
            let a = self.attrs(&p);
            self.line(&format!("<path{id} d=\"{d}\"{a}/>"));
            return;
        }
        let a = self.attrs(&Self::node_props(n));
        self.line(&format!("<g{id}{a}>"));
        self.depth += 1;
        for it in items {
            let mut p = Props::new();
            match it {
                AppearanceItem::Fill(f) => {
                    self.fill_props(f, rule, bounds, &mut p);
                    if f.blend != BlendMode::Normal {
                        p.push(("mix-blend-mode", blend_css(f.blend).into()));
                    }
                    let a = self.attrs(&p);
                    self.line(&format!("<path d=\"{d}\"{a}/>"));
                }
                AppearanceItem::Stroke(s) => {
                    p.push(("fill", "none".into()));
                    let width = if s.align == StrokeAlign::Center { s.width } else { s.width * 2.0 };
                    self.stroke_props(s, width, bounds, &mut p);
                    if s.blend != BlendMode::Normal {
                        p.push(("mix-blend-mode", blend_css(s.blend).into()));
                    }
                    let rule_attr = if rule == FillRule::EvenOdd { " clip-rule=\"evenodd\"" } else { "" };
                    let extra = match s.align {
                        StrokeAlign::Center => String::new(),
                        StrokeAlign::Inside => {
                            let cid = self.fresh_id("clip-path");
                            self.def(1, &format!("<clipPath id=\"{cid}\">"));
                            self.def(2, &format!("<path d=\"{d}\"{rule_attr}/>"));
                            self.def(1, "</clipPath>");
                            format!(" clip-path=\"url(#{cid})\"")
                        }
                        StrokeAlign::Outside => {
                            let mid = self.fresh_id("mask");
                            let b = self
                                .xf
                                .transform_rect_bbox(bounds.unwrap_or_default())
                                .inflate(width * s.miter_limit.max(1.0) + 1.0, width * s.miter_limit.max(1.0) + 1.0);
                            let (x, y, w, h) = (self.num(b.x0), self.num(b.y0), self.num(b.width()), self.num(b.height()));
                            let fr = if rule == FillRule::EvenOdd { " fill-rule=\"evenodd\"" } else { "" };
                            self.def(
                                1,
                                &format!("<mask id=\"{mid}\" maskUnits=\"userSpaceOnUse\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\">"),
                            );
                            self.def(2, &format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"#fff\"/>"));
                            self.def(2, &format!("<path d=\"{d}\" fill=\"#000\"{fr}/>"));
                            self.def(1, "</mask>");
                            format!(" mask=\"url(#{mid})\"")
                        }
                    };
                    let a = self.attrs(&p);
                    self.line(&format!("<path d=\"{d}\"{a}{extra}/>"));
                }
            }
        }
        self.depth -= 1;
        self.line("</g>");
    }

    /// `(d, rule)` of every path in a clipping object.
    fn clip_shapes(&self, n: &Node, out: &mut Vec<(String, FillRule)>) {
        match &n.kind {
            NodeKind::Path { path, rule, .. } => out.push((self.path_d(path, self.xf), *rule)),
            NodeKind::Compound { children, rule } => {
                let d: Vec<String> = children.iter().filter_map(|c| c.path_data()).map(|p| self.path_d(p, self.xf)).collect();
                out.push((d.join(" "), *rule));
            }
            NodeKind::Group { children, .. } | NodeKind::Layer { children, .. } => {
                for c in children {
                    self.clip_shapes(c, out);
                }
            }
            NodeKind::Image(im) => {
                let r = Rect::new(0.0, 0.0, im.width as f64, im.height as f64);
                let p = drawcraft_geom::shapes::rectangle(r).transformed(im.xf);
                out.push((self.path_d(&p, self.xf), FillRule::NonZero));
            }
            NodeKind::Text(_) | NodeKind::SymbolInstance { .. } => {}
            NodeKind::Blend { .. } | NodeKind::Envelope { .. } | NodeKind::Mesh(_) => {
                let g = drawcraft_doc::live::expand_deep(n, None);
                self.clip_shapes(&g, out);
            }
        }
    }

    fn node(&mut self, n: &Node) {
        if !n.visible {
            return;
        }
        match &n.kind {
            NodeKind::Layer { template: true, .. } => {}
            NodeKind::Layer { children, .. } | NodeKind::Group { children, clip: false } => {
                let id = self.id_attr(n);
                let a = self.attrs(&Self::node_props(n));
                self.line(&format!("<g{id}{a}>"));
                self.depth += 1;
                for c in children {
                    self.node(c);
                }
                self.depth -= 1;
                self.line("</g>");
            }
            NodeKind::Group { children, clip: true } => {
                let Some((clip, rest)) = children.split_first() else { return };
                let mut shapes = Vec::new();
                self.clip_shapes(clip, &mut shapes);
                let cid = self.fresh_id("clip-path");
                self.def(1, &format!("<clipPath id=\"{cid}\">"));
                for (d, rule) in shapes {
                    let r = if rule == FillRule::EvenOdd { " clip-rule=\"evenodd\"" } else { "" };
                    self.def(2, &format!("<path d=\"{d}\"{r}/>"));
                }
                self.def(1, "</clipPath>");
                let id = self.id_attr(n);
                let a = self.attrs(&Self::node_props(n));
                self.line(&format!("<g{id} clip-path=\"url(#{cid})\"{a}>"));
                self.depth += 1;
                for c in rest {
                    self.node(c);
                }
                self.depth -= 1;
                self.line("</g>");
            }
            NodeKind::Path { guide: true, .. } => {}
            NodeKind::Path { path, rule, .. } => {
                if path.is_empty() {
                    return;
                }
                let d = self.path_d(path, self.xf);
                self.shape(n, &d, *rule, n.geometric_bounds());
            }
            NodeKind::Compound { children, rule } => {
                let d: Vec<String> = children
                    .iter()
                    .filter(|c| c.visible)
                    .filter_map(|c| c.path_data())
                    .filter(|p| !p.is_empty())
                    .map(|p| self.path_d(p, self.xf))
                    .collect();
                if d.is_empty() {
                    return;
                }
                self.shape(n, &d.join(" "), *rule, n.geometric_bounds());
            }
            NodeKind::Text(t) => self.text(n, t),
            NodeKind::Image(im) => {
                let href = match self.doc.images.get(&im.key) {
                    Some(b) if !b.bytes.is_empty() => format!("data:{};base64,{}", b.mime, base64_encode(&b.bytes)),
                    _ => match &im.link {
                        Some(l) => l.clone(),
                        None => return,
                    },
                };
                let id = self.id_attr(n);
                let m = self.matrix(self.xf * im.xf);
                let a = self.attrs(&Self::node_props(n));
                self.line(&format!(
                    "<image{id} width=\"{}\" height=\"{}\" transform=\"{m}\" preserveAspectRatio=\"none\"{a} xlink:href=\"{}\"/>",
                    im.width,
                    im.height,
                    xml_escape(&href)
                ));
            }
            NodeKind::SymbolInstance { symbol, xf } => {
                let Some(sym) = self.doc.symbols.iter().find(|s| s.name == *symbol) else { return };
                let saved = self.xf;
                self.xf = saved * *xf;
                let id = self.id_attr(n);
                let a = self.attrs(&Self::node_props(n));
                self.line(&format!("<g{id}{a}>"));
                self.depth += 1;
                self.node(&sym.art.clone());
                self.depth -= 1;
                self.line("</g>");
                self.xf = saved;
            }
            // Live blends/envelopes/meshes export their evaluated (expanded) form.
            NodeKind::Blend { .. } | NodeKind::Envelope { .. } | NodeKind::Mesh(_) => {
                let g = drawcraft_doc::live::expand_deep(n, None);
                self.node(&g);
            }
        }
    }

    fn char_props(&mut self, st: &CharStyle) -> Props {
        let mut p = Props::new();
        let fam =
            if st.font_family.contains(|c: char| c.is_whitespace() || c == ',') { format!("'{}'", st.font_family) } else { st.font_family.clone() };
        p.push(("font-family", fam));
        p.push(("font-size", self.num(st.size)));
        let fs = st.font_style.to_ascii_lowercase();
        if fs.contains("bold") || fs.contains("black") || fs.contains("heavy") {
            p.push(("font-weight", "bold".into()));
        }
        if fs.contains("italic") || fs.contains("oblique") {
            p.push(("font-style", "italic".into()));
        }
        let fill = match &st.fill {
            Paint::Gradient(g) => g.gradient.stops.first().map(|s| s.color.to_hex()).unwrap_or_else(|| "none".into()),
            p => self.paint(p, None),
        };
        p.push(("fill", fill));
        if !st.stroke.is_none() && st.stroke_width > 0.0 {
            let s = match &st.stroke {
                Paint::Gradient(g) => g.gradient.stops.first().map(|s| s.color.to_hex()).unwrap_or_else(|| "none".into()),
                p => self.paint(p, None),
            };
            p.push(("stroke", s));
            p.push(("stroke-width", self.num(st.stroke_width)));
        }
        if st.tracking != 0.0 {
            p.push(("letter-spacing", self.num(st.tracking / 1000.0 * st.size)));
        }
        match (st.underline, st.strikethrough) {
            (true, true) => p.push(("text-decoration", "underline line-through".into())),
            (true, false) => p.push(("text-decoration", "underline".into())),
            (false, true) => p.push(("text-decoration", "line-through".into())),
            _ => {}
        }
        p
    }

    fn text(&mut self, n: &Node, t: &TextObject) {
        let first = t.first_style();
        let lead = first.effective_leading();
        let id = self.id_attr(n);
        let (m, on_path) = match &t.kind {
            TextKind::Point => (self.xf * t.xf, None),
            TextKind::Area { frame } => {
                let b = frame.bounds().unwrap_or_default();
                (self.xf * t.xf * Affine::translate(Vec2::new(b.x0 + t.para.left_indent, b.y0 + first.size)), None)
            }
            TextKind::OnPath { path, start } => {
                let pid = self.fresh_id("text-path");
                let d = self.path_d(path, self.xf * t.xf);
                self.def(1, &format!("<path id=\"{pid}\" d=\"{d}\"/>"));
                (Affine::IDENTITY, Some((pid, *start)))
            }
        };
        let mut props = self.char_props(&first);
        match t.para.justify {
            Justify::Center | Justify::JustifyCenter => props.push(("text-anchor", "middle".into())),
            Justify::Right | Justify::JustifyRight => props.push(("text-anchor", "end".into())),
            _ => {}
        }
        props.extend(Self::node_props(n));
        let a = self.attrs(&props);
        let tr = if m == Affine::IDENTITY { String::new() } else { format!(" transform=\"{}\"", self.matrix(m)) };
        let mut s = format!("<text{id}{tr} xml:space=\"preserve\"{a}>");
        if let Some((pid, start)) = &on_path {
            s.push_str(&format!("<textPath xlink:href=\"#{pid}\" startOffset=\"{}%\">", fmt_num(start * 100.0, 3)));
        }
        let base = self.char_props(&first);
        let mut line = 0usize;
        let mut pending = false;
        for run in &t.runs {
            let rp = self.char_props(&run.style);
            let diff: Props = rp.into_iter().filter(|kv| !base.contains(kv)).collect();
            for (i, piece) in run.text.split('\n').enumerate() {
                if i > 0 {
                    line += 1;
                    pending = true;
                }
                if piece.is_empty() {
                    continue;
                }
                let pos = if pending && on_path.is_none() { format!(" x=\"0\" y=\"{}\"", self.num(line as f64 * lead)) } else { String::new() };
                pending = false;
                let a = self.attrs(&diff);
                s.push_str(&format!("<tspan{pos}{a}>{}</tspan>", xml_escape(piece)));
            }
        }
        if on_path.is_some() {
            s.push_str("</textPath>");
        }
        s.push_str("</text>");
        self.line(&s);
    }
}
