//! Commands the panels need beyond the core set: in-place gradient editing (Gradient panel),
//! appearance item duplicate/reorder, artboard reorder/duplicate, swatch groups, graphic style
//! management and the advanced Character/Paragraph attributes.

use drawcraft_color::{Gradient, GradientKind, GradientPaint, GradientStop, Paint, SwatchGroup};
use drawcraft_doc::{Document, NodeKind};
use serde_json::{Value, json};

use super::*;
use crate::EngineError;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "paint.editGradient",
            "Gradient",
            ["Window", "Gradient"],
            None,
            "{stroke?: bool (default: the active proxy), kind?: linear|radial|freeform, stops?: [{offset 0..1, color, opacity? 0..1, midpoint? 0..1}], angle?: deg, aspect?: %, reverse?: bool, ids?} edit the gradient in place (keeps its placement); solid/none paints become the default gradient",
            has_doc,
            edit_gradient
        ),
        cmd!(
            "stroke.setAdvanced",
            "Stroke Options",
            [],
            None,
            "{arrowScale?: [start %, end %], swapArrows?: bool, flipProfile?: \"along\"|\"across\", brush?: name|null, ids?} Stroke panel extras",
            has_doc,
            stroke_advanced
        ),
        cmd!("appearance.duplicateItem", "Duplicate Item", ["Window", "Appearance"], None, "{index}", has_selection, duplicate_item),
        cmd!("appearance.moveItem", "Reorder Appearance Item", [], None, "{from, to} (paint-order indices)", has_selection, move_item),
        cmd!("artboard.reorder", "Move Artboard Up/Down", [], None, "{index, to} change an artboard's number", has_doc, artboard_reorder),
        cmd!(
            "artboard.duplicate",
            "Duplicate Artboard",
            ["Window", "Artboards"],
            None,
            "{index} copy placed right of the last artboard",
            has_doc,
            artboard_duplicate
        ),
        cmd!(
            "swatch.newGroup",
            "New Color Group",
            ["Window", "Swatches"],
            None,
            "{name?, swatches?: [names] (moved into the group), colors?: [colour] (added as new swatches)}",
            has_doc,
            swatch_new_group
        ),
        cmd!("swatch.duplicate", "Duplicate Swatch", ["Window", "Swatches"], None, "{name}", has_doc, swatch_duplicate),
        cmd!("swatch.sortByName", "Sort by Name", ["Window", "Swatches"], None, "{}", has_doc, swatch_sort),
        cmd!("graphicStyle.delete", "Delete Graphic Style", ["Window", "Graphic Styles"], None, "{name}", has_doc, style_delete),
        cmd!("graphicStyle.duplicate", "Duplicate Graphic Style", ["Window", "Graphic Styles"], None, "{name}", has_doc, style_duplicate),
        cmd!(
            "text.setFormat",
            "Character / Paragraph",
            [],
            None,
            "{ids?|id?, kerning?: 1/1000 em|\"auto\", baselineShift?: pt, hScale?: %, vScale?: %, rotation?: deg, underline?, strikethrough?, allCaps?, leftIndent?, rightIndent?, firstLineIndent?, spaceBefore?, spaceAfter?: pt, hyphenate?: bool}",
            has_doc,
            set_format
        ),
    ]
}

// ---------- gradient ----------

/// Leaf paint targets (groups apply to their children), like the Swatches/Color panels.
fn leaf_targets(s: &Session, p: &Value) -> Result<Vec<NodeId>> {
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
    let comp: Vec<_> = out.iter().filter(|id| matches!(d.node(**id).map(|n| &n.kind), Some(NodeKind::Compound { .. }))).copied().collect();
    out.retain(|id| !comp.iter().any(|c| d.parent_of(*id) == Some(*c)));
    Ok(out)
}

