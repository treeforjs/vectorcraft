//! Fill/stroke, Stroke panel, Appearance, Transparency, Swatches, Graphic Styles.

use serde_json::{Value, json};
use vectorcraft_color::{BlendMode, Color, Gradient, GradientKind, GradientPaint, Paint};
use vectorcraft_doc::{Appearance, AppearanceItem, Arrowhead, Dash, FillLayer, LineCap, LineJoin, NodeKind, StrokeAlign, StrokeLayer};

use super::*;
use crate::EngineError;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "paint.setFill",
            "Fill",
            [],
            None,
            "{color?: \"#rrggbb\"|[r,g,b]|{c,m,y,k}|{gray}, none?: true, swatch?: name, gradient?: {kind, stops:[{offset,color}], angle?}, ids?} sets selection fill and the default",
            has_doc,
            |s, p| set_paint(s, p, true)
        ),
        cmd!("paint.setStroke", "Stroke", [], None, "same as paint.setFill, for the stroke", has_doc, |s, p| set_paint(s, p, false)),
        cmd!("paint.swap", "Swap Fill and Stroke", [], Some("Shift+X"), "{}", has_doc, swap),
        cmd!("paint.default", "Default Fill and Stroke", [], Some("D"), "{}", has_doc, default_paint),
        cmd!("paint.toggleActive", "Toggle Fill/Stroke Focus", [], Some("X"), "{}", always, |s, _| {
            s.fill_active = !s.fill_active;
            Ok(json!({ "fillActive": s.fill_active }))
        }),
        cmd!("paint.none", "None", [], Some("/"), "{} set the active proxy (fill or stroke) to None", has_doc, |s, _| {
            let f = s.fill_active;
            set_paint(s, &json!({"none": true}), f)
        }),
        cmd!(
            "stroke.set",
            "Stroke Options",
            ["Window", "Stroke"],
            None,
            "{weight?, cap?: butt|round|square, join?: miter|round|bevel, miterLimit?, align?: center|inside|outside, dash?: [d,g,…]|null, dashOffset?, alignDashes?, startArrow?, endArrow?: name|null, profile?: \"uniform\"|\"lens\"|\"taperStart\"|\"taperEnd\", ids?}",
            has_doc,
            stroke_set
        ),
        cmd!("appearance.addFill", "Add New Fill", ["Window", "Appearance"], None, "{}", has_selection, |s, _| add_item(s, true)),
        cmd!("appearance.addStroke", "Add New Stroke", ["Window", "Appearance"], None, "{}", has_selection, |s, _| add_item(s, false)),
        cmd!("appearance.clear", "Clear Appearance", ["Window", "Appearance"], None, "{}", has_selection, clear_appearance),
        cmd!("appearance.reduceToBasic", "Reduce to Basic Appearance", ["Window", "Appearance"], None, "{}", has_selection, reduce_basic),
        cmd!(
            "appearance.setItem",
            "Appearance Item",
            [],
            None,
            "{index, opacity?, blend?, visible?, color?|none?} edit one fill/stroke of the selection's appearance stack",
            has_selection,
            set_item
        ),
        cmd!("appearance.removeItem", "Remove Item", [], None, "{index}", has_selection, remove_item),
        cmd!("appearance.addEffect", "Add Effect", ["Effect"], None, "{effect: id, params?: {…}} append a live effect", has_selection, add_effect),
        cmd!(
            "transparency.set",
            "Transparency",
            ["Window", "Transparency"],
            None,
            "{opacity?: 0..100, blend?: name, isolate?, knockout?}",
            has_selection,
            transparency
        ),
        cmd!("graphicStyle.apply", "Apply Graphic Style", ["Window", "Graphic Styles"], None, "{name}", has_selection, style_apply),
        cmd!("graphicStyle.new", "New Graphic Style", ["Window", "Graphic Styles"], None, "{name?} from the selection", has_selection, style_new),
        cmd!("swatch.new", "New Swatch", ["Window", "Swatches"], None, "{name?, color?, global?} (default: current fill)", has_doc, swatch_new),
        cmd!("swatch.delete", "Delete Swatch", ["Window", "Swatches"], None, "{name}", has_doc, swatch_delete),
    ]
}

