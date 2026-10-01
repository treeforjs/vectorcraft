//! Opacity masks (Transparency panel): make, release, enable/disable, link/unlink, clip, invert.
//!
//! The mask art is stored on the masked object ([`drawcraft_doc::OpacityMask`]), outside the
//! layer tree, so it is never hit-tested or selected. Its luminance sets the object's opacity.

use drawcraft_doc::{Node, NodeId, OpacityMask};
use serde_json::{Value, json};

use super::edit::selected_roots;
use super::*;
use crate::EngineError;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "transparency.makeOpacityMask",
            "Make Opacity Mask",
            ["Window", "Transparency"],
            None,
            "{clip?, invert?} the topmost selected object becomes the mask of the others (grouped if several) → {id}",
            has_multi,
            make
        ),
        cmd!(
            "transparency.releaseOpacityMask",
            "Release Opacity Mask",
            ["Window", "Transparency"],
            None,
            "{} put the mask art back above each masked object",
            has_mask,
            release
        ),
        cmd!("transparency.disableOpacityMask", "Disable Opacity Mask", ["Window", "Transparency"], None, "{}", has_mask, |s, _| set_flags(
            s,
            "Disable Opacity Mask",
            &json!({ "disabled": true })
        )),
        cmd!("transparency.enableOpacityMask", "Enable Opacity Mask", ["Window", "Transparency"], None, "{}", has_mask, |s, _| set_flags(
            s,
            "Enable Opacity Mask",
            &json!({ "disabled": false })
        )),
        cmd!(
            "transparency.unlinkOpacityMask",
            "Unlink Opacity Mask",
            ["Window", "Transparency"],
            None,
            "{} the object moves without its mask",
            has_mask,
            |s, _| set_flags(s, "Unlink Opacity Mask", &json!({ "linked": false }))
        ),
        cmd!("transparency.linkOpacityMask", "Link Opacity Mask", ["Window", "Transparency"], None, "{}", has_mask, |s, _| set_flags(
            s,
            "Link Opacity Mask",
            &json!({ "linked": true })
        )),
        cmd!(
            "transparency.setOpacityMask",
            "Opacity Mask Options",
            [],
            None,
            "{clip?, invert?, disabled?, linked?} change the selected objects' opacity masks",
            has_mask,
            |s, p| set_flags(s, "Opacity Mask Options", p)
        ),
        cmd!(
            "transparency.toggleNewMasksClipping",
            "New Opacity Masks Are Clipping",
            ["Window", "Transparency"],
            None,
            "{value?} → {value}",
            always,
            toggle_new_clip
        ),
        cmd!(
            "transparency.toggleNewMasksInverted",
            "New Opacity Masks Are Inverted",
            ["Window", "Transparency"],
            None,
            "{value?} → {value}",
            always,
            toggle_new_invert
        ),
        cmd!(query "transparency.opacityMaskInfo", "Opacity Mask Info", [], None, "{} → [{id, clip, invert, disabled, linked}] masks of the selected objects", has_doc, info),
    ]
}

fn has_mask(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let st = s.active().unwrap();
    if st.selection.in_paint_order(&st.doc).into_iter().any(|id| st.doc.node(id).is_some_and(|n| n.mask.is_some())) {
        Ok(())
    } else {
        Err("no selected object has an opacity mask".into())
    }
}

fn masked_ids(s: &Session) -> Result<Vec<NodeId>> {
    let st = s.doc()?;
    Ok(selected_roots(s)?.into_iter().filter(|id| st.doc.node(*id).is_some_and(|n| n.mask.is_some())).collect())
}

