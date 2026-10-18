//! Live compound shapes: Pathfinder › Shape Modes with Alt (Make Compound Shape), Release, Expand
//! and per-operand shape modes. The group keeps its operands, editable, in an invisible first
//! child and draws the generated result as its second; edits to the operands re-evaluate the
//! result in the same undo transaction (see [`refresh`]).
use super::edit::selected_roots;
use super::pathops::{node_path, shape_node};
use super::*;
use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_doc::compound::{CompoundShape, OPERANDS, RESULT, ShapeMode};
use vectorcraft_doc::{Document, Node, NodeId, NodeKind};
use vectorcraft_geom::{FillRule, PathData};
use vectorcraft_pathops as po;

const MAX_OPERANDS: usize = 256;
/// Compound shapes nest (an operand may be a compound shape or a group); deeper art is ignored.
const MAX_DEPTH: usize = 32;
/// The most anchors evaluated per compound shape.
const MAX_ANCHORS: usize = 100_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "object.compoundShape.make",
            "Make Compound Shape",
            [],
            None,
            "{mode?: add|subtract|intersect|exclude (add)} combine the selected objects (2–256, one layer or group) into a live compound shape: the topmost object takes `mode`, the result follows edits to the objects (Alt-click a Shape Mode in the Pathfinder panel). With a compound shape or some of its operands selected, sets their mode instead → {id}",
            has_selection,
            make
        ),
        cmd!(
            "object.compoundShape.setMode",
            "Set Shape Mode",
            [],
            None,
            "{mode: add|subtract|intersect|exclude, ids?: [id…] (default: the selection)} operands take `mode`; a selected compound shape sets every operand above its bottom one → {ids}",
            has_selection,
            set_mode
        ),
        cmd!(
            "object.compoundShape.release",
            "Release Compound Shape",
            [],
            None,
            "{id? (default: the selected compound shape)} put the operands back as separate objects → {ids}",
            has_selection,
            release
        ),
        cmd!(
            "object.compoundShape.expand",
            "Expand Compound Shape",
            [],
            None,
            "{id? (default: the selected compound shape)} keep the result as an ordinary path and drop the operands → {id}",
            has_selection,
            expand
        ),
    ]
}

fn operand_group(n: &Node) -> Option<&Node> {
    n.compound.as_ref()?;
    n.children()?.first().map(AsRef::as_ref).filter(|g| g.name.as_deref() == Some(OPERANDS))
}
/// The operands of a compound shape, back to front.
pub(crate) fn operands(n: &Node) -> Vec<Arc<Node>> {
    operand_group(n).and_then(Node::children).cloned().unwrap_or_default()
}

/// The area an operand contributes: paths, compound paths and text as they are, containers as
/// the union of their visible members. Hidden objects, guides and clipping paths add nothing.
fn operand_path(n: &Node, depth: usize, budget: &mut usize) -> Option<(PathData, FillRule)> {
    if !n.visible || depth > MAX_DEPTH {
        return None;
    }
    let shape = match &n.kind {
        NodeKind::Path { guide: true, .. } | NodeKind::Path { clipping: true, .. } => None,
        NodeKind::Path { .. } | NodeKind::Compound { .. } | NodeKind::Text(_) => node_path(n),
        NodeKind::Group { children, .. } | NodeKind::Layer { children, .. } => {
            let parts: Vec<(PathData, FillRule)> =
                children.iter().skip(usize::from(n.retains_sources())).filter_map(|c| operand_path(c, depth + 1, budget)).collect();
            if parts.is_empty() {
                None
            } else if let [one] = parts.as_slice() {
                Some(one.clone())
            } else {
                let refs: Vec<(&PathData, FillRule)> = parts.iter().map(|(p, r)| (p, *r)).collect();
                Some((po::unite_all(&refs), FillRule::NonZero))
            }
        }
        _ => None,
    }?;
    *budget = budget.checked_sub(shape.0.anchor_count())?;
    let finite = shape
        .0
        .subpaths
        .iter()
        .flat_map(|s| &s.anchors)
        .all(|a| [a.p, a.h_in, a.h_out].iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 1e9 && p.y.abs() <= 1e9));
    finite.then_some(shape)
}