/// Parse a paint from params (color / none / swatch / gradient). None = no paint keys given.
pub(crate) fn paint_from(s: &Session, p: &Value) -> Result<Option<Paint>> {
    if bool_or(p, "none", false) {
        return Ok(Some(Paint::None));
    }
    if let Some(name) = str_param(p, "swatch") {
        let st = s.doc()?;
        // A pattern definition works as its swatch even without a swatch entry.
        if st.doc.swatch(name).is_none() && st.doc.pattern(name).is_some() {
            return Ok(Some(vectorcraft_doc::pattern::pattern_paint(name)));
        }
        let sw = st.doc.swatch(name).ok_or_else(|| EngineError::Other(format!("no swatch `{name}`")))?;
        let mut paint = sw.paint.clone();
        if sw.global
            && let Paint::Solid { swatch, .. } = &mut paint
        {
            *swatch = Some(name.to_string());
        }
        return Ok(Some(paint));
    }
    if let Some(g) = p.get("gradient") {
        let kind = match str_param(g, "kind") {
            Some("radial") => GradientKind::Radial,
            _ => GradientKind::Linear,
        };
        let mut grad = Gradient { kind, ..Default::default() };
        if let Some(stops) = g.get("stops").and_then(Value::as_array) {
            grad.stops = stops
                .iter()
                .filter_map(|st| {
                    Some(vectorcraft_color::GradientStop {
                        offset: st.get("offset")?.as_f64()? as f32,
                        color: color_value(st.get("color")?)?,
                        opacity: st.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                        midpoint: 0.5,
                    })
                })
                .collect();
            grad.sort();
        }
        let mut gp = GradientPaint::new(grad);
        gp.angle = f64_or(g, "angle", 0.0);
        return Ok(Some(Paint::Gradient(Box::new(gp))));
    }
    if let Some(c) = p.get("color") {
        let c = color_value(c).ok_or_else(|| bad("paint", format!("bad color {c}")))?;
        return Ok(Some(Paint::solid(c)));
    }
    Ok(None)
}

/// Leaf objects whose appearance should change for `ids` (groups apply to children).
fn paint_targets(s: &Session, p: &Value) -> Result<Vec<vectorcraft_doc::NodeId>> {
    let ids = targets(s, p)?;
    let d = &s.doc()?.doc;
    let mut out = vec![];
    for id in ids {
        let Some(n) = d.node(id) else { continue };
        match &n.kind {
            NodeKind::Group { .. } | NodeKind::Layer { .. } => n.walk(&mut |c| {
                if !c.is_container() || matches!(c.kind, NodeKind::Compound { .. }) {
                    out.push(c.id)
                }
            }),
            _ => out.push(id),
        }
    }
    // Don't recurse into compound children (the compound owns the appearance).
    let comp: Vec<_> = out.iter().filter(|id| matches!(d.node(**id).map(|n| &n.kind), Some(NodeKind::Compound { .. }))).copied().collect();
    out.retain(|id| !comp.iter().any(|c| d.parent_of(*id) == Some(*c)));
    Ok(out)
}

fn set_paint(s: &mut Session, p: &Value, fill: bool) -> Result<Value> {
    let paint = paint_from(s, p)?.ok_or_else(|| bad("paint.setFill", "give color, none, swatch or gradient"))?;
    if fill {
        s.paint.fill = paint.clone();
    } else {
        s.paint.stroke = paint.clone();
    }
    s.fill_active = fill;
    let ids = paint_targets(s, p)?;
    if ids.is_empty() {
        return ok();
    }
    s.edit(if fill { "Fill Color" } else { "Stroke Color" }, |d, _| {
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            if let NodeKind::Text(t) = &mut n.kind {
                for r in &mut t.runs {
                    if fill {
                        r.style.fill = paint.clone();
                    } else {
                        r.style.stroke = paint.clone();
                        if r.style.stroke_width == 0.0 {
                            r.style.stroke_width = 1.0;
                        }
                    }
                }
                continue;
            }
            if fill {
                n.appearance.set_fill(paint.clone());
            } else {
                n.appearance.set_stroke(paint.clone());
            }
        }
        Ok(())
    })?;
    ok()
}

fn swap(s: &mut Session, _: &Value) -> Result<Value> {
    std::mem::swap(&mut s.paint.fill, &mut s.paint.stroke);
    let ids = paint_targets(s, &json!({}))?;
    if !ids.is_empty() {
        s.edit("Swap Fill and Stroke", |d, _| {
            for id in &ids {
                if let Some(n) = d.node_mut(*id) {
                    let f = n.appearance.fill_paint();
                    let st = n.appearance.stroke_paint();
                    n.appearance.set_fill(st);
                    n.appearance.set_stroke(f);
                }
            }
            Ok(())
        })?;
    }
    ok()
}

fn default_paint(s: &mut Session, _: &Value) -> Result<Value> {
    s.paint.fill = Paint::solid(Color::WHITE);
    s.paint.stroke = Paint::solid(Color::BLACK);
    s.paint.stroke_width = 1.0;
    let ids = paint_targets(s, &json!({}))?;
    if !ids.is_empty() {
        s.edit("Default Fill and Stroke", |d, _| {
            for id in &ids {
                if let Some(n) = d.node_mut(*id)
                    && !matches!(n.kind, NodeKind::Text(_))
                {
                    n.appearance = Appearance::default_art();
                }
            }
            Ok(())
        })?;
    }
    ok()
}