fn make(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = selected_roots(s)?;
    if ids.len() < 2 {
        return Err(bad("transparency.makeOpacityMask", "select the art and, on top, the mask object"));
    }
    let clip = bool_or(p, "clip", !s.menu.new_masks_unclipped);
    let invert = bool_or(p, "invert", s.menu.new_masks_inverted);
    let (mask_id, art) = (ids[ids.len() - 1], &ids[..ids.len() - 1]);
    let id = s.edit("Make Opacity Mask", |d, sel| {
        let mask_art = (*d.remove(mask_id)?).clone();
        // Several objects are grouped so they share one mask.
        let target = if let [one] = art {
            *one
        } else {
            let last = art[art.len() - 1];
            let (par, idx, _) = d.position(last).ok_or(EngineError::NoNode(last))?;
            let gid = d.alloc_id();
            d.insert(par, idx + 1, Node::group(gid, vec![]))?;
            for id in art {
                d.move_node(*id, Some(gid), usize::MAX)?;
            }
            gid
        };
        let n = d.node_mut(target).ok_or(EngineError::NoNode(target))?;
        if n.mask.is_some() {
            return Err(EngineError::Other("the object already has an opacity mask".into()));
        }
        let mut m = OpacityMask::new(mask_art, clip);
        m.invert = invert;
        n.mask = Some(Box::new(m));
        sel.set([target]);
        Ok(target)
    })?;
    Ok(json!({ "id": id.0 }))
}

fn release(s: &mut Session, _: &Value) -> Result<Value> {
    let ids = masked_ids(s)?;
    s.edit("Release Opacity Mask", |d, sel| {
        let mut out = vec![];
        for id in &ids {
            let Some(m) = d.node_mut(*id).and_then(|n| n.mask.take()) else { continue };
            let (par, idx, _) = d.position(*id).ok_or(EngineError::NoNode(*id))?;
            // Fresh ids: copies of a masked object share the ids inside their mask art.
            let art = d.reid(&m.art);
            out.push(*id);
            out.push(d.insert(par, idx + 1, art)?);
        }
        sel.set(out);
        Ok(())
    })?;
    ok()
}

fn set_flags(s: &mut Session, label: &str, p: &Value) -> Result<Value> {
    let ids = masked_ids(s)?;
    let get = |k: &str| p.get(k).and_then(Value::as_bool);
    let (clip, invert, disabled, linked) = (get("clip"), get("invert"), get("disabled"), get("linked"));
    if clip.is_none() && invert.is_none() && disabled.is_none() && linked.is_none() {
        return Err(bad("transparency.setOpacityMask", "give at least one of clip, invert, disabled, linked"));
    }
    s.edit(label, |d, _| {
        for id in &ids {
            if let Some(m) = d.node_mut(*id).and_then(|n| n.mask.as_mut()) {
                m.clip = clip.unwrap_or(m.clip);
                m.invert = invert.unwrap_or(m.invert);
                m.disabled = disabled.unwrap_or(m.disabled);
                m.linked = linked.unwrap_or(m.linked);
            }
        }
        Ok(())
    })?;
    ok()
}

fn toggle_new_clip(s: &mut Session, p: &Value) -> Result<Value> {
    let v = bool_or(p, "value", s.menu.new_masks_unclipped);
    s.menu.new_masks_unclipped = !v;
    Ok(json!({ "value": v }))
}

fn toggle_new_invert(s: &mut Session, p: &Value) -> Result<Value> {
    let v = bool_or(p, "value", !s.menu.new_masks_inverted);
    s.menu.new_masks_inverted = v;
    Ok(json!({ "value": v }))
}

impl Session {
    /// Defaults for new opacity masks (clip, invert) from the Transparency panel menu.
    pub fn new_mask_defaults(&self) -> (bool, bool) {
        (!self.menu.new_masks_unclipped, self.menu.new_masks_inverted)
    }
}