/// Apply the gradient edits in `p` to `paint` (pure; unit-tested).
pub(crate) fn apply_gradient_edit(paint: &Paint, p: &Value, bounds: Option<drawcraft_geom::Rect>) -> std::result::Result<Paint, String> {
    let mut gp = match paint {
        Paint::Gradient(g) => (**g).clone(),
        _ => GradientPaint::new(Gradient::default()),
    };
    if let Some(k) = str_param(p, "kind") {
        let kind = match k.to_ascii_lowercase().as_str() {
            "linear" => GradientKind::Linear,
            "radial" => GradientKind::Radial,
            "freeform" => GradientKind::Freeform,
            _ => return Err(format!("unknown gradient kind `{k}`")),
        };
        if kind != gp.gradient.kind {
            gp.gradient.kind = kind;
            gp.geom = None;
        }
    }
    if let Some(stops) = p.get("stops") {
        let arr = stops.as_array().ok_or("`stops` must be an array")?;
        let mut out = Vec::with_capacity(arr.len());
        for st in arr {
            let offset = st.get("offset").and_then(Value::as_f64).ok_or("stop needs `offset`")?;
            let color = st.get("color").and_then(color_value).ok_or("stop needs a valid `color`")?;
            let opacity = st.get("opacity").and_then(Value::as_f64).map(|o| if o > 1.0 { o / 100.0 } else { o }).unwrap_or(1.0);
            let midpoint = st.get("midpoint").and_then(Value::as_f64).unwrap_or(0.5);
            out.push(GradientStop {
                offset: offset.clamp(0.0, 1.0) as f32,
                color,
                opacity: opacity.clamp(0.0, 1.0) as f32,
                midpoint: midpoint.clamp(0.13, 0.87) as f32,
            });
        }
        if out.len() < 2 {
            return Err("a gradient needs at least two stops".into());
        }
        gp.gradient.stops = out;
        gp.gradient.sort();
        gp.swatch = None;
    }
    if bool_or(p, "reverse", false) {
        gp.gradient.reverse();
        // Midpoints belong to the segment to the right; mirror them.
        let n = gp.gradient.stops.len();
        let mids: Vec<f32> = gp.gradient.stops.iter().map(|s| s.midpoint).collect();
        for i in 0..n {
            gp.gradient.stops[i].midpoint = if i + 1 < n { 1.0 - mids[n - 2 - i] } else { 0.5 };
        }
    }
    if let Some(a) = p.get("angle").and_then(Value::as_f64) {
        let a = ((a + 180.0).rem_euclid(360.0)) - 180.0;
        gp.angle = a;
        if let Some(g) = &mut gp.geom {
            let r = a.to_radians();
            let dir = drawcraft_geom::Vec2::new(r.cos(), -r.sin());
            if gp.gradient.kind == GradientKind::Radial {
                g.end = g.start + dir * g.length();
            } else {
                let c = g.start.midpoint(g.end);
                let half = g.length() / 2.0;
                g.start = c - dir * half;
                g.end = c + dir * half;
            }
        }
    }
    if let Some(asp) = p.get("aspect").and_then(Value::as_f64) {
        let asp = (asp / 100.0).clamp(0.005, 327.67);
        if gp.geom.is_none()
            && let Some(b) = bounds
        {
            gp.geom = Some(gp.resolve(b));
        }
        if let Some(g) = &mut gp.geom {
            g.aspect = asp;
        }
    }
    Ok(Paint::Gradient(Box::new(gp)))
}

fn edit_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "paint.editGradient";
    let stroke = p.get("stroke").and_then(Value::as_bool).unwrap_or(!s.fill_active);
    let ids = leaf_targets(s, p)?;
    // Validate against the defaults first so bad params fail without touching the document.
    let default_paint = if stroke { s.paint.stroke.clone() } else { s.paint.fill.clone() };
    let new_default = apply_gradient_edit(&default_paint, p, None).map_err(|e| bad(C, e))?;
    if ids.is_empty() {
        if stroke {
            s.paint.stroke = new_default;
        } else {
            s.paint.fill = new_default;
        }
        return Ok(json!({"ids": []}));
    }
    let mut err = None;
    s.edit("Gradient", |d, _| {
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            if let NodeKind::Text(t) = &mut n.kind {
                for r in &mut t.runs {
                    let cur = if stroke { &mut r.style.stroke } else { &mut r.style.fill };
                    match apply_gradient_edit(cur, p, None) {
                        Ok(np) => *cur = np,
                        Err(e) => err = Some(e),
                    }
                }
                continue;
            }
            let b = n.geometric_bounds();
            let ap = &mut n.appearance;
            let cur = if stroke { ap.stroke_paint() } else { ap.fill_paint() };
            match apply_gradient_edit(&cur, p, b) {
                Ok(np) => {
                    if stroke {
                        ap.set_stroke(np)
                    } else {
                        ap.set_fill(np)
                    }
                }
                Err(e) => err = Some(e),
            }
        }
        match err.take() {
            Some(e) => Err(bad(C, e)),
            None => Ok(()),
        }
    })?;
    if stroke {
        s.paint.stroke = new_default;
    } else {
        s.paint.fill = new_default;
    }
    Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
}

