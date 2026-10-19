//! Effect › 3D and Materials: apply the effect as an object type, read its settings and edit them
//! (the 3D and Materials panel acts through these). The effect itself is
//! `vectorcraft_effects::MATERIALS` in the object's own effect list; `effect.*` reach it too.

use serde_json::{Map, Value, json};
use vectorcraft_doc::Node;
use vectorcraft_render::effects;

use super::appearance::appearance_targets;
use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "threeD.apply",
            "3D and Materials",
            [],
            None,
            "{object?: \"plane\"|\"extrude\"|\"revolve\"|\"inflate\" (\"extrude\"), params?: {…} (3D and Materials settings, see threeD.set), ids?: [..]} give each selected object Effect › 3D and Materials as that object type; an object that already has it switches to the type and keeps its other settings; one undo step → {ids, index (of the effect in the first object's effects)}",
            has_doc,
            apply
        ),
        cmd!(
            query "threeD.settings",
            "3D and Materials Settings",
            [],
            None,
            "{ids?: [..]} → {params: the first selected object's 3D and Materials settings with the defaults filled in, or null without one, index: the effect's position in its effects or null, ids: the selected objects that have it, presets: {objects, bevels, rotation: [{id, label, rotateX, rotateY, rotateZ}], materials: [{id, label, roughness, metallic, materialColor}], lighting: [{id, label}]}, maxLights}",
            always,
            settings
        ),
        cmd!(
            "threeD.set",
            "3D and Materials Options",
            [],
            None,
            "{params?: {…} merged into the settings (object, depth, cap, twist, taper, bevel, bevelShape, bevelWidth, bevelHeight, bevelRepeat, bevelOutside, bevelBothSides, angle, offset, from, volume, bothSides, rotateX, rotateY, rotateZ, perspective, roughness, metallic, materialColor, lights, ambient, shadow, shadowPosition, shadowDistance, shadowBounds, shadowSoftness, shadowOpacity; see effect.list), rotation?: view preset id (sets rotateX/Y/Z), material?: material preset id (sets roughness, metallic, materialColor), lighting?: lighting preset id (sets the lights and ambient), light?: {index: int (the light to change; the count adds one, at most 4), color?, intensity?, rotation?, height?, softness?}, removeLight?: int, ids?: [..]} edit each selected object's 3D and Materials effect as one undo step (a rotation, material or lighting edited by hand becomes \"custom\") → {ids}",
            has_doc,
            set
        ),
    ]
}

/// Where `n`'s 3D and Materials effect is in its own effects (the last one, which draws).
fn index_of(n: &Node) -> Option<usize> {
    n.appearance.effects.iter().rposition(|e| e.id == effects::MATERIALS)
}

fn object_param(p: &Value, c: &str) -> Result<Option<&'static str>> {
    match p.get("object") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let id = v.as_str().unwrap_or_default();
            effects::OBJECT_TYPES
                .iter()
                .find(|o| o.0.eq_ignore_ascii_case(id))
                .map(|o| Some(o.0))
                .ok_or_else(|| bad(c, "`object` must be plane, extrude, revolve or inflate"))
        }
    }
}

/// The `params` object of `p` (empty when missing).
fn params_object(p: &Value, c: &str) -> Result<Map<String, Value>> {
    match p.get("params") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(m)) => Ok(m.clone()),
        Some(_) => Err(bad(c, "`params` must be an object")),
    }
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "threeD.apply";
    let object = object_param(p, C)?.unwrap_or("extrude");
    let mut params = params_object(p, C)?;
    params.insert("object".into(), json!(object));
    let params = Value::Object(params);
    let effect = effects::new_effect(effects::MATERIALS, &params).ok_or_else(|| bad(C, "3D and Materials is not available"))?;
    let roots = appearance_targets(s, p)?;
    if roots.is_empty() {
        return Err(bad(C, "select objects"));
    }
    let mut first = None;
    let ids = s.edit("3D and Materials", |d, _| {
        let mut done = vec![];
        for id in &roots {
            let Some(n) = d.node_mut(*id) else { continue };
            let index = match index_of(n) {
                Some(i) => {
                    if let Some(e) = n.appearance.effects.get_mut(i) {
                        merge_into(&mut e.params, &params);
                        e.visible = true;
                    }
                    i
                }
                None => {
                    n.appearance.effects.push(effect.clone());
                    n.appearance.effects.len() - 1
                }
            };
            first.get_or_insert(index);
            done.push(*id);
        }
        if done.is_empty() {
            return Err(bad(C, "select objects"));
        }
        Ok(done)
    })?;
    Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "index": first}))
}