/// The paint a compound shape takes: the topmost operand that adds to it (or the bottom one).
fn style_operand(ops: &[Arc<Node>], spec: &CompoundShape) -> Option<usize> {
    (1..ops.len())
        .rev()
        .find(|i| spec.mode(*i) != ShapeMode::Subtract && ops.get(*i).is_some_and(|n| n.visible))
        .or(Some(0))
        .filter(|_| !ops.is_empty())
}
/// The leaf whose paint a container operand lends (its topmost visible leaf).
fn paint_leaf(n: &Node, depth: usize) -> Option<&Node> {
    if depth > MAX_DEPTH || !n.visible {
        return None;
    }
    match &n.kind {
        NodeKind::Path { guide: false, clipping: false, .. } | NodeKind::Compound { .. } | NodeKind::Text(_) => Some(n),
        NodeKind::Group { children, .. } | NodeKind::Layer { children, .. } => {
            children.iter().skip(usize::from(n.retains_sources())).rev().find_map(|c| paint_leaf(c, depth + 1))
        }
        _ => None,
    }
}

/// The result path of `ops` combined with `spec`'s modes, with id `id`.
fn evaluate(d: &mut Document, id: Option<NodeId>, ops: &[Arc<Node>], spec: &CompoundShape) -> Result<Node> {
    let mut budget = MAX_ANCHORS;
    let mut shapes = vec![];
    let mut modes = vec![];
    for (i, n) in ops.iter().enumerate() {
        if let Some(shape) = operand_path(n, 0, &mut budget) {
            shapes.push(shape);
            modes.push(spec.mode(i));
        }
        if budget == 0 {
            return Err(EngineError::Other("Compound Shape: these objects have too many anchor points".into()));
        }
    }
    let refs: Vec<(&PathData, FillRule)> = shapes.iter().map(|(p, r)| (p, *r)).collect();
    let path = if refs.is_empty() {
        PathData::default()
    } else {
        po::boolean_n(&refs, |inside| {
            let mut acc = inside.first().copied().unwrap_or(false);
            for (i, &here) in inside.iter().enumerate().skip(1) {
                acc = match modes.get(i).copied().unwrap_or_default() {
                    ShapeMode::Add => acc || here,
                    ShapeMode::Subtract => acc && !here,
                    ShapeMode::Intersect => acc && here,
                    ShapeMode::Exclude => acc != here,
                };
            }
            acc
        })
    };
    let mut n = shape_node(d, path, None);
    if let Some(id) = id {
        n.id = id;
    }
    n.name = Some(RESULT.into());
    if let Some(src) = style_operand(ops, spec).and_then(|i| ops.get(i)) {
        let leaf = paint_leaf(src, 0).unwrap_or(src);
        n.appearance = match &leaf.kind {
            NodeKind::Text(t) => t.first_style().appearance(),
            _ => leaf.appearance.clone(),
        };
        n.opacity = leaf.opacity;
        n.blend = leaf.blend;
    }
    Ok(n)
}

fn compound_node(d: &mut Document, ops: Vec<Arc<Node>>, spec: CompoundShape) -> Result<Node> {
    let result = evaluate(d, None, &ops, &spec)?;
    let mut source = Node::group(d.alloc_id(), ops);
    source.name = Some(OPERANDS.into());
    source.opacity = 0.0;
    let mut group = Node::group(d.alloc_id(), vec![Arc::new(source), Arc::new(result)]);
    group.compound = Some(Box::new(spec));
    Ok(group)
}

fn mode_param(p: &Value, c: &str, default: Option<ShapeMode>) -> Result<ShapeMode> {
    match str_param(p, "mode") {
        Some(m) => ShapeMode::from_id(m).ok_or_else(|| bad(c, "mode must be add, subtract, intersect or exclude")),
        None => default.ok_or_else(|| bad(c, "missing mode")),
    }
}

