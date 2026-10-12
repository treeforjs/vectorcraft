//! Ruler coordinate settings: view metadata, independent of artwork and undo.

use serde_json::{Value, json};
use vectorcraft_doc::rulers::{RulerMode, RulerSettings};
use vectorcraft_geom::Point;

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "view.rulers.global", "Change to Global Rulers", ["View", "Rulers"], Some("Cmd+Alt+R"),
            "{on?: bool (default toggle), artboard?: index} global or active-artboard ruler coordinates; does not transform artwork", has_doc, global),
        cmd!(query "view.rulers.origin", "Ruler Origin", [], None,
            "{point?: [documentX, documentY], reset?: bool, artboard?: index} set/reset the current mode's origin; no point/reset queries it", has_doc, origin),
        cmd!(query "view.rulers.info", "Ruler Coordinates", [], None,
            "{artboard?: index} → {mode, origin:[x,y], settings}; commands for artwork continue to use document coordinates", has_doc, info),
    ]
}

/// Origins for deleted artboards stay reserved so Undo can restore them without making a
/// new, unrelated artboard inherit that view state. Also reserve their IDs after reopening.
pub(super) fn next_artboard_id(doc: &vectorcraft_doc::Document, reserved: Option<u32>, cmd: &str) -> Result<u32> {
    doc.artboards
        .iter()
        .map(|b| b.id)
        .chain(reserved)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|id| *id <= i32::MAX as u32)
        .ok_or_else(|| bad(cmd, "no artboard identifiers remain in the supported range"))
}

fn params(p: &Value, cmd: &str) -> Result<()> {
    if p.is_null() || p.is_object() { Ok(()) } else { Err(bad(cmd, "params must be an object")) }
}

fn board(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let i = match p.get("artboard") {
        None => 0,
        Some(v) => v.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(cmd, "artboard must be a nonnegative index"))?,
    };
    if s.doc()?.doc.artboards.get(i).is_none() {
        return Err(bad(cmd, "artboard index is out of range"));
    }
    Ok(i)
}

fn boolean(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    p.get(key).map(|v| v.as_bool().ok_or_else(|| bad(cmd, format!("{key} must be a boolean")))).transpose()
}

fn response(s: &Session, index: usize) -> Result<Value> {
    let st = s.doc()?;
    let point = st.rulers.origin(&st.doc.artboards, index);
    Ok(json!({"mode": st.rulers.mode, "origin": [point.x, point.y], "settings": st.rulers}))
}

fn global(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "view.rulers.global";
    params(p, C)?;
    let i = board(s, p, C)?;
    let on = boolean(p, "on", C)?.unwrap_or(s.doc()?.rulers.mode != RulerMode::Global);
    s.doc_mut()?.rulers.mode = if on { RulerMode::Global } else { RulerMode::Artboard };
    response(s, i)
}

fn info(s: &mut Session, p: &Value) -> Result<Value> {
    params(p, "view.rulers.info")?;
    response(s, board(s, p, "view.rulers.info")?)
}

