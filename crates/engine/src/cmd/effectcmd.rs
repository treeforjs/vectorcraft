//! Effect menu commands (live effects) and Object → Expand Appearance.
//!
//! Effects are stored on the object's appearance (`appearance.effects`) and evaluated at render
//! time by `vectorcraft-effects` (re-exported by the renderer).

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_doc::{Node, NodeKind};
use vectorcraft_geom::{FillRule, PathData};
use vectorcraft_render::effects;

use super::edit::selected_roots;
use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "effect.apply",
            "Apply Effect",
            [],
            None,
            "{effect: id (see effect.list, e.g. \"stylize.dropShadow\", \"distort.roughen\", \"warp.arc\"), params?: {…} (missing keys take the dialog defaults), ids?: [..]} append a live effect to each selected object's appearance → {ids, index}",
            has_selection,
            apply
        ),
        cmd!(query "effect.list", "Effects", [], None, "{} → {catalog: [{id, label, menu, params, defaults, raster}], applied: [{id, effects}]} for the selection", always, list),
        cmd!(
            "effect.remove",
            "Remove Effect",
            [],
            None,
            "{index: int (position in the object's effect list), ids?: [..]} → {ids}",
            has_selection,
            remove
        ),
        cmd!(
            "effect.setParams",
            "Effect Options",
            [],
            None,
            "{index: int, params?: {…} (merged into the current parameters), visible?: bool, ids?: [..]} → {ids}",
            has_selection,
            set_params
        ),
        cmd!(
            "effect.expandAppearance",
            "Expand Appearance",
            ["Object"],
            None,
            "{ids?: [..]} bake geometry effects into the paths and drop them (raster effects stay live) → {ids}",
            has_selection,
            expand_appearance
        ),
    ]
}

fn target_roots(s: &Session, p: &Value) -> Result<Vec<NodeId>> {
    if p.get("ids").is_some() || p.get("id").is_some() {
        return targets(s, p);
    }
    selected_roots(s)
}

fn ids_json(ids: &[NodeId]) -> Value {
    json!(ids.iter().map(|i| i.0).collect::<Vec<_>>())
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "effect.apply";
    let id = str_param(p, "effect").or_else(|| str_param(p, "id")).ok_or_else(|| bad(C, "missing `effect` id"))?;
    let params = p.get("params").cloned().unwrap_or(Value::Null);
    if !params.is_null() && !params.is_object() {
        return Err(bad(C, "`params` must be an object"));
    }
    let effect = effects::new_effect(id, &params).ok_or_else(|| bad(C, format!("unknown effect `{id}`")))?;
    let roots = target_roots(s, p)?;
    let label = effects::effect_info(id).map(|e| e.label.trim_end_matches('…').to_string()).unwrap_or_default();
    let (ids, index) = s.edit(&label, |d, _| {
        let mut done = vec![];
        let mut index = 0;
        for id in &roots {
            let Some(n) = d.node_mut(*id) else { continue };
            if n.is_layer() {
                continue;
            }
            n.appearance.effects.push(effect.clone());
            index = n.appearance.effects.len() - 1;
            done.push(*id);
        }
        if done.is_empty() {
            return Err(EngineError::Other("Apply Effect: select objects".into()));
        }
        Ok((done, index))
    })?;
    Ok(json!({ "ids": ids_json(&ids), "index": index }))
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let catalog: Vec<Value> = effects::effect_catalog()
        .into_iter()
        .map(|e| json!({"id": e.id, "label": e.label, "menu": e.menu, "params": e.params, "defaults": e.defaults, "raster": e.raster}))
        .collect();
    let mut applied = vec![];
    if s.active().is_some() {
        for id in target_roots(s, p)? {
            if let Some(n) = s.doc()?.doc.node(id) {
                applied.push(json!({"id": id.0, "effects": serde_json::to_value(&n.appearance.effects).unwrap_or(Value::Null)}));
            }
        }
    }
    Ok(json!({ "catalog": catalog, "applied": applied }))
}

fn index_param(p: &Value, cmd: &str) -> Result<usize> {
    p.get("index").and_then(Value::as_u64).map(|i| i as usize).ok_or_else(|| bad(cmd, "missing integer `index`"))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "effect.remove";
    let index = index_param(p, C)?;
    let roots = target_roots(s, p)?;
    let ids = s.edit("Remove Effect", |d, _| {
        let mut done = vec![];
        for id in &roots {
            let Some(n) = d.node_mut(*id) else { continue };
            if index < n.appearance.effects.len() {
                n.appearance.effects.remove(index);
                done.push(*id);
            }
        }
        if done.is_empty() {
            return Err(bad(C, format!("no effect at index {index}")));
        }
        Ok(done)
    })?;
    Ok(json!({ "ids": ids_json(&ids) }))
}

