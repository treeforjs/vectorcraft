//! Type menu: Create Outlines, and the Character/Paragraph panel setters (`text.setText`,
//! `text.setStyle`).

use std::sync::Arc;

use drawcraft_doc::{Document, Justify, Node, NodeId, NodeKind, TextObject};
use drawcraft_geom::PathData;
use serde_json::{Value, json};

use super::edit::selected_roots;
use super::pathops::{num_param, shape_node, text_style_appearance};
use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "type.createOutlines",
            "Create Outlines",
            ["Type"],
            Some("Cmd+Shift+O"),
            "{} convert the selected text to groups of compound paths (one per glyph) → {ids}",
            has_selection,
            create_outlines
        ),
        cmd!(
            "text.setText",
            "Set Text",
            [],
            None,
            "{id?|ids?, text} replace the contents of text objects (keeps the first run's style)",
            has_doc,
            set_text
        ),
        cmd!(
            "text.setStyle",
            "Character",
            [],
            None,
            "{ids?|id?, font?, style?, size?: pt, leading?: pt|\"auto\", tracking?: 1/1000 em, justify?: \"left\"|\"center\"|\"right\"|\"justifyAll\", fill?: colour}",
            has_doc,
            set_style
        ),
    ]
}

/// Recompute the layout bounds cache after a text edit.
pub(crate) fn refresh_bounds(t: &mut TextObject) {
    let lay = drawcraft_text::layout(drawcraft_text::FontDb::global(), t);
    t.cached_bounds = Some(lay.bounds);
}

/// Text objects among `ids` and their descendants.
fn text_ids(d: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    let mut out = vec![];
    for id in ids {
        if let Some(n) = d.node(*id) {
            n.walk(&mut |c| {
                if matches!(c.kind, NodeKind::Text(_)) && !out.contains(&c.id) {
                    out.push(c.id);
                }
            });
        }
    }
    out
}

fn create_outlines(s: &mut Session, _: &Value) -> Result<Value> {
    let roots = selected_roots(s)?;
    let ids = s.edit("Create Outlines", |d, sel| {
        let texts = text_ids(d, &roots);
        if texts.is_empty() {
            return Err(EngineError::Other("Create Outlines: select text objects".into()));
        }
        let mut new_sel: Vec<NodeId> = roots.iter().copied().filter(|r| !texts.contains(r)).collect();
        let mut out = vec![];
        for tid in texts {
            let Some(n) = d.node(tid).cloned() else { continue };
            let NodeKind::Text(t) = &n.kind else { continue };
            let lay = drawcraft_text::layout(drawcraft_text::FontDb::global(), t);
            let mut children = vec![];
            for g in &lay.glyphs {
                let path = PathData::from_bezpath(&g.outline).transformed(t.xf);
                if path.is_empty() {
                    continue;
                }
                let st = t.runs.get(g.run).map(|r| r.style.clone()).unwrap_or_else(|| t.first_style());
                let mut node = shape_node(d, path, None);
                node.appearance = text_style_appearance(&st.fill, &st.stroke, st.stroke_width);
                children.push(Arc::new(node));
            }
            let (par, idx, _) = d.position(tid).ok_or(EngineError::NoNode(tid))?;
            d.remove(tid)?;
            if children.is_empty() {
                continue;
            }
            let gid = d.alloc_id();
            let mut g = Node::group(gid, children);
            g.opacity = n.opacity;
            g.blend = n.blend;
            g.name = n.name.clone();
            d.insert(par, idx, g)?;
            out.push(gid);
            if roots.contains(&tid) {
                new_sel.push(gid);
            }
        }
        sel.set(new_sel);
        Ok(out)
    })?;
    Ok(json!({ "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>() }))
}

pub(crate) fn text_targets(s: &Session, p: &Value, cmd: &str) -> Result<Vec<NodeId>> {
    let ids = targets(s, p)?;
    let t = text_ids(&s.doc()?.doc, &ids);
    if t.is_empty() {
        return Err(bad(cmd, "no text objects selected"));
    }
    Ok(t)
}

fn set_text(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("text.setText", "missing `text`"))?.to_string();
    let ids = text_targets(s, p, "text.setText")?;
    s.edit("Typing", |d, _| {
        for id in &ids {
            let Some(NodeKind::Text(t)) = d.node_mut(*id).map(|n| &mut n.kind) else { continue };
            let style = t.first_style();
            t.runs = vec![drawcraft_doc::TextRun { text: text.clone(), style }];
            refresh_bounds(t);
        }
        Ok(())
    })?;
    Ok(json!({ "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>() }))
}

fn justify_param(v: &str) -> Option<Justify> {
    Some(match v.to_ascii_lowercase().as_str() {
        "left" => Justify::Left,
        "center" => Justify::Center,
        "right" => Justify::Right,
        "justifyleft" => Justify::JustifyLeft,
        "justifycenter" => Justify::JustifyCenter,
        "justifyright" => Justify::JustifyRight,
        "justifyall" | "justify" => Justify::JustifyAll,
        _ => return None,
    })
}

fn set_style(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "text.setStyle";
    let font = str_param(p, "font").map(str::to_string);
    let style = str_param(p, "style").map(str::to_string);
    let size = num_param(p, "size");
    if size.is_some_and(|v| v <= 0.0) {
        return Err(bad(C, "size must be positive"));
    }
    let leading = match p.get("leading") {
        None | Some(Value::Null) => None,
        Some(Value::String(a)) if a.eq_ignore_ascii_case("auto") => Some(None),
        Some(_) => Some(Some(num_param(p, "leading").ok_or_else(|| bad(C, "leading must be a number or \"auto\""))?.clamp(0.1, 5000.0))),
    };
    let tracking = num_param(p, "tracking");
    let justify = match str_param(p, "justify") {
        Some(j) => Some(justify_param(j).ok_or_else(|| bad(C, "justify must be left|center|right|justifyAll"))?),
        None => None,
    };
    let fill = match p.get("fill") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) if n.eq_ignore_ascii_case("none") => Some(drawcraft_color::Paint::None),
        Some(v) => Some(drawcraft_color::Paint::solid(color_value(v).ok_or_else(|| bad(C, "bad fill colour"))?)),
    };
    if font.is_none() && style.is_none() && size.is_none() && leading.is_none() && tracking.is_none() && justify.is_none() && fill.is_none() {
        return Err(bad(C, "nothing to change"));
    }
    let ids = text_targets(s, p, C)?;
    s.edit("Character", |d, _| {
        for id in &ids {
            let Some(NodeKind::Text(t)) = d.node_mut(*id).map(|n| &mut n.kind) else { continue };
            for r in &mut t.runs {
                let st = &mut r.style;
                if let Some(f) = &font {
                    st.font_family = f.clone();
                }
                if let Some(f) = &style {
                    st.font_style = f.clone();
                }
                if let Some(v) = size {
                    st.size = v.clamp(0.1, 1296.0);
                }
                if let Some(l) = leading {
                    st.leading = l;
                }
                if let Some(v) = tracking {
                    st.tracking = v.clamp(-1000.0, 10000.0);
                }
                if let Some(f) = &fill {
                    st.fill = f.clone();
                }
            }
            if let Some(j) = justify {
                t.para.justify = j;
            }
            refresh_bounds(t);
        }
        Ok(())
    })?;
    Ok(json!({ "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>() }))
}