// ---------- appearance ----------

fn duplicate_item(s: &mut Session, p: &Value) -> Result<Value> {
    let idx = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("appearance.duplicateItem", "missing index"))? as usize;
    let ids = leaf_targets(s, p)?;
    s.edit("Duplicate Item", |d, _| {
        let mut any = false;
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            if let Some(item) = n.appearance.items.get(idx).cloned() {
                n.appearance.items.insert(idx + 1, item);
                any = true;
            }
        }
        if any { Ok(()) } else { Err(bad("appearance.duplicateItem", format!("no item at index {idx}"))) }
    })?;
    ok()
}

fn move_item(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "appearance.moveItem";
    let from = p.get("from").and_then(Value::as_u64).ok_or_else(|| bad(C, "missing from"))? as usize;
    let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad(C, "missing to"))? as usize;
    let ids = leaf_targets(s, p)?;
    s.edit("Reorder Appearance", |d, _| {
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            let items = &mut n.appearance.items;
            if from >= items.len() {
                return Err(bad(C, format!("no item at index {from}")));
            }
            let it = items.remove(from);
            let to = to.min(items.len());
            items.insert(to, it);
        }
        Ok(())
    })?;
    ok()
}

// ---------- artboards ----------

fn artboard_reorder(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "artboard.reorder";
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(C, "missing index"))? as usize;
    let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad(C, "missing to"))? as usize;
    s.edit("Reorder Artboards", |d, _| {
        if i >= d.artboards.len() {
            return Err(EngineError::Other("no such artboard".into()));
        }
        let a = d.artboards.remove(i);
        let to = to.min(d.artboards.len());
        d.artboards.insert(to, a);
        Ok(())
    })?;
    ok()
}

fn artboard_duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = s.edit("Duplicate Artboard", |d, _| {
        let src = d.artboards.get(i).cloned().ok_or_else(|| EngineError::Other("no such artboard".into()))?;
        let right = d.artboards.iter().map(|a| a.rect.x1).fold(f64::MIN, f64::max);
        let mut a = src.clone();
        a.id = d.artboards.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        a.name = format!("{} copy", src.name);
        let dx = right + 20.0 - src.rect.x0;
        a.rect = drawcraft_geom::Rect::new(src.rect.x0 + dx, src.rect.y0, src.rect.x1 + dx, src.rect.y1);
        d.artboards.push(a);
        Ok(d.artboards.len() - 1)
    })?;
    Ok(json!({"index": index}))
}

// ---------- swatches ----------

fn unique_name(d: &Document, base: &str) -> String {
    let exists = |n: &str| d.swatch(n).is_some() || d.swatch_groups.iter().any(|g| g.name == n);
    if !exists(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !exists(n)).unwrap_or_else(|| base.to_string())
}

fn swatch_new_group(s: &mut Session, p: &Value) -> Result<Value> {
    let names: Vec<String> =
        p.get("swatches").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let colors: Vec<drawcraft_color::Color> =
        p.get("colors").and_then(Value::as_array).map(|a| a.iter().filter_map(color_value).collect()).unwrap_or_default();
    let requested = str_param(p, "name").map(str::to_string);
    let name = s.edit("New Color Group", |d, _| {
        let name = unique_name(d, requested.as_deref().unwrap_or("Color Group"));
        let mut group = SwatchGroup { name: name.clone(), swatches: vec![] };
        for n in &names {
            if let Some(pos) = d.swatches.iter().position(|sw| &sw.name == n) {
                group.swatches.push(d.swatches.remove(pos));
            }
        }
        for c in &colors {
            let [r, g, b] = c.to_rgb();
            let nm = format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round());
            group.swatches.push(drawcraft_color::Swatch { name: nm, paint: Paint::solid(*c), global: false, spot: false });
        }
        d.swatch_groups.push(group);
        Ok(name)
    })?;
    Ok(json!({"name": name}))
}