fn set_params(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "effect.setParams";
    let index = index_param(p, C)?;
    let params = p.get("params").cloned().unwrap_or(Value::Null);
    if !params.is_null() && !params.is_object() {
        return Err(bad(C, "`params` must be an object"));
    }
    let visible = p.get("visible").and_then(Value::as_bool);
    let roots = target_roots(s, p)?;
    let ids = s.edit("Effect Options", |d, _| {
        let mut done = vec![];
        for id in &roots {
            let Some(n) = d.node_mut(*id) else { continue };
            let Some(e) = n.appearance.effects.get_mut(index) else { continue };
            if let (Value::Object(new), cur) = (&params, &mut e.params) {
                if !cur.is_object() {
                    *cur = effects::merged_params(&e.id, &Value::Null);
                }
                if let Value::Object(m) = cur {
                    for (k, v) in new {
                        m.insert(k.clone(), v.clone());
                    }
                }
            }
            if let Some(v) = visible {
                e.visible = v;
            }
            done.push(*id);
        }
        if done.is_empty() {
            return Err(bad(C, format!("no effect at index {index}")));
        }
        Ok(done)
    })?;
    Ok(json!({ "ids": ids_json(&ids) }))
}

/// Geometry of a path or compound path node.
fn node_geometry(n: &Node) -> Option<(PathData, FillRule)> {
    match &n.kind {
        NodeKind::Path { path, rule, guide: false, .. } => Some((path.clone(), *rule)),
        NodeKind::Compound { children, rule } => {
            Some((PathData::new(children.iter().filter_map(|c| c.path_data()).flat_map(|p| p.subpaths.iter().cloned()).collect()), *rule))
        }
        _ => None,
    }
}

/// Bake the object-level geometry effects of `id` (and, recursively, of group members).
fn expand_node(d: &mut vectorcraft_doc::Document, id: NodeId, out: &mut Vec<NodeId>) {
    let Some(n) = d.node(id).cloned() else { return };
    // Effect → Pathfinder: the group's members become the Pathfinder result.
    if let Some(result) = effects::pathfinder_children(&n, None) {
        let children = result
            .into_iter()
            .map(|c| {
                let mut c = Arc::unwrap_or_clone(c);
                c.id = d.alloc_id();
                Arc::new(c)
            })
            .collect();
        let Some(m) = d.node_mut(id) else { return };
        if let Some(ch) = m.children_mut() {
            *ch = children;
        }
        m.appearance.effects.retain(|e| !effects::is_pathfinder(&e.id));
        out.push(id);
        return;
    }
    if let Some(children) = n.children()
        && matches!(n.kind, NodeKind::Group { .. })
    {
        for c in children.iter().map(|c| c.id).collect::<Vec<_>>() {
            expand_node(d, c, out);
        }
        return;
    }
    if !effects::has_geometry(&n.appearance.effects) {
        return;
    }
    let Some((path, rule)) = node_geometry(&n) else { return };
    let Some(bounds) = path.bounds() else { return };
    let w = n.appearance.stroke_width();
    let ctx = effects::GeomContext { stroke_width: if w > 0.0 { w } else { 1.0 } };
    let baked = effects::apply_geometry_with(&n.appearance.effects, &path, bounds, &ctx);
    let kind = if matches!(n.kind, NodeKind::Compound { .. }) || baked.subpaths.len() > 1 {
        let children = baked
            .subpaths
            .into_iter()
            .map(|sp| {
                let cid = d.alloc_id();
                Arc::new(Node::path(cid, PathData::single(sp), Default::default()))
            })
            .collect();
        NodeKind::Compound { children, rule }
    } else {
        NodeKind::Path { path: baked, rule, live: None, clipping: false, guide: false }
    };
    let Some(m) = d.node_mut(id) else { return };
    m.kind = kind;
    m.appearance.effects.retain(|e| !effects::is_geometry(&e.id));
    out.push(id);
}