fn info(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.doc()?;
    let v: Vec<Value> = st
        .selection
        .in_paint_order(&st.doc)
        .into_iter()
        .filter_map(|id| st.doc.node(id))
        .filter_map(|n| {
            let m = n.mask.as_deref()?;
            Some(json!({ "id": n.id.0, "clip": m.clip, "invert": m.invert, "disabled": m.disabled, "linked": m.linked, "art": m.art.id.0 }))
        })
        .collect();
    Ok(json!(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session_with_two() -> (Session, u64, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
        let a = s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 100, "height": 100})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 100})).unwrap()["id"].as_u64().unwrap();
        (s, a, b)
    }

    fn select(s: &mut Session, ids: &[u64]) {
        s.execute("select.set", &json!({ "ids": ids })).unwrap();
    }

    #[test]
    fn make_moves_top_object_into_mask() {
        let (mut s, a, b) = session_with_two();
        select(&mut s, &[a, b]);
        let r = s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
        assert_eq!(r["id"].as_u64(), Some(a));
        let d = &s.doc().unwrap().doc;
        assert!(d.node(NodeId(b)).is_none(), "mask art left the layer tree");
        let m = d.node(NodeId(a)).unwrap().mask.as_deref().unwrap();
        assert!(m.clip && m.linked && !m.invert && !m.disabled);
        assert_eq!(m.art.id, NodeId(b));
    }

    #[test]
    fn make_groups_several_objects() {
        let (mut s, a, b) = session_with_two();
        let c = s.execute("shape.ellipse", &json!({"x": 0, "y": 0, "width": 20, "height": 20})).unwrap()["id"].as_u64().unwrap();
        select(&mut s, &[a, b, c]);
        let g = s.execute("transparency.makeOpacityMask", &json!({"clip": false})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let n = d.node(NodeId(g)).unwrap();
        assert_eq!(n.children().unwrap().len(), 2);
        assert!(!n.mask.as_ref().unwrap().clip);
    }

    #[test]
    fn release_restores_mask_art_and_undo_restores_mask() {
        let (mut s, a, b) = session_with_two();
        select(&mut s, &[a, b]);
        s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
        s.execute("transparency.releaseOpacityMask", &json!({})).unwrap();
        let st = s.doc().unwrap();
        assert!(st.doc.node(NodeId(a)).unwrap().mask.is_none());
        assert_eq!(st.selection.len(), 2);
        assert_eq!(st.doc.layers[0].children().unwrap().len(), 2);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.node(NodeId(a)).unwrap().mask.is_some());
    }

    #[test]
    fn flags_and_linking() {
        let (mut s, a, b) = session_with_two();
        select(&mut s, &[a, b]);
        s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
        s.execute("transparency.setOpacityMask", &json!({"invert": true, "clip": false})).unwrap();
        s.execute("transparency.disableOpacityMask", &json!({})).unwrap();
        s.execute("transparency.unlinkOpacityMask", &json!({})).unwrap();
        let info = s.execute("transparency.opacityMaskInfo", &json!({})).unwrap();
        assert_eq!(info[0]["invert"], true);
        assert_eq!(info[0]["clip"], false);
        assert_eq!(info[0]["disabled"], true);
        assert_eq!(info[0]["linked"], false);
        // Unlinked: moving the object leaves the mask art where it was.
        let before = s.doc().unwrap().doc.node(NodeId(a)).unwrap().mask.as_ref().unwrap().art.clone();
        s.execute("object.move", &json!({"dx": 30, "dy": 0})).unwrap();
        let after = s.doc().unwrap().doc.node(NodeId(a)).unwrap().mask.as_ref().unwrap().art.clone();
        assert_eq!(before.geometric_bounds(), after.geometric_bounds());
        s.execute("transparency.linkOpacityMask", &json!({})).unwrap();
        s.execute("object.move", &json!({"dx": 30, "dy": 0})).unwrap();
        let moved = s.doc().unwrap().doc.node(NodeId(a)).unwrap().mask.as_ref().unwrap().art.clone();
        assert!((moved.geometric_bounds().unwrap().x0 - after.geometric_bounds().unwrap().x0 - 30.0).abs() < 1e-9);
    }

    #[test]
    fn new_masks_clipping_toggle() {
        let (mut s, a, b) = session_with_two();
        assert_eq!(s.execute("transparency.toggleNewMasksClipping", &json!({})).unwrap()["value"], false);
        select(&mut s, &[a, b]);
        s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
        assert!(!s.doc().unwrap().doc.node(NodeId(a)).unwrap().mask.as_ref().unwrap().clip);
        s.execute("transparency.toggleNewMasksInverted", &json!({})).unwrap();
        assert_eq!(s.new_mask_defaults(), (false, true));
    }

    #[test]
    fn masks_survive_native_round_trip() {
        let (mut s, a, b) = session_with_two();
        select(&mut s, &[a, b]);
        s.execute("transparency.makeOpacityMask", &json!({"invert": true})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let bytes = drawcraft_format::save(&d, false);
        let back = drawcraft_format::load(&bytes).unwrap();
        assert_eq!(back.node(NodeId(a)).unwrap().mask, d.node(NodeId(a)).unwrap().mask);
    }
}
