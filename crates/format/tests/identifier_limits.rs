//! Native files must leave room for allocating object and resource IDs after import.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_doc::{Document, ExportAsset, NodeId, Slice};
use vectorcraft_format::{FormatError, load, save};
use vectorcraft_geom::Rect;

fn file() -> Value {
    let mut d = Document::new(100.0, 100.0);
    d.slices.push(Slice { id: NodeId(2), rect: Rect::new(0.0, 0.0, 20.0, 20.0), options: Default::default() });
    d.assets.push(ExportAsset { id: 3, name: "Asset 1".into(), nodes: vec![d.layers[0].id] });
    d.reserve_ids(4);
    serde_json::from_slice(&save(&d, false)).unwrap()
}

#[test]
fn exhausted_and_nearly_exhausted_object_ids_are_rejected() {
    for field in ["/document/next_id", "/document/layers/0/id", "/document/slices/0/id", "/document/assets/0/id"] {
        for id in [i64::MAX as u64 + 1, u64::MAX - 1, u64::MAX] {
            let mut f = file();
            *f.pointer_mut(field).unwrap() = json!(id);
            let result = load(&serde_json::to_vec(&f).unwrap());
            assert!(matches!(result, Err(FormatError::IdentifierRange("object"))), "{field}={id}: {result:?}");
        }
    }
}

#[test]
fn exhausted_and_nearly_exhausted_resource_ids_are_rejected() {
    for (field, kind) in [("/document/artboards/0/id", "artboard"), ("/document/graphic_styles/0/id", "graphic style")] {
        for id in [i32::MAX as u32 + 1, u32::MAX - 1, u32::MAX] {
            let mut f = file();
            *f.pointer_mut(field).unwrap() = json!(id);
            let result = load(&serde_json::to_vec(&f).unwrap());
            assert!(matches!(result, Err(FormatError::IdentifierRange(k)) if k == kind), "{field}={id}: {result:?}");
        }
    }
}

#[test]
fn sparse_high_ids_keep_their_references_and_allocate_after_loading() {
    let mut f = file();
    let high = 1u64 << 40;
    f["document"]["layers"][0]["id"] = json!(high);
    f["document"]["slices"][0]["id"] = json!(high + 10);
    f["document"]["assets"][0]["id"] = json!(high + 20);
    f["document"]["assets"][0]["nodes"][0] = json!(high);
    f["document"]["artboards"][0]["id"] = json!(1_000_000);
    f["document"]["graphic_styles"][0]["id"] = json!(1_000_000);
    let mut d = load(&serde_json::to_vec(&f).unwrap()).unwrap();
    assert_eq!(d.alloc_id(), NodeId(high + 21));
    assert_eq!(d.alloc_id(), NodeId(high + 22));
    assert_eq!(d.assets[0].nodes, [d.layers[0].id]);
    assert_eq!(d.next_artboard_id(), 1_000_001);
    assert_eq!(d.next_graphic_style_id(), 1_000_001);
    let reopened = load(&save(&d, false)).unwrap();
    assert_eq!(reopened, d);
}

#[test]
fn ordinary_documents_still_round_trip_and_allocate_fresh_ids() {
    let f = file();
    let mut d = load(&serde_json::to_vec(&f).unwrap()).unwrap();
    assert_eq!(d.alloc_id(), NodeId(4));
    let bytes = save(&d, false);
    let mut reopened = load(&bytes).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.alloc_id(), NodeId(5));
}
