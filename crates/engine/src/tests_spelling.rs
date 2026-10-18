//! Edit › Spelling: Check Spelling, Change, Change All and the custom dictionary.

use serde_json::{Value, json};
use vectorcraft_doc::{NodeId, NodeKind};

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
    s
}

fn text(s: &mut Session, t: &str) -> NodeId {
    NodeId(s.execute("text.create", &json!({"x": 50, "y": 50, "text": t})).unwrap()["id"].as_u64().unwrap())
}

fn plain(s: &Session, id: NodeId) -> String {
    match &s.doc().unwrap().doc.node(id).unwrap().kind {
        NodeKind::Text(t) => t.plain_text(),
        _ => panic!("not text"),
    }
}

fn findings(s: &mut Session, p: Value) -> Vec<Value> {
    s.execute("type.checkSpelling", &p).unwrap()["findings"].as_array().cloned().unwrap()
}

#[test]
fn check_spelling_finds_words_and_changes_them() {
    let mut s = session();
    let a = text(&mut s, "Teh quick brown fox jumpd over the the lazy dog.");
    let b = text(&mut s, "Nothing wrong here. NASA flies XIV rockets.");
    let f = findings(&mut s, json!({}));
    let words: Vec<(&str, &str)> = f.iter().map(|f| (f["word"].as_str().unwrap(), f["issue"].as_str().unwrap())).collect();
    assert_eq!(words, [("Teh", "misspelled"), ("jumpd", "misspelled"), ("the", "repeated")]);
    assert!(f.iter().all(|f| f["id"] == a.0));
    assert_eq!(f[0]["suggestions"][0], "The");
    assert!(f[1]["suggestions"].as_array().unwrap().iter().any(|s| s == "jumped"), "{}", f[1]);
    // Change one finding; a stale one is refused.
    let undo = s.doc().unwrap().history.undo.len();
    let r = s.execute("type.spelling.change", &json!({"id": a.0, "start": f[0]["start"], "end": f[0]["end"], "word": "Teh", "to": "The"})).unwrap();
    assert_eq!(r["end"], 3);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    assert!(s.execute("type.spelling.change", &json!({"id": a.0, "start": 0, "end": 3, "word": "Teh", "to": "x"})).is_err());
    assert!(s.execute("type.spelling.change", &json!({"id": a.0, "start": 0, "end": 999, "to": "x"})).is_err());
    assert!(s.execute("type.spelling.change", &json!({"id": b.0 + 1000, "start": 0, "end": 1, "to": "x"})).is_err());
    // Changing a repeat to nothing removes it and the space before it.
    let f = findings(&mut s, json!({"ids": [a.0]}));
    let rep = f.iter().find(|f| f["issue"] == "repeated").unwrap();
    s.execute("type.spelling.change", &json!({"id": a.0, "start": rep["start"], "end": rep["end"], "to": ""})).unwrap();
    assert_eq!(plain(&s, a), "The quick brown fox jumpd over the lazy dog.");
    // Ignore All skips a word; the custom dictionary accepts it for good.
    assert!(findings(&mut s, json!({"ignore": ["JUMPD"]})).is_empty());
    s.execute("type.spelling.dictionary", &json!({"add": "Jumpd"})).unwrap();
    assert!(findings(&mut s, json!({})).is_empty());
    assert_eq!(s.prefs.spelling.words, ["jumpd"]);
}

#[test]
fn change_all_replaces_whole_words_everywhere() {
    let mut s = session();
    let a = text(&mut s, "colour colours colour");
    let b = text(&mut s, "Colour, colour!");
    let r = s.execute("type.spelling.changeAll", &json!({"word": "colour", "to": "color"})).unwrap();
    assert_eq!(r["count"], 3);
    assert_eq!(plain(&s, a), "color colours color");
    assert_eq!(plain(&s, b), "Colour, color!");
    assert_eq!(s.execute("type.spelling.changeAll", &json!({"word": "zzz", "to": "y"})).unwrap()["count"], 0);
    assert!(s.execute("type.spelling.changeAll", &json!({"word": "", "to": "y"})).is_err());
}

#[test]
fn options_and_dictionary_live_with_the_preferences() {
    let mut s = session();
    text(&mut s, "it was the the end. ABCD.");
    assert_eq!(findings(&mut s, json!({})).len(), 2, "an uncapitalized start and a repeat");
    let o = s.execute("type.spelling.options", &json!({"repeatedWords": false, "uncapitalizedStart": false, "ignoreUppercase": false})).unwrap();
    assert_eq!(o["repeatedWords"], false);
    let f = findings(&mut s, json!({"suggestions": false}));
    assert_eq!(f.len(), 1, "ABCD, now that capitals are checked");
    assert_eq!(f[0]["suggestions"], json!([]));
    assert!(s.execute("type.spelling.options", &json!({"repeatedWords": "yes"})).is_err());
    assert_eq!(s.execute("type.spelling.options", &json!({"defaults": true})).unwrap()["repeatedWords"], true);
    // The dictionary: add, remove, replace; bad words are refused.
    let w = s.execute("type.spelling.dictionary", &json!({"add": ["Zorp", "blerg", "zorp"]})).unwrap();
    assert_eq!(w["words"], json!(["blerg", "zorp"]));
    let w = s.execute("type.spelling.dictionary", &json!({"remove": "ZORP"})).unwrap();
    assert_eq!(w["words"], json!(["blerg"]));
    assert!(s.execute("type.spelling.dictionary", &json!({"add": "two words"})).is_err());
    assert!(s.execute("type.spelling.dictionary", &json!({"add": "x".repeat(65)})).is_err());
    assert_eq!(s.execute("type.spelling.dictionary", &json!({"words": []})).unwrap()["words"], json!([]));
    // Resetting the preferences keeps the user's words.
    s.execute("type.spelling.dictionary", &json!({"add": "blerg"})).unwrap();
    s.prefs.reset(None);
    assert_eq!(s.prefs.spelling.words, ["blerg"]);
    // And they round-trip through the preferences' JSON.
    let back: Prefs = serde_json::from_value(serde_json::to_value(&s.prefs).unwrap()).unwrap();
    assert_eq!(back.spelling, s.prefs.spelling);
}

#[test]
fn checking_works_without_a_selection_or_text() {
    let mut s = session();
    assert!(findings(&mut s, json!({})).is_empty());
    s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    assert!(findings(&mut s, json!({"ids": [999_999]})).is_empty());
}
