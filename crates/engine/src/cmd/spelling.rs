//! Edit › Spelling: Check Spelling (find misspelled and repeated words and sentences that start in
//! lower case, with suggestions), Change and Change All, and the custom dictionary. The word list
//! and the checks are `vectorcraft_text::spell`; the custom dictionary and the check's options
//! are kept with the preferences ([`SpellingPrefs`], not reset with them: they are the user's
//! words, not settings).

use serde_json::{Value, json};
use vectorcraft_doc::{NodeId, NodeKind};
use vectorcraft_text::edit;
use vectorcraft_text::spell::{self, SpellOptions, WordList};

use super::typecmd::refresh_bounds;
use super::typemenu::{all_texts, text_ids};
use super::*;

/// The custom dictionary and Check Spelling's options (`Prefs::spelling`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SpellingPrefs {
    /// Words added to the dictionary (lowercase, sorted).
    pub words: Vec<String>,
    #[serde(flatten)]
    pub options: SpellOptions,
}

/// Longest custom-dictionary word kept, in characters.
const MAX_WORD: usize = 64;
/// How many custom-dictionary words are kept.
const MAX_WORDS: usize = 20_000;
/// Findings one check reports.
const MAX_FINDINGS: usize = 1_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "type.checkSpelling",
            "Check Spelling…",
            [],
            None,
            "{ids? (default: every visible, unlocked text object), ignore?: [word] (Ignore All this session), suggestions?: true, max?: 8 (suggestions per word)} → {findings: [{id, start, end (bytes of the object's text), word, issue: \"misspelled\"|\"repeated\"|\"uncapitalized\", suggestions}]} in document order (the first 1000)",
            has_doc,
            check
        ),
        cmd!(
            "type.spelling.change",
            "Change",
            [],
            None,
            "{id, start, end, to, word? (refused unless the text there still reads word)} replace one finding → {id, end (after the change)}",
            has_doc,
            change
        ),
        cmd!(
            "type.spelling.changeAll",
            "Change All",
            [],
            None,
            "{word, to, ids?} replace every whole-word occurrence of word (same case) in every text object → {count}",
            has_doc,
            change_all
        ),
        cmd!(
            query "type.spelling.dictionary",
            "Edit Custom Dictionary…",
            [],
            None,
            "{add?: [word]|word, remove?: [word]|word, words?: [word] (replace the list)} the words Check Spelling accepts besides its own (kept with the preferences) → {words}",
            always,
            dictionary
        ),
        cmd!(
            query "type.spelling.options",
            "Check Spelling Options",
            [],
            None,
            "{repeatedWords?, uncapitalizedStart?, ignoreUppercase?, ignoreRomanNumerals?, ignoreWordsWithNumbers?: bool, defaults?: true} what Check Spelling looks for and skips (kept with the preferences) → the options",
            always,
            options
        ),
    ]
}

fn issue_name(i: spell::Issue) -> &'static str {
    match i {
        spell::Issue::Misspelled => "misspelled",
        spell::Issue::Repeated => "repeated",
        spell::Issue::Uncapitalized => "uncapitalized",
    }
}

fn words_of(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => vec![],
    }
}

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let ids = match ids_param(p, "ids") {
        Some(v) => text_ids(&st.doc, &v),
        None => all_texts(&st.doc),
    };
    let ignore: Vec<String> = words_of(p.get("ignore")).iter().map(|w| w.to_lowercase()).collect();
    let want = bool_or(p, "suggestions", true);
    let max = p.get("max").and_then(Value::as_u64).unwrap_or(8).min(32) as usize;
    let prefs = &s.prefs.spelling;
    let list = WordList::en_us();
    let mut out = vec![];
    'all: for id in ids {
        let Some(NodeKind::Text(t)) = st.doc.node(id).map(|n| &n.kind) else { continue };
        let text = t.plain_text();
        for f in spell::check(&text, list, &prefs.words, &prefs.options) {
            if ignore.contains(&f.word.to_lowercase()) {
                continue;
            }
            let suggestions = if !want {
                vec![]
            } else if f.issue == spell::Issue::Repeated {
                // Change to nothing removes the repeat (with the space before it).
                vec![]
            } else {
                spell::suggestions(&f.word, list, &prefs.words, max)
            };
            out.push(json!({"id": id.0, "start": f.start, "end": f.end, "word": f.word, "issue": issue_name(f.issue), "suggestions": suggestions}));
            if out.len() >= MAX_FINDINGS {
                break 'all;
            }
        }
    }
    Ok(json!({ "findings": out }))
}