/// `edits` (an object) merged into `params` (made an object when it isn't).
fn merge_into(params: &mut Value, edits: &Value) {
    if !params.is_object() {
        *params = json!({});
    }
    if let (Value::Object(m), Value::Object(e)) = (params, edits) {
        for (k, v) in e {
            m.insert(k.clone(), v.clone());
        }
    }
}

fn presets() -> Value {
    let pairs = |list: &[(&str, &str)]| list.iter().map(|(id, label)| json!({"id": id, "label": label})).collect::<Vec<_>>();
    json!({
        "objects": pairs(&effects::OBJECT_TYPES),
        "bevels": pairs(&effects::BEVEL_PROFILES),
        "rotation": effects::ROTATION_PRESETS.iter().map(|(id, label, [x, y, z])| json!({"id": id, "label": label, "rotateX": x, "rotateY": y, "rotateZ": z})).collect::<Vec<_>>(),
        "materials": effects::MATERIAL_PRESETS
            .iter()
            .map(|(id, label, roughness, metallic, color)| json!({"id": id, "label": label, "roughness": roughness, "metallic": metallic, "materialColor": color}))
            .collect::<Vec<_>>(),
        "lighting": pairs(&effects::LIGHTING_PRESETS),
    })
}

fn settings(s: &mut Session, p: &Value) -> Result<Value> {
    let mut out = json!({"params": null, "index": null, "ids": [], "presets": presets(), "maxLights": effects::MAX_LIGHTS});
    if s.active().is_none() {
        return Ok(out);
    }
    let d = &s.doc()?.doc;
    let mut ids = vec![];
    for id in appearance_targets(s, p)? {
        let Some(n) = d.node(id) else { continue };
        let Some(i) = index_of(n) else { continue };
        if ids.is_empty()
            && let Some(e) = n.appearance.effects.get(i)
        {
            out["params"] = effects::merged_params(&e.id, &e.params);
            out["index"] = json!(i);
        }
        ids.push(id.0);
    }
    out["ids"] = json!(ids);
    Ok(out)
}

