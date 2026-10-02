//! Opacity-mask editing mode (click the mask thumbnail in the Transparency panel).
//!
//! The mask art moves onto a temporary isolated layer where every tool and command can edit it.
//! After each edit [`sync`] writes the layer's art back into the object's mask, so the masked
//! object updates live; the renderer doesn't paint the editing layer (the mask is seen through its
//! effect, as in Illustrator). Leaving the mode removes the layer.

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_doc::{Document, MaskEdit, Node, NodeId};

use super::*;

/// Name of the temporary mask-editing layer.
pub const MASK_EDIT_LAYER: &str = "Opacity Mask Editing";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "transparency.editOpacityMask",
            "Edit Opacity Mask",
            ["Window", "Transparency"],
            None,
            "{id?} edit the selected object's mask art in place (the mask thumbnail in the Transparency panel) → {layer}",
            has_doc,
            enter
        ),
        cmd!(
            "transparency.stopEditingOpacityMask",
            "Stop Editing Opacity Mask",
            ["Window", "Transparency"],
            None,
            "{} leave mask editing (the object thumbnail) → {id}",
            has_doc,
            leave
        ),
    ]
}

fn enter(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "transparency.editOpacityMask";
    if s.doc()?.doc.mask_edit.is_some() {
        return Err(bad(C, "already editing an opacity mask"));
    }
    let st = s.doc()?;
    let id = id_param(p, "id")
        .or_else(|| st.selection.objects.iter().copied().find(|i| st.doc.node(*i).is_some_and(|n| n.mask.is_some())))
        .ok_or_else(|| bad(C, "select an object with an opacity mask"))?;
    let layer = s.edit("Edit Opacity Mask", |d, sel| {
        let art = d
            .node(id)
            .and_then(|n| n.mask.as_ref())
            .map(|m| m.art.clone())
            .ok_or_else(|| EngineError::Other("the object has no opacity mask".into()))?;
        let layer = d.add_layer(Some(MASK_EDIT_LAYER));
        // Fresh ids: copies of a masked object share the ids inside their mask art.
        let copy = d.reid(&art);
        let cid = copy.id;
        d.insert(Some(layer), 0, copy)?;
        d.mask_edit = Some(MaskEdit { object: id, layer });
        sel.set([cid]);
        Ok(layer)
    })?;
    let st = s.doc_mut()?;
    st.isolation = Some(layer);
    st.active_layer = Some(layer);
    st.revision += 1;
    Ok(json!({ "layer": layer.0 }))
}

fn leave(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(me) = s.doc()?.doc.mask_edit else { return Err(bad("transparency.stopEditingOpacityMask", "not editing an opacity mask")) };
    s.edit("Stop Editing Opacity Mask", |d, sel| {
        sync(d);
        d.mask_edit = None;
        let _ = d.remove(me.layer);
        if d.node(me.object).is_some() {
            sel.set([me.object]);
        } else {
            sel.clear();
        }
        Ok(())
    })?;
    let st = s.doc_mut()?;
    st.isolation = None;
    st.active_layer = st.doc.default_layer();
    st.revision += 1;
    Ok(json!({ "id": me.object.0 }))
}

/// Write the editing layer's art into the object's mask (called after every edit). Leaves the
/// mode when the object or the layer is gone (e.g. deleted).
pub(crate) fn sync(d: &mut Document) {
    let Some(me) = d.mask_edit else { return };
    let art: Option<Vec<Arc<Node>>> = d.node(me.layer).and_then(|l| l.children().cloned());
    let (Some(art), true) = (art, d.node(me.object).is_some_and(|n| n.mask.is_some())) else {
        d.mask_edit = None;
        let _ = d.remove(me.layer);
        return;
    };
    let new_art = match art.as_slice() {
        [one] => one.clone(),
        many => {
            // Several objects (or none): a group, so the mask keeps one art node.
            let mut g = Node::group(NodeId(u64::MAX - 1), many.to_vec());
            g.name = Some("Opacity Mask".into());
            Arc::new(g)
        }
    };
    if let Some(m) = d.node(me.object).and_then(|n| n.mask.as_ref())
        && Arc::ptr_eq(&m.art, &new_art)
    {
        return;
    }
    if let Some(m) = d.node_mut(me.object).and_then(|n| n.mask.as_mut()) {
        m.art = new_art;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{NodeId, Session};

    #[test]
    fn edit_mask_art_in_place() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 300, "height": 300})).unwrap();
        let obj = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 200, "height": 200})).unwrap()["id"].as_u64().unwrap();
        let m = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 100, "height": 200})).unwrap()["id"].as_u64().unwrap();
        s.execute("paint.setFill", &json!({"color": "#ffffff", "ids": [m]})).unwrap();
        s.execute("select.set", &json!({"ids": [obj, m]})).unwrap();
        s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
        let layer = s.execute("transparency.editOpacityMask", &json!({})).unwrap()["layer"].as_u64().unwrap();
        assert_eq!(s.doc().unwrap().isolation, Some(NodeId(layer)));
        // Edit the mask art with ordinary commands: widen it; the object's mask follows live.
        s.execute("object.move", &json!({"dx": 50, "dy": 0})).unwrap();
        let mask_bounds = |s: &Session| s.doc().unwrap().doc.node(NodeId(obj)).unwrap().mask.as_ref().unwrap().art.geometric_bounds().unwrap();
        assert!((mask_bounds(&s).x0 - 50.0).abs() < 1e-6);
        // Drawing while editing adds to the mask.
        s.execute("shape.ellipse", &json!({"x": 180, "y": 150, "width": 40, "height": 40})).unwrap();
        assert!(mask_bounds(&s).x1 > 185.0);
        // Rendered: the object shows through the moved mask; the mask art itself isn't painted
        // (the ellipse pokes out of the object at x = 210).
        let doc = s.doc().unwrap().doc.clone();
        let img = vectorcraft_render::Renderer::new().render(&doc, 300, 300, vectorcraft_geom::Affine::IDENTITY, &Default::default());
        assert!(img.pixel(100, 100)[3] > 200);
        assert_eq!(img.pixel(25, 100)[3], 0);
        assert_eq!(img.pixel(212, 170)[3], 0);
        s.execute("transparency.stopEditingOpacityMask", &json!({})).unwrap();
        let st = s.doc().unwrap();
        assert!(st.doc.mask_edit.is_none() && st.isolation.is_none());
        assert!(!st.doc.layers.iter().any(|l| l.name.as_deref() == Some(super::MASK_EDIT_LAYER)));
        assert_eq!(st.selection.objects, vec![NodeId(obj)]);
        // Undo returns to editing mode's last state, then to before.
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.mask_edit.is_some());
    }
}