fn swatch_duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("swatch.duplicate", "missing name"))?.to_string();
    let new = s.edit("Duplicate Swatch", |d, _| {
        let src = d.swatch(&name).cloned().ok_or_else(|| EngineError::Other(format!("no swatch `{name}`")))?;
        let nm = unique_name(d, &format!("{name} copy"));
        let copy = drawcraft_color::Swatch { name: nm.clone(), ..src };
        if let Some(pos) = d.swatches.iter().position(|sw| sw.name == name) {
            d.swatches.insert(pos + 1, copy);
        } else if let Some(g) = d.swatch_groups.iter_mut().find(|g| g.swatches.iter().any(|sw| sw.name == name)) {
            let pos = g.swatches.iter().position(|sw| sw.name == name).unwrap_or(0);
            g.swatches.insert(pos + 1, copy);
        }
        Ok(nm)
    })?;
    Ok(json!({"name": new}))
}

fn swatch_sort(s: &mut Session, _: &Value) -> Result<Value> {
    s.edit("Sort Swatches", |d, _| {
        // [None] and other bracketed specials stay first, like Illustrator.
        let key = |sw: &drawcraft_color::Swatch| (!sw.name.starts_with('['), sw.name.to_lowercase());
        d.swatches.sort_by_key(key);
        for g in &mut d.swatch_groups {
            g.swatches.sort_by_key(key);
        }
        Ok(())
    })?;
    ok()
}

// ---------- graphic styles ----------

fn style_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("graphicStyle.delete", "missing name"))?.to_string();
    s.edit("Delete Graphic Style", |d, _| {
        let before = d.graphic_styles.len();
        d.graphic_styles.retain(|g| g.name != name);
        if d.graphic_styles.len() == before { Err(EngineError::Other(format!("no graphic style `{name}`"))) } else { Ok(()) }
    })?;
    ok()
}

fn style_duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("graphicStyle.duplicate", "missing name"))?.to_string();
    let new = s.edit("Duplicate Graphic Style", |d, _| {
        let pos = d.graphic_styles.iter().position(|g| g.name == name).ok_or_else(|| EngineError::Other(format!("no graphic style `{name}`")))?;
        let mut g = d.graphic_styles[pos].clone();
        let base = format!("{name} copy");
        let mut nm = base.clone();
        let mut i = 2;
        while d.graphic_styles.iter().any(|x| x.name == nm) {
            nm = format!("{base} {i}");
            i += 1;
        }
        g.name = nm.clone();
        d.graphic_styles.insert(pos + 1, g);
        Ok(nm)
    })?;
    Ok(json!({"name": new}))
}

// ---------- text ----------