/// The edits `p` asks of settings `cur` (the current ones, defaults filled in): its params and
/// presets, and its light changes.
fn edits(p: &Value, cur: &Value, c: &str) -> Result<Map<String, Value>> {
    let mut out = params_object(p, c)?;
    if out.contains_key("object") {
        object_param(&Value::Object(out.clone()), c)?;
    }
    if let Some(lights) = out.get("lights")
        && !lights.is_array()
    {
        return Err(bad(c, "`lights` must be an array"));
    }
    // Hand-set values make their preset custom.
    let touched = |keys: &[&str]| keys.iter().any(|k| out.contains_key(*k));
    let custom: Vec<&str> = [
        ("rotation", touched(&["rotateX", "rotateY", "rotateZ"])),
        ("material", touched(&["roughness", "metallic", "materialColor"])),
        ("lighting", touched(&["lights", "ambient"]) || p.get("light").is_some() || p.get("removeLight").is_some()),
    ]
    .into_iter()
    .filter(|(k, t)| *t && !out.contains_key(*k))
    .map(|(k, _)| k)
    .collect();
    for k in custom {
        out.insert(k.into(), json!("custom"));
    }
    type Preset = fn(&str) -> Option<Value>;
    for (key, preset, what) in [
        ("rotation", effects::rotation_preset as Preset, "view"),
        ("material", effects::material_preset as Preset, "material"),
        ("lighting", effects::lighting_preset as Preset, "lighting"),
    ] {
        let Some(v) = p.get(key) else { continue };
        let id = v.as_str().unwrap_or_default();
        let Some(Value::Object(m)) = preset(id) else { return Err(bad(c, format!("unknown {what} preset `{id}`"))) };
        out.extend(m);
    }
    let light = p.get("light").filter(|l| !l.is_null());
    let remove = p.get("removeLight").filter(|l| !l.is_null());
    if light.is_some() || remove.is_some() {
        let mut lights: Vec<Value> = out
            .get("lights")
            .or_else(|| cur.get("lights"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(Value::is_object)
            .collect();
        lights.truncate(effects::MAX_LIGHTS);
        if let Some(l) = light {
            let Value::Object(fields) = l else { return Err(bad(c, "`light` must be an object")) };
            let index = fields.get("index").and_then(Value::as_u64).ok_or_else(|| bad(c, "`light.index` must be a light's index"))?;
            let index = usize::try_from(index).map_err(|_| bad(c, "`light.index` is out of range"))?;
            if index == lights.len() && index < effects::MAX_LIGHTS {
                lights.push(json!({"color": "#ffffff", "intensity": 100.0, "rotation": 45.0, "height": 45.0, "softness": 50.0}));
            }
            let target = lights.get_mut(index).ok_or_else(|| bad(c, "`light.index` is out of range"))?;
            for (k, v) in fields.iter().filter(|(k, _)| k.as_str() != "index") {
                target[k.as_str()] = v.clone();
            }
        }
        if let Some(r) = remove {
            let index = r
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < lights.len())
                .ok_or_else(|| bad(c, "`removeLight` must be a light's index"))?;
            lights.remove(index);
        }
        out.insert("lights".into(), Value::Array(lights));
    }
    Ok(out)
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "threeD.set";
    let roots = appearance_targets(s, p)?;
    // Check the request before editing anything.
    {
        let d = &s.doc()?.doc;
        let first = roots.iter().filter_map(|id| d.node(*id)).find_map(|n| index_of(n).and_then(|i| n.appearance.effects.get(i)));
        let Some(e) = first else { return Err(bad(C, "select an object with 3D and Materials")) };
        edits(p, &effects::merged_params(&e.id, &e.params), C)?;
    }
    let ids = s.edit("3D and Materials", |d, _| {
        let mut done = vec![];
        for id in &roots {
            let Some(n) = d.node_mut(*id) else { continue };
            let Some(e) = index_of(n).and_then(|i| n.appearance.effects.get_mut(i)) else { continue };
            let cur = effects::merged_params(&e.id, &e.params);
            let changes = Value::Object(edits(p, &cur, C)?);
            merge_into(&mut e.params, &changes);
            done.push(id.0);
        }
        Ok(done)
    })?;
    Ok(json!({ "ids": ids }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{NodeId, Session};

    fn session() -> (Session, NodeId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 400, "height": 400})).unwrap();
        let r = s.execute("shape.rectangle", &json!({"x": 100, "y": 100, "width": 100, "height": 100})).unwrap();
        (s, NodeId(r["id"].as_u64().unwrap()))
    }

    fn effects(s: &Session, id: NodeId) -> Vec<vectorcraft_doc::Effect> {
        s.doc().unwrap().doc.node(id).unwrap().appearance.effects.clone()
    }

    #[test]
    fn apply_adds_the_effect_and_switches_its_object_type() {
        let (mut s, id) = session();
        let r = s.execute("threeD.apply", &json!({"object": "inflate", "params": {"depth": 20}})).unwrap();
        assert_eq!((r["ids"].clone(), r["index"].clone()), (json!([id.0]), json!(0)));
        let fx = effects(&s, id);
        assert_eq!(
            (fx.len(), fx[0].id.as_str(), fx[0].params["object"].clone(), fx[0].params["depth"].clone()),
            (1, "threeD.materials", json!("inflate"), json!(20))
        );
        // Again as Revolve: the same effect, now revolved, the depth kept.
        s.execute("threeD.apply", &json!({"object": "revolve"})).unwrap();
        let fx = effects(&s, id);
        assert_eq!((fx.len(), fx[0].params["object"].clone(), fx[0].params["depth"].clone()), (1, json!("revolve"), json!(20)));
        // One undo step each.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&s, id)[0].params["object"], json!("inflate"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(effects(&s, id).is_empty());
        assert!(s.execute("threeD.apply", &json!({"object": "cube"})).is_err());
        assert!(s.execute("threeD.apply", &json!({"params": 5})).is_err());
        s.execute("select.none", &json!({})).unwrap();
        assert!(s.execute("threeD.apply", &json!({})).is_err(), "nothing selected");
    }

    #[test]
    fn settings_report_the_effect_and_the_presets() {
        let (mut s, id) = session();
        let r = s.execute("threeD.settings", &json!({})).unwrap();
        assert!(r["params"].is_null() && r["ids"] == json!([]));
        assert_eq!(r["presets"]["objects"].as_array().unwrap().len(), 4);
        assert!(r["presets"]["rotation"].as_array().unwrap().iter().any(|p| p["id"] == "isometricLeft"));
        s.execute("effect.apply", &json!({"effect": "stylize.dropShadow"})).unwrap();
        s.execute("threeD.apply", &json!({})).unwrap();
        let r = s.execute("threeD.settings", &json!({})).unwrap();
        assert_eq!((r["index"].clone(), r["ids"].clone()), (json!(1), json!([id.0])));
        assert_eq!((r["params"]["object"].clone(), r["params"]["lighting"].clone()), (json!("extrude"), json!("standard")));
        assert_eq!(r["params"]["lights"].as_array().unwrap().len(), 1, "defaults filled in");
        assert!(Session::new().execute("threeD.settings", &json!({})).unwrap()["params"].is_null());
    }

    #[test]
    fn set_edits_params_presets_and_lights_as_one_step_each() {
        let (mut s, id) = session();
        assert!(s.execute("threeD.set", &json!({"params": {"depth": 5}})).is_err(), "no 3D effect yet");
        s.execute("threeD.apply", &json!({})).unwrap();
        s.execute(
            "threeD.set",
            &json!({"params": {"depth": 80, "twist": 45}, "rotation": "isometricLeft", "material": "gold", "lighting": "diffuse"}),
        )
        .unwrap();
        let p = effects(&s, id)[0].params.clone();
        assert_eq!(
            (p["depth"].clone(), p["twist"].clone(), p["rotation"].clone(), p["rotateY"].clone()),
            (json!(80), json!(45), json!("isometricLeft"), json!(-35.264))
        );
        assert_eq!((p["material"].clone(), p["metallic"].clone(), p["lights"].as_array().unwrap().len()), (json!("gold"), json!(1.0), 2));
        // A hand-set angle makes the view custom; a light added, changed and removed.
        s.execute("threeD.set", &json!({"params": {"rotateX": 10}})).unwrap();
        assert_eq!(effects(&s, id)[0].params["rotation"], json!("custom"));
        s.execute("threeD.set", &json!({"light": {"index": 2, "intensity": 150, "color": "#ff0000"}})).unwrap();
        let p = effects(&s, id)[0].params.clone();
        assert_eq!(
            (p["lights"].as_array().unwrap().len(), p["lights"][2]["intensity"].clone(), p["lighting"].clone()),
            (3, json!(150), json!("custom"))
        );
        s.execute("threeD.set", &json!({"removeLight": 0})).unwrap();
        assert_eq!(effects(&s, id)[0].params["lights"].as_array().unwrap().len(), 2);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&s, id)[0].params["lights"].as_array().unwrap().len(), 3, "one undo step");
        for bad in [
            json!({"rotation": "sideways"}),
            json!({"material": 5}),
            json!({"lighting": null}),
            json!({"light": {"index": 9}}),
            json!({"light": {"index": -1}}),
            json!({"light": 3}),
            json!({"removeLight": 99}),
            json!({"params": {"object": "cube"}}),
            json!({"params": {"lights": 5}}),
            json!({"params": []}),
        ] {
            assert!(s.execute("threeD.set", &bad).is_err(), "{bad}");
        }
        assert_eq!(effects(&s, id)[0].params["lights"].as_array().unwrap().len(), 3, "refused edits change nothing");
    }

    #[test]
    fn a_drag_is_one_undo_step_and_expand_appearance_makes_faces() {
        let (mut s, id) = session();
        s.execute("threeD.apply", &json!({"object": "extrude", "params": {"bevel": true, "shadow": true}})).unwrap();
        s.begin_interaction("3D and Materials").unwrap();
        for depth in [10, 20, 30] {
            s.preview("threeD.set", &json!({"params": {"depth": depth}})).unwrap();
        }
        s.commit_interaction().unwrap();
        assert_eq!(effects(&s, id)[0].params["depth"], json!(30));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&s, id)[0].params["depth"], json!(50.0), "the whole drag undone at once");
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute("effect.expandAppearance", &json!({})).unwrap();
        let n = s.doc().unwrap().doc.node(id).cloned().unwrap();
        let faces = n.children().unwrap();
        assert!(
            matches!(n.kind, vectorcraft_doc::NodeKind::Group { .. }) && faces.len() >= 10,
            "shadow, bevel, walls and the front: {}",
            faces.len()
        );
        assert!(faces.iter().all(|f| matches!(f.kind, vectorcraft_doc::NodeKind::Path { .. })));
        assert!(n.appearance.effects.is_empty() && faces[0].opacity < 1.0, "the shadow first, the live effect gone");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&s, id)[0].id, "threeD.materials");
    }
}