fn stroke_set(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(w) = p.get("weight").and_then(Value::as_f64) {
        s.paint.stroke_width = w.max(0.0);
    }
    let ids = paint_targets(s, p)?;
    if ids.is_empty() {
        return ok();
    }
    let arrow = |k: &str| -> Result<Option<Option<Arrowhead>>> {
        match p.get(k) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(n)) if n == "none" => Ok(Some(None)),
            Some(v) => {
                serde_json::from_value::<Arrowhead>(v.clone()).map(|a| Some(Some(a))).map_err(|_| bad("stroke.set", format!("unknown arrowhead {v}")))
            }
        }
    };
    let (sa, ea) = (arrow("startArrow")?, arrow("endArrow")?);
    s.edit("Stroke", |d, _| {
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            if n.appearance.stroke().is_none() {
                n.appearance.set_stroke(Paint::solid(Color::BLACK));
            }
            let st: &mut StrokeLayer = n.appearance.stroke_mut().unwrap();
            if let Some(w) = p.get("weight").and_then(Value::as_f64) {
                st.width = w.max(0.0);
            }
            match str_param(p, "cap") {
                Some("round") => st.cap = LineCap::Round,
                Some("square") | Some("projecting") => st.cap = LineCap::Square,
                Some("butt") => st.cap = LineCap::Butt,
                _ => {}
            }
            match str_param(p, "join") {
                Some("round") => st.join = LineJoin::Round,
                Some("bevel") => st.join = LineJoin::Bevel,
                Some("miter") => st.join = LineJoin::Miter,
                _ => {}
            }
            if let Some(m) = p.get("miterLimit").and_then(Value::as_f64) {
                st.miter_limit = m.clamp(1.0, 500.0);
            }
            match str_param(p, "align") {
                Some("inside") => st.align = StrokeAlign::Inside,
                Some("outside") => st.align = StrokeAlign::Outside,
                Some("center") => st.align = StrokeAlign::Center,
                _ => {}
            }
            match p.get("dash") {
                Some(Value::Null) => st.dash = None,
                Some(Value::Array(a)) => {
                    let pattern: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                    st.dash = if pattern.is_empty() {
                        None
                    } else {
                        Some(Dash { pattern, offset: f64_or(p, "dashOffset", 0.0), align_corners: bool_or(p, "alignDashes", false) })
                    };
                }
                _ => {}
            }
            if let Some(a) = sa {
                st.start_arrow = a;
            }
            if let Some(a) = ea {
                st.end_arrow = a;
            }
            match str_param(p, "profile") {
                Some("uniform") => st.profile = None,
                Some("lens") => st.profile = Some(vectorcraft_doc::WidthProfile::lens()),
                Some("taperStart") => st.profile = Some(vectorcraft_doc::WidthProfile::taper_start()),
                Some("taperEnd") => st.profile = Some(vectorcraft_doc::WidthProfile::taper_end()),
                _ => {}
            }
        }
        Ok(())
    })?;
    ok()
}

fn add_item(s: &mut Session, fill: bool) -> Result<Value> {
    let ids = paint_targets(s, &json!({}))?;
    s.edit(if fill { "Add New Fill" } else { "Add New Stroke" }, |d, _| {
        for id in &ids {
            if let Some(n) = d.node_mut(*id) {
                let item = if fill {
                    AppearanceItem::Fill(FillLayer::new(n.appearance.fill_paint()))
                } else {
                    AppearanceItem::Stroke(StrokeLayer::new(n.appearance.stroke_paint(), n.appearance.stroke_width().max(1.0)))
                };
                n.appearance.items.push(item);
            }
        }
        Ok(())
    })?;
    ok()
}

fn clear_appearance(s: &mut Session, _: &Value) -> Result<Value> {
    let ids = paint_targets(s, &json!({}))?;
    s.edit("Clear Appearance", |d, _| {
        for id in &ids {
            if let Some(n) = d.node_mut(*id) {
                n.appearance = Appearance::basic(Paint::None, Paint::None, 1.0);
            }
        }
        Ok(())
    })?;
    ok()
}

fn reduce_basic(s: &mut Session, _: &Value) -> Result<Value> {
    let ids = paint_targets(s, &json!({}))?;
    s.edit("Reduce to Basic Appearance", |d, _| {
        for id in &ids {
            if let Some(n) = d.node_mut(*id) {
                let f = n.appearance.fill_paint();
                let st = n.appearance.stroke().cloned();
                n.appearance = Appearance { items: vec![AppearanceItem::Fill(FillLayer::new(f))], effects: vec![] };
                if let Some(mut st) = st {
                    st.effects.clear();
                    st.opacity = 1.0;
                    st.blend = BlendMode::Normal;
                    n.appearance.items.push(AppearanceItem::Stroke(st));
                }
            }
        }
        Ok(())
    })?;
    ok()
}