fn change(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "type.spelling.change";
    let id = id_param(p, "id").ok_or_else(|| bad(C, "missing `id`"))?;
    let to = str_param(p, "to").ok_or_else(|| bad(C, "missing `to`"))?.to_string();
    let (Some(a), Some(b)) = (p.get("start").and_then(Value::as_u64), p.get("end").and_then(Value::as_u64)) else {
        return Err(bad(C, "missing `start` or `end`"));
    };
    let (a, b) = (usize::try_from(a).unwrap_or(usize::MAX), usize::try_from(b).unwrap_or(usize::MAX));
    let text = match s.doc()?.doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Text(t)) => t.plain_text(),
        _ => return Err(bad(C, format!("{} is not a text object", id.0))),
    };
    let Some(here) = text.get(a..b) else { return Err(bad(C, "the range isn't in the text")) };
    if let Some(w) = str_param(p, "word")
        && w != here
    {
        return Err(bad(C, "the text has changed since it was checked"));
    }
    // Deleting a repeated word takes the space before it too.
    let a = if to.is_empty() { text.get(..a).map_or(a, |t| t.trim_end_matches(char::is_whitespace).len()) } else { a };
    let end = s.edit("Change", |d, _| {
        let Some(NodeKind::Text(t)) = d.node_mut(id).map(|n| &mut n.kind) else { return Err(EngineError::NoNode(id)) };
        t.splice_paras(a, b, &to);
        let end = edit::replace_range(&mut t.runs, a, b, &to);
        refresh_bounds(t);
        Ok(end)
    })?;
    Ok(json!({"id": id.0, "end": end}))
}

fn change_all(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "type.spelling.changeAll";
    let word = str_param(p, "word").filter(|w| !w.is_empty()).ok_or_else(|| bad(C, "missing `word`"))?.to_string();
    let to = str_param(p, "to").ok_or_else(|| bad(C, "missing `to`"))?.to_string();
    let ids: Vec<NodeId> = match ids_param(p, "ids") {
        Some(v) => text_ids(&s.doc()?.doc, &v),
        None => all_texts(&s.doc()?.doc),
    };
    // Every whole-word occurrence per object, as the checker splits words.
    let hits =
        |text: &str| -> Vec<(usize, usize)> { spell::words(text).into_iter().filter(|&(a, b)| text.get(a..b) == Some(word.as_str())).collect() };
    let st = s.doc()?;
    let work: Vec<(NodeId, Vec<(usize, usize)>)> = ids
        .into_iter()
        .filter_map(|id| match st.doc.node(id).map(|n| &n.kind) {
            Some(NodeKind::Text(t)) => Some((id, hits(&t.plain_text()))),
            _ => None,
        })
        .filter(|(_, h)| !h.is_empty())
        .collect();
    let count: usize = work.iter().map(|(_, h)| h.len()).sum();
    if count == 0 {
        return Ok(json!({ "count": 0 }));
    }
    s.edit("Change All", |d, _| {
        for (id, h) in &work {
            let Some(NodeKind::Text(t)) = d.node_mut(*id).map(|n| &mut n.kind) else { continue };
            // Back to front, so earlier offsets stay valid.
            for &(a, b) in h.iter().rev() {
                t.splice_paras(a, b, &to);
                edit::replace_range(&mut t.runs, a, b, &to);
            }
            refresh_bounds(t);
        }
        Ok(())
    })?;
    Ok(json!({ "count": count }))
}

/// A custom-dictionary word as stored: trimmed, lowercase, one word.
fn clean_word(w: &str) -> Option<String> {
    let w = w.trim().to_lowercase().replace('’', "'");
    (!w.is_empty() && w.chars().count() <= MAX_WORD && !w.chars().any(char::is_whitespace)).then_some(w)
}

fn dictionary(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "type.spelling.dictionary";
    let mut words = s.prefs.spelling.words.clone();
    if p.get("words").is_some() {
        words = words_of(p.get("words")).iter().filter_map(|w| clean_word(w)).collect();
    }
    for w in words_of(p.get("add")) {
        words.push(clean_word(&w).ok_or_else(|| bad(C, format!("`{w}` isn't a word")))?);
    }
    let remove: Vec<String> = words_of(p.get("remove")).iter().filter_map(|w| clean_word(w)).collect();
    words.retain(|w| !remove.contains(w));
    words.sort();
    words.dedup();
    if words.len() > MAX_WORDS {
        return Err(bad(C, format!("the dictionary holds at most {MAX_WORDS} words")));
    }
    s.prefs.spelling.words = words;
    Ok(json!({ "words": s.prefs.spelling.words }))
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "type.spelling.options";
    let mut o = if bool_or(p, "defaults", false) { SpellOptions::default() } else { s.prefs.spelling.options };
    for (k, slot) in [
        ("repeatedWords", &mut o.repeated_words),
        ("uncapitalizedStart", &mut o.uncapitalized_start),
        ("ignoreUppercase", &mut o.ignore_uppercase),
        ("ignoreRomanNumerals", &mut o.ignore_roman_numerals),
        ("ignoreWordsWithNumbers", &mut o.ignore_words_with_numbers),
    ] {
        if let Some(v) = p.get(k) {
            *slot = v.as_bool().ok_or_else(|| bad(C, format!("{k} must be true or false")))?;
        }
    }
    s.prefs.spelling.options = o;
    Ok(json!(o))
}