fn origin(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "view.rulers.origin";
    params(p, C)?;
    let i = board(s, p, C)?;
    let reset = boolean(p, "reset", C)?.unwrap_or(false);
    let point = p
        .get("point")
        .map(|v| {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(C, "point must contain two numbers"))?;
            let x = a.first().and_then(Value::as_f64).ok_or_else(|| bad(C, "point x must be a number"))?;
            let y = a.get(1).and_then(Value::as_f64).ok_or_else(|| bad(C, "point y must be a number"))?;
            let pt = Point::new(x, y);
            if RulerSettings::valid_point(pt) { Ok(pt) } else { Err(bad(C, "point must be finite and within ±1 billion document points")) }
        })
        .transpose()?;
    if reset && point.is_some() {
        return Err(bad(C, "use either point or reset, not both"));
    }
    if reset || point.is_some() {
        let st = s.doc_mut()?;
        match st.rulers.mode {
            RulerMode::Global => st.rulers.global_origin = point,
            RulerMode::Artboard => {
                let b = st.doc.artboards.get(i).ok_or_else(|| bad(C, "artboard index is out of range"))?;
                if let Some(point) = point {
                    let base = Point::new(b.rect.x0, b.rect.y0);
                    if !RulerSettings::valid_point(base) {
                        return Err(bad(C, "artboard origin is outside the supported coordinate range"));
                    }
                    st.rulers.artboard_origins.insert(b.id, Point::new(point.x - base.x, point.y - base.y));
                } else {
                    st.rulers.artboard_origins.remove(&b.id);
                }
            }
        }
    }
    response(s, i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let mut doc = (*s.doc().unwrap().doc).clone();
        let mut b = doc.artboards[0].clone();
        b.id = 9;
        b.rect = vectorcraft_geom::Rect::new(800.0, -100.0, 1000.0, 100.0);
        doc.artboards.push(b);
        *s.doc_mut().unwrap() = crate::DocState::new(doc, None);
        s
    }

    #[test]
    fn ruler_modes_origins_and_reset_leave_artwork_and_history_unchanged() {
        let mut s = session();
        let original = s.doc().unwrap().doc.clone();
        assert!(!s.doc().unwrap().is_dirty());
        s.execute("view.rulers.global", &json!({"on": false})).unwrap();
        assert_eq!(s.execute("view.rulers.info", &json!({"artboard":1})).unwrap()["origin"], json!([800.0, -100.0]));
        s.execute("view.rulers.origin", &json!({"artboard":1,"point":[820,-70]})).unwrap();
        assert_eq!(s.execute("view.rulers.info", &json!({"artboard":1})).unwrap()["origin"], json!([820.0, -70.0]));
        s.execute("view.rulers.global", &json!({"on": true})).unwrap();
        s.execute("view.rulers.origin", &json!({"point":[50,60]})).unwrap();
        s.execute("view.rulers.global", &json!({"on": false})).unwrap();
        assert_eq!(s.execute("view.rulers.info", &json!({"artboard":1})).unwrap()["origin"], json!([820.0, -70.0]));
        s.execute("view.rulers.origin", &json!({"artboard":1,"reset":true})).unwrap();
        assert_eq!(s.execute("view.rulers.info", &json!({"artboard":1})).unwrap()["origin"], json!([800.0, -100.0]));
        assert!(Arc::ptr_eq(&original, &s.doc().unwrap().doc));
        assert!(!s.doc().unwrap().is_dirty());
        assert!(!s.commands().iter().find(|c| c.id == "edit.undo").unwrap().enabled);
    }

    #[test]
    fn deleted_artboard_origins_are_reserved_for_undo_not_inherited_by_new_boards() {
        let mut s = session();
        s.execute("view.rulers.global", &json!({"on":false})).unwrap();
        s.execute("view.rulers.origin", &json!({"artboard":1,"point":[825,-75]})).unwrap();
        s.execute("artboard.delete", &json!({"index":1})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("view.rulers.info", &json!({"artboard":1})).unwrap()["origin"], json!([825.0, -75.0]));
        s.execute("edit.redo", &json!({})).unwrap();
        // Model the native save/reopen boundary, which stores even the deleted board's offset.
        let mut saved = (*s.doc().unwrap().doc).clone();
        saved.rulers = s.doc().unwrap().rulers.clone();
        *s.doc_mut().unwrap() = crate::DocState::new(saved, None);
        s.execute("artboard.new", &json!({"x":200,"y":300})).unwrap();
        assert_ne!(s.doc().unwrap().doc.artboards[1].id, 9);
        assert_eq!(s.execute("view.rulers.info", &json!({"artboard":1})).unwrap()["origin"], json!([200.0, 300.0]));
    }

    #[test]
    fn duplicate_alt_copy_paste_and_conversion_reserve_deleted_origin_ids() {
        for (cmd, params) in [
            ("artboard.duplicate", json!({"index":0})),
            ("artboard.move", json!({"index":0,"copy":true,"dx":20,"dy":30})),
            ("edit.paste", json!({})),
            ("artboard.convertToArtboards", json!({})),
        ] {
            let mut s = session();
            s.execute("view.rulers.global", &json!({"on":false})).unwrap();
            s.execute("view.rulers.origin", &json!({"artboard":1,"point":[825,-75]})).unwrap();
            s.execute("artboard.delete", &json!({"index":1})).unwrap();
            s.execute("shape.rectangle", &json!({"x":10,"y":10,"width":20,"height":20})).unwrap();
            s.execute("artboard.copy", &json!({"index":0})).unwrap();
            s.execute(cmd, &params).unwrap();
            let st = s.doc().unwrap();
            let board = &st.doc.artboards[1];
            assert!(board.id > 9, "{cmd} reused deleted ID");
            assert_eq!(st.rulers.origin(&st.doc.artboards, 1), Point::new(board.rect.x0, board.rect.y0), "{cmd}");
        }
    }

    #[test]
    fn invalid_ruler_requests_are_atomic_and_documents_are_independent() {
        let mut s = session();
        let before = s.doc().unwrap().rulers.clone();
        for p in [
            json!({"point":[1e100,0]}),
            json!({"point":[1]}),
            json!({"point":["a",0]}),
            json!({"reset":"yes"}),
            json!({"artboard":999,"point":[1,2]}),
            json!({"reset":true,"point":[1,2]}),
            json!([]),
        ] {
            assert!(s.execute("view.rulers.origin", &p).is_err(), "{p}");
            assert_eq!(s.doc().unwrap().rulers, before);
        }
        assert!(s.execute("view.rulers.global", &json!({"on":"yes"})).is_err());
        assert_eq!(s.doc().unwrap().rulers, before);
        s.execute("view.rulers.origin", &json!({"point":[7,9]})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().rulers, RulerSettings::default());
    }
}
