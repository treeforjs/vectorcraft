//! The glyph geometry of type is cached per node, and every edit of the text makes a new node: the
//! entries of earlier versions must go, or typing in a long story grows without bound.

use vectorcraft_doc::text::{CharStyle, TextObject};
use vectorcraft_doc::{Node, NodeKind};
use vectorcraft_geom::Point;

use super::*;

#[test]
fn text_cache_drops_earlier_versions_of_edited_type() {
    let mut d = Document::new(300.0, 150.0);
    let id = d.alloc_id();
    let l = d.layers[0].id;
    let opts = RenderOptions::default();
    let mut r = Renderer::new();
    for k in 0..20 {
        let text = format!("Hamburg {}", "x".repeat(k));
        let mut n = Node::new(id, NodeKind::Text(Box::new(TextObject::point(Point::new(10.0, 100.0), &text, CharStyle::default()))));
        n.id = id;
        if k > 0 {
            d.remove(id).unwrap();
        }
        d.insert(Some(l), 0, n).unwrap();
        r.render(&d, 300, 150, Affine::IDENTITY, &opts);
    }
    assert!(r.texts.len() <= 4, "{} cached versions of one text object", r.texts.len());
}