/// `(compound shape, operand index)` for an operand id.
fn operand_of(d: &Document, id: NodeId) -> Option<(NodeId, usize)> {
    let (Some(parent), index, _) = d.position(id)? else { return None };
    let group = d.parent_of(parent)?;
    let n = d.node(group)?;
    (operand_group(n).map(|g| g.id) == Some(parent)).then_some((group, index))
}

fn make(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "object.compoundShape.make";
    let mode = mode_param(p, C, Some(ShapeMode::Add))?;
    let roots = selected_roots(s)?;
    {
        // A selected compound shape, or operands of one, take the mode instead.
        let d = &s.doc()?.doc;
        if roots.iter().all(|id| d.node(*id).is_some_and(|n| n.compound.is_some()) || operand_of(d, *id).is_some()) && !roots.is_empty() {
            let ids: Vec<u64> = roots.iter().map(|i| i.0).collect();
            set_mode(s, &json!({"mode": mode.id(), "ids": ids}))?;
            return Ok(json!({"id": roots.first().map(|i| i.0)}));
        }
    }
    if roots.len() < 2 || roots.len() > MAX_OPERANDS {
        return Err(bad(C, "select 2–256 objects"));
    }
    let d = &s.doc()?.doc;
    let parent = roots.first().and_then(|id| d.position(*id)).map(|p| p.0);
    if roots.iter().any(|id| d.position(*id).map(|p| p.0) != parent) {
        return Err(bad(C, "the objects must be in the same layer or group"));
    }
    let id = s.edit("Make Compound Shape", |d, sel| {
        let top = *roots.last().ok_or_else(|| bad(C, "nothing selected"))?;
        let (parent, index, _) = d.position(top).ok_or(EngineError::NoNode(top))?;
        let earlier = roots.iter().filter(|id| **id != top && d.position(**id).is_some_and(|p| p.1 < index)).count();
        let ops = roots.iter().map(|id| d.node(*id).cloned().map(Arc::new).ok_or(EngineError::NoNode(*id))).collect::<Result<Vec<_>>>()?;
        let spec = CompoundShape { modes: (0..ops.len()).map(|i| if i == 0 { ShapeMode::Add } else { mode }).collect() };
        let group = compound_node(d, ops, spec)?;
        for id in &roots {
            d.remove(*id)?;
        }
        let id = d.insert(parent, index.saturating_sub(earlier), group)?;
        sel.set([id]);
        Ok(id)
    })?;
    Ok(json!({"id": id.0}))
}

fn set_mode(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "object.compoundShape.setMode";
    let mode = mode_param(p, C, None)?;
    let ids = match ids_param(p, "ids") {
        Some(ids) => ids,
        None => selected_roots(s)?,
    };
    let d = &s.doc()?.doc;
    let mut edits: Vec<(NodeId, usize)> = vec![];
    for id in &ids {
        if let Some(n) = d.node(*id).filter(|n| n.compound.is_some()) {
            edits.extend((1..operands(n).len()).map(|i| (*id, i)));
        } else {
            edits.push(operand_of(d, *id).ok_or_else(|| bad(C, "select a compound shape or its operands"))?);
        }
    }
    if edits.is_empty() {
        return Err(bad(C, "select a compound shape or its operands"));
    }
    s.edit("Shape Mode", |d, _| {
        for (group, i) in &edits {
            let n = d.node_mut(*group).ok_or(EngineError::NoNode(*group))?;
            let count = operands(n).len();
            let spec = n.compound.get_or_insert_with(Default::default);
            spec.modes.resize(count.max(spec.modes.len()), ShapeMode::Add);
            if let Some(m) = spec.modes.get_mut(*i) {
                *m = mode;
            }
        }
        Ok(())
    })?;
    Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
}

fn target(s: &Session, p: &Value, c: &str) -> Result<NodeId> {
    let d = &s.doc()?.doc;
    let id = match id_param(p, "id") {
        Some(id) => id,
        None => {
            let roots = selected_roots(s)?;
            roots
                .iter()
                .find(|id| d.node(**id).is_some_and(|n| n.compound.is_some()))
                .or_else(|| roots.first())
                .map(|id| operand_of(d, *id).map_or(*id, |o| o.0))
                .ok_or_else(|| bad(c, "select a compound shape"))?
        }
    };
    d.node(id).filter(|n| n.compound.is_some()).ok_or_else(|| bad(c, "select a compound shape"))?;
    Ok(id)
}