fn set_item(s: &mut Session, p: &Value) -> Result<Value> {
    let idx = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("appearance.setItem", "missing index"))? as usize;
    let paint = paint_from(s, p)?;
    let ids = paint_targets(s, p)?;
    let blend = str_param(p, "blend").and_then(BlendMode::parse);
    s.edit("Appearance", |d, _| {
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            let Some(item) = n.appearance.items.get_mut(idx) else { continue };
            let (pp, op, bl, vis) = match item {
                AppearanceItem::Fill(f) => (&mut f.paint, &mut f.opacity, &mut f.blend, &mut f.visible),
                AppearanceItem::Stroke(st) => {
                    if let Some(w) = p.get("weight").and_then(Value::as_f64) {
                        st.width = w.max(0.0);
                    }
                    (&mut st.paint, &mut st.opacity, &mut st.blend, &mut st.visible)
                }
            };
            if let Some(pa) = &paint {
                *pp = pa.clone();
            }
            if let Some(o) = p.get("opacity").and_then(Value::as_f64) {
                *op = if o > 1.0 { o / 100.0 } else { o }.clamp(0.0, 1.0) as f32;
            }
            if let Some(b) = blend {
                *bl = b;
            }
            if let Some(v) = p.get("visible").and_then(Value::as_bool) {
                *vis = v;
            }
        }
        Ok(())
    })?;
    ok()
}

fn remove_item(s: &mut Session, p: &Value) -> Result<Value> {
    let idx = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("appearance.removeItem", "missing index"))? as usize;
    let ids = paint_targets(s, p)?;
    s.edit("Remove Item", |d, _| {
        for id in &ids {
            if let Some(n) = d.node_mut(*id)
                && idx < n.appearance.items.len()
            {
                n.appearance.items.remove(idx);
            }
        }
        Ok(())
    })?;
    ok()
}

fn add_effect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = str_param(p, "effect").ok_or_else(|| bad("appearance.addEffect", "missing effect id"))?.to_string();
    let params = p.get("params").cloned().unwrap_or(json!({}));
    let ids = targets(s, p)?;
    s.edit("Add Effect", |d, _| {
        for nid in &ids {
            if let Some(n) = d.node_mut(*nid) {
                n.appearance.effects.push(vectorcraft_doc::Effect { id: id.clone(), params: params.clone(), visible: true });
            }
        }
        Ok(())
    })?;
    ok()
}

fn transparency(s: &mut Session, p: &Value) -> Result<Value> {
    let mut q = p.clone();
    if let Some(o) = p.get("opacity").and_then(Value::as_f64) {
        q["opacity"] = json!(o / 100.0);
    }
    s.execute("object.setProps", &q)
}

fn style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("graphicStyle.apply", "missing name"))?;
    let ap = s
        .doc()?
        .doc
        .graphic_styles
        .iter()
        .find(|g| g.name == name)
        .map(|g| g.appearance.clone())
        .ok_or_else(|| EngineError::Other(format!("no graphic style `{name}`")))?;
    let ids = paint_targets(s, p)?;
    s.edit("Apply Graphic Style", |d, _| {
        for id in &ids {
            if let Some(n) = d.node_mut(*id) {
                n.appearance = ap.clone();
            }
        }
        Ok(())
    })?;
    ok()
}

fn style_new(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let id = *st.selection.objects.first().unwrap();
    let ap = st.doc.node(id).map(|n| n.appearance.clone()).unwrap_or_default();
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| format!("Graphic Style {}", st.doc.graphic_styles.len() + 1));
    s.edit("New Graphic Style", |d, _| {
        d.graphic_styles.push(vectorcraft_doc::GraphicStyle { name: name.clone(), appearance: ap });
        Ok(())
    })?;
    Ok(json!({ "name": name }))
}

fn swatch_new(s: &mut Session, p: &Value) -> Result<Value> {
    let paint = match paint_from(s, p)? {
        Some(x) => x,
        None => s.paint.fill.clone(),
    };
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => match &paint {
            Paint::Solid { color, .. } => {
                let [r, g, b] = color.to_rgb();
                format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
            }
            _ => format!("Swatch {}", s.doc()?.doc.swatches.len() + 1),
        },
    };
    let global = bool_or(p, "global", false);
    s.edit("New Swatch", |d, _| {
        d.swatches.push(vectorcraft_color::Swatch { name: name.clone(), paint, global, spot: false });
        Ok(())
    })?;
    Ok(json!({ "name": name }))
}

fn swatch_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("swatch.delete", "missing name"))?.to_string();
    s.edit("Delete Swatch", |d, _| {
        d.swatches.retain(|sw| sw.name != name);
        for g in &mut d.swatch_groups {
            g.swatches.retain(|sw| sw.name != name);
        }
        Ok(())
    })?;
    ok()
}