fn set_format(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "text.setFormat";
    let ids: Vec<NodeId> = {
        let ids = targets(s, p)?;
        let d = &s.doc()?.doc;
        ids.into_iter().filter(|id| matches!(d.node(*id).map(|n| &n.kind), Some(NodeKind::Text(_)))).collect()
    };
    if ids.is_empty() {
        return Err(bad(C, "no text objects selected"));
    }
    let num = |k: &str| p.get(k).and_then(Value::as_f64);
    let flag = |k: &str| p.get(k).and_then(Value::as_bool);
    let kerning = match p.get("kerning") {
        None | Some(Value::Null) => None,
        Some(Value::String(a)) if a.eq_ignore_ascii_case("auto") => Some(None),
        Some(v) => Some(Some(v.as_f64().ok_or_else(|| bad(C, "kerning must be a number or \"auto\""))?.clamp(-1000.0, 10000.0))),
    };
    let keys = [
        "kerning",
        "baselineShift",
        "hScale",
        "vScale",
        "rotation",
        "underline",
        "strikethrough",
        "allCaps",
        "leftIndent",
        "rightIndent",
        "firstLineIndent",
        "spaceBefore",
        "spaceAfter",
        "hyphenate",
    ];
    if !keys.iter().any(|k| p.get(*k).is_some()) {
        return Err(bad(C, "nothing to change"));
    }
    s.edit("Character", |d, _| {
        for id in &ids {
            let Some(NodeKind::Text(t)) = d.node_mut(*id).map(|n| &mut n.kind) else { continue };
            for r in &mut t.runs {
                let st = &mut r.style;
                if let Some(k) = kerning {
                    st.kerning = k;
                }
                if let Some(v) = num("baselineShift") {
                    st.baseline_shift = v.clamp(-1296.0, 1296.0);
                }
                if let Some(v) = num("hScale") {
                    st.h_scale = v.clamp(1.0, 10000.0);
                }
                if let Some(v) = num("vScale") {
                    st.v_scale = v.clamp(1.0, 10000.0);
                }
                if let Some(v) = num("rotation") {
                    st.rotation = ((v + 180.0).rem_euclid(360.0)) - 180.0;
                }
                if let Some(v) = flag("underline") {
                    st.underline = v;
                }
                if let Some(v) = flag("strikethrough") {
                    st.strikethrough = v;
                }
                if let Some(v) = flag("allCaps") {
                    st.all_caps = v;
                }
            }
            let para = &mut t.para;
            if let Some(v) = num("leftIndent") {
                para.left_indent = v;
            }
            if let Some(v) = num("rightIndent") {
                para.right_indent = v;
            }
            if let Some(v) = num("firstLineIndent") {
                para.first_line_indent = v;
            }
            if let Some(v) = num("spaceBefore") {
                para.space_before = v;
            }
            if let Some(v) = num("spaceAfter") {
                para.space_after = v;
            }
            if let Some(v) = flag("hyphenate") {
                para.hyphenate = v;
            }
            super::typecmd::refresh_bounds(t);
        }
        Ok(())
    })?;
    Ok(json!({ "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>() }))
}

// ---------- stroke extras ----------

/// Mirror a width profile along the path (t → 1 − t) or across it (swap left/right widths).
pub(crate) fn flip_profile(p: &drawcraft_doc::WidthProfile, along: bool) -> drawcraft_doc::WidthProfile {
    let mut pts: Vec<(f64, f64, f64)> =
        if along { p.points.iter().map(|&(t, l, r)| (1.0 - t, l, r)).collect() } else { p.points.iter().map(|&(t, l, r)| (t, r, l)).collect() };
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    drawcraft_doc::WidthProfile { points: pts }
}

fn stroke_advanced(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "stroke.setAdvanced";
    let scale = match p.get("arrowScale") {
        None => None,
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(C, "arrowScale must be [start, end]"))?;
            let f = |i: usize| a[i].as_f64().map(|x| x.clamp(1.0, 1000.0));
            Some((f(0).ok_or_else(|| bad(C, "bad arrowScale"))?, f(1).ok_or_else(|| bad(C, "bad arrowScale"))?))
        }
    };
    let flip = match str_param(p, "flipProfile") {
        None => None,
        Some("along") => Some(true),
        Some("across") => Some(false),
        Some(o) => return Err(bad(C, format!("flipProfile must be along|across, got {o}"))),
    };
    let swap = bool_or(p, "swapArrows", false);
    let brush = match p.get("brush") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(b)) => Some(Some(b.clone())),
        Some(_) => return Err(bad(C, "brush must be a name or null")),
    };
    if scale.is_none() && flip.is_none() && !swap && brush.is_none() {
        return Err(bad(C, "nothing to change"));
    }
    let ids = leaf_targets(s, p)?;
    if ids.is_empty() {
        return ok();
    }
    s.edit("Stroke", |d, _| {
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { continue };
            let Some(st) = n.appearance.stroke_mut() else { continue };
            if let Some(sc) = scale {
                st.arrow_scale = sc;
            }
            if swap {
                std::mem::swap(&mut st.start_arrow, &mut st.end_arrow);
                st.arrow_scale = (st.arrow_scale.1, st.arrow_scale.0);
            }
            if let (Some(along), Some(pr)) = (flip, &st.profile) {
                st.profile = Some(flip_profile(pr, along));
            }
            if let Some(b) = &brush {
                st.brush = b.clone();
            }
        }
        Ok(())
    })?;
    ok()
}