fn release(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "object.compoundShape.release")?;
    let ids = s.edit("Release Compound Shape", |d, sel| {
        let ops = d.node(id).map(operands).ok_or(EngineError::NoNode(id))?;
        let (parent, index, _) = d.position(id).ok_or(EngineError::NoNode(id))?;
        d.remove(id)?;
        let mut ids = vec![];
        for (i, n) in ops.into_iter().enumerate() {
            ids.push(d.insert(parent, index.saturating_add(i), (*n).clone())?);
        }
        sel.set(ids.iter().copied());
        Ok(ids)
    })?;
    leave_operands(s, id)?;
    Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
}

fn expand(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "object.compoundShape.expand")?;
    let rid = s.edit("Expand Compound Shape", |d, sel| {
        let n = d.node(id).ok_or(EngineError::NoNode(id))?;
        let mut result = n.children().and_then(|c| c.get(1)).map(|r| (**r).clone()).ok_or(EngineError::NoNode(id))?;
        result.name = None;
        let (parent, index, _) = d.position(id).ok_or(EngineError::NoNode(id))?;
        d.remove(id)?;
        let rid = d.insert(parent, index, result)?;
        sel.set([rid]);
        Ok(rid)
    })?;
    leave_operands(s, id)?;
    Ok(json!({"id": rid.0}))
}

/// Isolation inside a compound shape that no longer exists ends.
fn leave_operands(s: &mut Session, _group: NodeId) -> Result<()> {
    let st = s.doc_mut()?;
    if st.isolation.is_some_and(|i| st.doc.node(i).is_none()) {
        st.isolation = None;
    }
    Ok(())
}

/// Re-evaluate the compound shapes whose operands or modes changed. Errors reach
/// `Session::edit`'s rollback.
pub(crate) fn refresh(before: &Document, d: &mut Document) -> Result<()> {
    let mut ids = vec![];
    d.walk(|n| {
        if n.compound.is_some() {
            ids.push(n.id)
        }
    });
    // Innermost first, so a nested compound shape is current before the one that contains it.
    ids.reverse();
    for id in ids {
        let Some(current) = d.node(id).cloned() else { continue };
        let ops = operands(&current);
        let result = current.children().and_then(|c| c.get(1)).cloned();
        let old = before.node(id);
        let unchanged = old.is_some_and(|o| o.compound == current.compound && operands(o) == ops);
        if unchanged {
            // Painting the result directly paints the operand it takes its look from.
            let (Some(o), Some(r)) = (old, result.as_ref()) else { continue };
            let Some(prev) = o.children().and_then(|c| c.get(1)) else { continue };
            if prev.appearance == r.appearance && prev.opacity == r.opacity && prev.blend == r.blend {
                continue;
            }
            let spec = current.compound.as_deref().cloned().unwrap_or_default();
            let Some(src) = style_operand(&ops, &spec).and_then(|i| ops.get(i)) else { continue };
            let leaf = paint_leaf(src, 0).map_or(src.id, |l| l.id);
            if let Some(n) = d.node_mut(leaf).filter(|n| !matches!(n.kind, NodeKind::Text(_))) {
                n.appearance = r.appearance.clone();
                n.opacity = r.opacity;
                n.blend = r.blend;
            }
            continue;
        }
        if ops.is_empty() {
            d.remove(id)?;
            continue;
        }
        let spec = current.compound.as_deref().cloned().unwrap_or_default();
        let fresh = evaluate(d, result.as_ref().map(|r| r.id), &ops, &spec)?;
        let n = d.node_mut(id).ok_or(EngineError::NoNode(id))?;
        if let Some(children) = n.children_mut() {
            children.truncate(1);
            children.push(Arc::new(fresh));
        }
    }
    Ok(())
}
