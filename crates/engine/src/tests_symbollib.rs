//! Symbol libraries: the built-in libraries, adding to the document (deduped, one undo step,
//! resources and sizes brought along, placed), saving and loading `.vcsymbols` and user libraries.

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::{Paint, Swatch};
use vectorcraft_doc::symbol_libs::SYMBOL_LIBRARIES;
use vectorcraft_doc::{ImageBlob, Node, NodeId, NodeKind, PatternDef};
use vectorcraft_geom::{Rect, shapes};

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn doc(s: &Session) -> &vectorcraft_doc::Document {
    &s.doc().unwrap().doc
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

fn names(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect()
}

fn symbol_names(s: &Session) -> Vec<String> {
    doc(s).symbols.iter().map(|x| x.name.clone()).collect()
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> NodeId {
    NodeId(run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"].as_u64().unwrap())
}

#[test]
fn builtin_libraries_are_listed_and_every_symbol_places_visibly() {
    let mut s = session();
    let list = run(&mut s, "symbol.libraries", json!({}));
    let libs = list["libraries"].as_array().unwrap();
    assert_eq!(libs.len(), SYMBOL_LIBRARIES.len());
    assert!(libs.iter().all(|l| l["category"] == "builtIn"), "{libs:?}");
    let mut renderer = vectorcraft_render::Renderer::new();
    for l in libs {
        let id = l["id"].as_str().unwrap();
        let g = run(&mut s, "symbol.library", json!({"library": id}));
        let symbols = g["symbols"].as_array().unwrap();
        assert!(symbols.len() >= 6 && l["count"].as_u64() == Some(symbols.len() as u64), "{id}");
        for sym in symbols {
            let name = sym["name"].as_str().unwrap();
            run(&mut s, "file.new", json!({"width": 100, "height": 100}));
            let r = run(&mut s, "symbol.addFromLibrary", json!({"library": id, "name": name, "place": true, "x": 50, "y": 50}));
            let inst = NodeId(r["id"].as_u64().unwrap());
            let b = doc(&s).node(inst).unwrap().geometric_bounds().unwrap();
            let size = sym["size"].as_array().unwrap();
            assert!((b.width() - size[0].as_f64().unwrap()).abs() < 1e-6, "{id} / {name}: placed at its natural size");
            let img = renderer.render_region(doc(&s), Rect::new(0.0, 0.0, 100.0, 100.0), 1.0, true);
            let painted = img.pixels.as_chunks::<4>().0.iter().filter(|p| p[..3] != [255, 255, 255]).count();
            assert!(painted > 60, "{id} / {name} paints {painted} pixels");
        }
    }
    // By name in any case.
    assert_eq!(run(&mut s, "symbol.library", json!({"library": "map markers"}))["id"], "map-markers");
    assert!(s.execute("symbol.library", &json!({"library": "nope"})).is_err());
}

#[test]
fn add_from_library_is_one_undo_step_and_numbers_names_taken_by_other_art() {
    let mut s = session();
    let before = undo_len(&s);
    let r = run(&mut s, "symbol.addFromLibrary", json!({"library": "map-markers", "names": ["Pin", "Flag", "Pin"]}));
    assert_eq!((names(&r["added"]), names(&r["existing"])), (vec!["Pin", "Flag"], vec![]));
    assert_eq!(undo_len(&s), before + 1, "one undo step");
    assert_eq!(symbol_names(&s), ["Pin", "Flag"]);
    assert_eq!(run(&mut s, "symbol.list", json!({}))["current"], "Pin", "the first becomes current");
    let size = run(&mut s, "symbol.list", json!({}))["symbols"][0]["size"].clone();
    assert_eq!(size, json!([28.0, 40.0]), "the natural size comes along");
    // Adding again finds them and changes nothing (no undo step).
    let r = run(&mut s, "symbol.addFromLibrary", json!({"library": "map-markers", "name": "Pin"}));
    assert_eq!((names(&r["added"]), names(&r["existing"])), (vec![], vec!["Pin"]));
    assert_eq!(undo_len(&s), before + 1);
    // A name the document gives other art gets a number.
    let id = rect(&mut s, 0.0, 0.0, 20.0, 20.0);
    run(&mut s, "symbol.new", json!({"name": "Compass", "ids": [id.0]}));
    let r = run(&mut s, "symbol.addFromLibrary", json!({"library": "map-markers", "name": "Compass"}));
    assert_eq!(names(&r["added"]), ["Compass 2"]);
    assert_eq!(names(&r["symbols"]), ["Compass 2"]);
    // Placing comes in the same step; undo removes the symbol and the instance.
    let steps = undo_len(&s);
    let r = run(&mut s, "symbol.addFromLibrary", json!({"library": "outdoors", "name": "Sun", "place": true, "x": 40, "y": 60}));
    assert_eq!(undo_len(&s), steps + 1, "added and placed as one step");
    let inst = doc(&s).node(NodeId(r["id"].as_u64().unwrap())).unwrap();
    assert!(matches!(&inst.kind, NodeKind::SymbolInstance { symbol, .. } if symbol == "Sun"));
    assert!((inst.geometric_bounds().unwrap().center() - vectorcraft_geom::Point::new(40.0, 60.0)).hypot() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert!(!symbol_names(&s).contains(&"Sun".to_string()) && doc(&s).node(NodeId(r["id"].as_u64().unwrap())).is_none());
    // The whole library.
    let r = run(&mut s, "symbol.addFromLibrary", json!({"library": "interface-glyphs"}));
    assert_eq!(r["added"].as_array().unwrap().len(), cmd::symbollib::library(&s, "interface-glyphs").unwrap().1.len());
    assert!(s.execute("symbol.addFromLibrary", &json!({"library": "outdoors", "name": "Nope"})).is_err());
    assert!(s.execute("symbol.addFromLibrary", &json!({"library": "nope"})).is_err());
}

/// A document with pattern `name` (a dot) and its swatch.
fn add_pattern(s: &mut Session, name: &str, radius: f64) {
    s.edit("Pattern", |d, _| {
        let dot = Node::path(d.alloc_id(), shapes::ellipse(Rect::new(0.0, 0.0, radius, radius)), vectorcraft_doc::Appearance::default_art());
        d.patterns.push(PatternDef { tile: Rect::new(0.0, 0.0, 10.0, 10.0), ..PatternDef::new(name, vec![Arc::new(dot)]) });
        d.swatches.push(Swatch {
            name: name.into(),
            paint: Paint::Pattern { pattern: name.into(), xf: Default::default() },
            global: false,
            spot: false,
        });
        Ok(())
    })
    .unwrap();
}

#[test]
fn save_and_load_round_trip_with_the_resources_the_art_uses() {
    let mut s = session();
    add_pattern(&mut s, "Dots", 4.0);
    run(&mut s, "swatch.edit", json!({"name": "Red", "global": true}));
    // "Badge": a dotted square with a red outline, holding an instance of "Dot".
    let dot = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    run(&mut s, "symbol.new", json!({"name": "Dot", "ids": [dot.0]}));
    let dot_inst = doc(&s).symbols.len();
    assert_eq!(dot_inst, 1);
    let inst = run(&mut s, "symbol.place", json!({"name": "Dot", "x": 30, "y": 30}))["id"].as_u64().unwrap();
    let sq = rect(&mut s, 0.0, 0.0, 60.0, 40.0);
    run(&mut s, "select.set", json!({"ids": [sq.0]}));
    run(&mut s, "paint.setFill", json!({"swatch": "Dots"}));
    run(&mut s, "paint.setStroke", json!({"swatch": "Red"}));
    run(&mut s, "symbol.new", json!({"name": "Badge", "ids": [sq.0, inst]}));
    let r = run(&mut s, "symbol.saveLibrary", json!({"names": ["Badge"], "name": "Kit"}));
    assert_eq!(r["count"], 2, "Badge and the Dot it places");
    let data = r["data"].as_str().unwrap().to_string();
    assert!(cmd::symbollib::sniff(&data) && data.contains("\"vcsymbols\""));
    let r = run(&mut s, "symbol.loadLibrary", json!({"data": data, "name": "kit.vcsymbols"}));
    assert_eq!((r["name"].as_str(), r["count"].as_u64()), (Some("Kit"), Some(2)));
    let lib_id = r["library"].as_str().unwrap().to_string();
    let (_, lib) = cmd::symbollib::library(&s, &lib_id).unwrap();
    assert_eq!(lib.names().collect::<Vec<_>>(), ["Badge", "Dot"], "the symbols asked for first");
    assert_eq!(lib.size("Badge"), Some((60.0, 40.0)));
    assert_eq!(lib.doc.patterns.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Dots"]);
    assert!(lib.doc.swatches.iter().any(|w| w.name == "Red" && w.global));
    let list = run(&mut s, "symbol.libraries", json!({}));
    assert!(list["libraries"].as_array().unwrap().iter().any(|l| l["id"] == lib_id.as_str() && l["category"] == "loaded"));

    // Into another document: the nested symbol, the pattern (with its swatch) and the global
    // swatch come along, with the sizes.
    run(&mut s, "file.new", json!({"width": 100, "height": 100}));
    let r = run(&mut s, "symbol.addFromLibrary", json!({"library": lib_id, "name": "Badge"}));
    assert_eq!(names(&r["added"]), ["Badge"]);
    let d = doc(&s);
    assert!(d.symbols.iter().any(|x| x.name == "Dot") && d.pattern("Dots").is_some() && d.swatch("Dots").is_some());
    // The document's own "Red" isn't global, so the library's comes in beside it.
    assert!(d.swatches_iter().any(|w| w.global && w.name.starts_with("Red")));
    assert_eq!(vectorcraft_doc::symbol_libs::natural_size(d, "Badge"), Some((60.0, 40.0)));
    assert!(s.execute("symbol.saveLibrary", &json!({"names": ["Nope"]})).is_err());
    assert!(s.execute("symbol.saveLibrary", &json!({"user": true})).is_err(), "no user folder in a headless session");
}

#[test]
fn vcsymbols_files_keep_images_and_reject_bad_data() {
    let mut lib = (*vectorcraft_doc::symbol_libs::builtin_symbol_library("badges-labels").unwrap()).clone();
    lib.doc.images.insert("k".into(), ImageBlob::new("image/png", vec![1, 2, 3, 250]));
    let text = cmd::symbollib::write(&lib);
    let back = cmd::symbollib::read(&text, "x").unwrap();
    assert_eq!(back.name, lib.name);
    assert_eq!(back.doc.symbols, lib.doc.symbols);
    assert_eq!(back.names().map(|n| back.size(n)).collect::<Vec<_>>(), lib.names().map(|n| lib.size(n)).collect::<Vec<_>>());
    assert_eq!(back.doc.images["k"].bytes.as_slice(), &[1, 2, 3, 250]);
    // An unnamed library takes the file's name; hostile sizes and images are dropped; other JSON
    // isn't a library.
    let odd = r#"{"format": "vcsymbols", "version": 1, "name": " ", "symbols": [], "sizes": {"a": [-1, 2], "b": "x"}, "images": {"k": {"mime": "image/png", "data": "@@"}}}"#;
    let odd = cmd::symbollib::read(odd, "Mine").unwrap();
    assert_eq!((odd.name.as_str(), odd.size("a"), odd.doc.images.len()), ("Mine", None, 0));
    assert!(cmd::symbollib::read("{\"format\": \"vcstyles\", \"version\": 1}", "x").is_err());
    assert!(cmd::symbollib::read("not json", "x").is_err());
    assert!(!cmd::symbollib::sniff("{\"format\": \"vectorcraft\"}"));
}

#[test]
fn user_libraries_and_other_documents_symbols() {
    let dir = std::env::temp_dir().join(format!("vc-symbollib-user-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = session();
    s.symbol_libraries.set_user_dir(Some(dir.to_string_lossy().to_string()));
    run(&mut s, "symbol.addFromLibrary", json!({"library": "basic-shapes", "names": ["Star", "Heart"]}));
    let r = run(&mut s, "symbol.saveLibrary", json!({"user": true, "name": "My: Marks"}));
    assert_eq!(r["library"], "user/My- Marks.vcsymbols");
    let path = r["path"].as_str().unwrap().to_string();
    let list = run(&mut s, "symbol.libraries", json!({}));
    let user: Vec<&Value> = list["libraries"].as_array().unwrap().iter().filter(|l| l["category"] == "user").collect();
    assert_eq!(user.len(), 1);
    assert_eq!((user[0]["name"].as_str(), user[0]["count"].as_u64()), (Some("My: Marks"), Some(2)));
    assert_eq!(run(&mut s, "symbol.loadLibrary", json!({ "path": path }))["library"], "user/My- Marks.vcsymbols");
    let _ = std::fs::remove_dir_all(dir);

    // Another document's symbols load as a library.
    let id = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    run(&mut s, "symbol.new", json!({"name": "Poster Mark", "ids": [id.0]}));
    let b64 = run(&mut s, "document.serialize", json!({}))["dataBase64"].as_str().unwrap().to_string();
    let r = run(&mut s, "symbol.loadLibrary", json!({"dataBase64": b64, "name": "Poster.vectorcraft"}));
    assert_eq!((r["name"].as_str(), r["count"].as_u64()), (Some("Poster"), Some(3)));
    let (_, lib) = cmd::symbollib::library(&s, r["library"].as_str().unwrap()).unwrap();
    assert!(lib.symbol("Poster Mark").is_some() && lib.size("Poster Mark") == Some((10.0, 10.0)));
    assert!(s.execute("symbol.loadLibrary", &json!({"data": "hello", "name": "x.txt"})).is_err());
}