fn expand_appearance(s: &mut Session, p: &Value) -> Result<Value> {
    let roots = target_roots(s, p)?;
    let ids = s.edit("Expand Appearance", |d, _| {
        let mut out = vec![];
        for id in &roots {
            expand_node(d, *id, &mut out);
        }
        if out.is_empty() {
            return Err(EngineError::Other("Expand Appearance: no geometry effects to expand".into()));
        }
        Ok(out)
    })?;
    Ok(json!({ "ids": ids_json(&ids) }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_doc::NodeKind;

    use crate::{NodeId, Session};

    fn session_with_rect() -> (Session, NodeId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 400, "height": 400})).unwrap();
        let r = s.execute("shape.rectangle", &json!({"x": 100, "y": 100, "width": 100, "height": 100})).unwrap();
        (s, NodeId(r["id"].as_u64().unwrap()))
    }

    fn node(s: &Session, id: NodeId) -> vectorcraft_doc::Node {
        s.doc().unwrap().doc.node(id).cloned().unwrap()
    }

    #[test]
    fn apply_uses_catalog_defaults_and_undoes() {
        let (mut s, id) = session_with_rect();
        let r = s.execute("effect.apply", &json!({"effect": "stylize.dropShadow", "params": {"x": 3}})).unwrap();
        assert_eq!(r["index"], json!(0));
        let n = node(&s, id);
        assert_eq!(n.appearance.effects.len(), 1);
        assert_eq!(n.appearance.effects[0].params["x"], json!(3));
        assert_eq!(n.appearance.effects[0].params["opacity"], json!(75.0));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(node(&s, id).appearance.effects.is_empty());
        assert!(s.execute("effect.apply", &json!({"effect": "nope"})).is_err());
    }

    #[test]
    fn list_remove_and_set_params() {
        let (mut s, id) = session_with_rect();
        s.execute("effect.apply", &json!({"effect": "distort.twist"})).unwrap();
        s.execute("effect.apply", &json!({"effect": "stylize.outerGlow"})).unwrap();
        let l = s.execute("effect.list", &json!({})).unwrap();
        assert!(l["catalog"].as_array().unwrap().len() >= 34);
        assert_eq!(l["applied"][0]["effects"].as_array().unwrap().len(), 2);
        s.execute("effect.setParams", &json!({"index": 0, "params": {"angle": 45}, "visible": false})).unwrap();
        let e = &node(&s, id).appearance.effects[0];
        assert_eq!(e.params["angle"], json!(45));
        assert!(!e.visible);
        s.execute("effect.remove", &json!({"index": 0})).unwrap();
        let n = node(&s, id);
        assert_eq!(n.appearance.effects.len(), 1);
        assert_eq!(n.appearance.effects[0].id, "stylize.outerGlow");
        assert!(s.execute("effect.remove", &json!({"index": 5})).is_err());
    }

    #[test]
    fn expand_appearance_bakes_geometry_and_keeps_raster() {
        let (mut s, id) = session_with_rect();
        s.execute("effect.apply", &json!({"effect": "path.offsetPath", "params": {"offset": 10}})).unwrap();
        s.execute("effect.apply", &json!({"effect": "stylize.dropShadow"})).unwrap();
        s.execute("effect.expandAppearance", &json!({})).unwrap();
        let n = node(&s, id);
        let b = n.geometric_bounds().unwrap();
        assert!((b.width() - 120.0).abs() < 0.1, "{b:?}");
        assert_eq!(n.appearance.effects.len(), 1);
        assert_eq!(n.appearance.effects[0].id, "stylize.dropShadow");
        assert!(matches!(n.kind, NodeKind::Path { live: None, .. }));
        // Nothing left to expand.
        assert!(s.execute("effect.expandAppearance", &json!({})).is_err());
    }

    #[test]
    fn expand_transform_copies_makes_compound() {
        let (mut s, id) = session_with_rect();
        s.execute("effect.apply", &json!({"effect": "distort.transform", "params": {"moveH": 120, "copies": 1}})).unwrap();
        s.execute("effect.expandAppearance", &json!({})).unwrap();
        let n = node(&s, id);
        assert!(matches!(n.kind, NodeKind::Compound { .. }));
        let b = n.geometric_bounds().unwrap();
        assert!((b.x1 - 320.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn pathfinder_effect_on_a_group_renders_live_and_expands() {
        let (mut s, a) = session_with_rect();
        let b = s.execute("shape.rectangle", &json!({"x": 150, "y": 100, "width": 100, "height": 100})).unwrap();
        s.execute("select.set", &json!({"ids": [a.0, b["id"]]})).unwrap();
        let g = NodeId(s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap());
        s.execute("effect.apply", &json!({"effect": "pathfinder.subtract"})).unwrap();
        // Live: the renderer shows only the back square minus the front one.
        let doc = s.doc().unwrap().doc.clone();
        let img = vectorcraft_render::Renderer::new().render(&doc, 400, 400, vectorcraft_geom::Affine::IDENTITY, &Default::default());
        assert!(img.pixel(125, 150)[3] > 0);
        assert_eq!(img.pixel(200, 150)[3], 0);
        s.execute("effect.expandAppearance", &json!({})).unwrap();
        let n = node(&s, g);
        assert!(n.appearance.effects.is_empty());
        let ch = n.children().unwrap();
        assert_eq!(ch.len(), 1);
        assert!((ch[0].geometric_bounds().unwrap().width() - 50.0).abs() < 1e-6);
    }
}
