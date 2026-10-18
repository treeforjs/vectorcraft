//! Check Spelling: words checked against the bundled US English word list (the public-domain Moby
//! Hyphenator II list by Grady Ward, `assets/spelling/en-us.dic`) plus a few modern words it
//! lacks and the user's custom dictionary, with simple inflections of known stems accepted.
//! Suggestions are the list's words within a small edit distance (Damerau–Levenshtein).
//!
//! [`check`] finds misspelled words and, when asked, repeated words and sentences that start in
//! lower case. Byte ranges are into the text checked.

use std::io::Read;
use std::sync::OnceLock;

static EN_US: &[u8] = include_bytes!("../../../assets/spelling/en-us.dic");

/// Words in common use that the 2001 list predates or leaves out.
const EXTRA: &[&str] = &[
    "app",
    "apps",
    "artboard",
    "artboards",
    "backend",
    "bezier",
    "beziers",
    "blog",
    "blogs",
    "bookmark",
    "bookmarked",
    "checkbox",
    "copywriter",
    "cyber",
    "dataset",
    "datasets",
    "desktop",
    "download",
    "downloads",
    "downloaded",
    "dropdown",
    "ebook",
    "email",
    "emails",
    "emailed",
    "emoji",
    "favicon",
    "filename",
    "filenames",
    "fullscreen",
    "gif",
    "gifs",
    "hashtag",
    "homepage",
    "hyperlink",
    "iframe",
    "inbox",
    "infographic",
    "infographics",
    "internet",
    "jpeg",
    "kerning",
    "laptop",
    "login",
    "logout",
    "lowercase",
    "metadata",
    "mockup",
    "mockups",
    "multiline",
    "offline",
    "online",
    "pdf",
    "pdfs",
    "pixelated",
    "plugin",
    "plugins",
    "png",
    "podcast",
    "popup",
    "prepress",
    "printable",
    "redo",
    "rgb",
    "screenshot",
    "screenshots",
    "sitemap",
    "smartphone",
    "smartphones",
    "spam",
    "startup",
    "startups",
    "subfolder",
    "svg",
    "tablet",
    "teardown",
    "toolbar",
    "toolbars",
    "touchscreen",
    "tracklist",
    "typeface",
    "typefaces",
    "upload",
    "uploads",
    "uploaded",
    "uppercase",
    "url",
    "urls",
    "username",
    "usernames",
    "vectorize",
    "vectorized",
    "walkthrough",
    "webcam",
    "webinar",
    "webpage",
    "webpages",
    "website",
    "websites",
    "widget",
    "widgets",
    "wifi",
    "workflow",
    "workflows",
    "workspace",
    "workspaces",
    "zoom",
];

/// Entries of the list that are common misspellings, not words.
const NOT_WORDS: &[&str] = &["teh"];

/// A sorted list of lowercase words.
#[derive(Clone, Debug, Default)]
pub struct WordList {
    words: String,
    offs: Vec<u32>,
}

/// Header of the deflated list (shared with DesignCraft's hyphenation dictionary).
const MAGIC: &[u8] = b"DCHYPH1\n";
/// The inflated list is about 2 MB; anything much larger is not ours.
const MAX_INFLATED: u64 = 16 << 20;

impl WordList {
    /// Decode a list stored as DesignCraft's hyphenation dictionary: raw deflate of [`MAGIC`] then
    /// one front-coded line per word (a byte with the length shared with the previous line, then
    /// the rest; `-` marks hyphenation points and is dropped here).
    pub fn from_bytes(data: &[u8]) -> Result<WordList, String> {
        let mut raw = Vec::new();
        flate2::read::DeflateDecoder::new(data).take(MAX_INFLATED).read_to_end(&mut raw).map_err(|e| format!("word list: inflate failed: {e}"))?;
        let body = raw.strip_prefix(MAGIC).ok_or("word list: bad header")?;
        let mut words: Vec<String> = Vec::new();
        let mut cur: Vec<u8> = Vec::new();
        let mut rest = body;
        while let Some((&p, tail)) = rest.split_first() {
            let end = tail.iter().position(|&b| b == b'\n').ok_or("word list: truncated")?;
            if usize::from(p) > cur.len() {
                return Err("word list: bad prefix".into());
            }
            cur.truncate(usize::from(p));
            cur.extend_from_slice(tail.get(..end).unwrap_or_default());
            rest = tail.get(end + 1..).unwrap_or_default();
            let s = std::str::from_utf8(&cur).map_err(|_| "word list: bad utf-8")?;
            words.push(s.replace('-', ""));
        }
        Ok(WordList::from_words(words))
    }

    /// A list of `words` (lowercased, sorted and de-duplicated here).
    pub fn from_words(words: impl IntoIterator<Item = String>) -> WordList {
        let mut v: Vec<String> = words.into_iter().map(|w| w.to_lowercase()).filter(|w| !w.is_empty()).collect();
        v.sort_unstable();
        v.dedup();
        let mut l = WordList { words: String::new(), offs: vec![0] };
        for w in v {
            l.words.push_str(&w);
            l.offs.push(u32::try_from(l.words.len()).unwrap_or(u32::MAX));
        }
        l
    }

    /// The bundled US English list with [`EXTRA`] added and [`NOT_WORDS`] taken out (decoded on
    /// first use; empty if the bundled file can't be read).
    pub fn en_us() -> &'static WordList {
        static L: OnceLock<WordList> = OnceLock::new();
        L.get_or_init(|| {
            let base = WordList::from_bytes(EN_US).unwrap_or_else(|e| {
                log::error!("{e}");
                WordList::default()
            });
            WordList::from_words(base.iter().filter(|w| !NOT_WORDS.contains(w)).map(str::to_string).chain(EXTRA.iter().map(|w| w.to_string())))
        })
    }

    pub fn len(&self) -> usize {
        self.offs.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn word(&self, i: usize) -> &str {
        let (Some(&a), Some(&b)) = (self.offs.get(i), self.offs.get(i + 1)) else { return "" };
        self.words.get(a as usize..b as usize).unwrap_or("")
    }

    /// Is `w` (lowercase) in the list?
    pub fn contains(&self, w: &str) -> bool {
        let (mut lo, mut hi) = (0usize, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.word(mid).cmp(w) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> + '_ {
        (0..self.len()).map(|i| self.word(i))
    }
}

/// Check Spelling's options (Illustrator's Find and Ignore groups).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SpellOptions {
    /// Report a word written twice in a row ("the the").
    pub repeated_words: bool,
    /// Report a sentence that starts with a lowercase letter.
    pub uncapitalized_start: bool,
    /// Skip words in all capitals (acronyms).
    pub ignore_uppercase: bool,
    /// Skip Roman numerals (XIV).
    pub ignore_roman_numerals: bool,
    /// Skip words containing digits (A4, mp3).
    pub ignore_words_with_numbers: bool,
}

impl Default for SpellOptions {
    fn default() -> Self {
        Self { repeated_words: true, uncapitalized_start: true, ignore_uppercase: true, ignore_roman_numerals: true, ignore_words_with_numbers: true }
    }
}

/// What kind of problem a [`Finding`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Issue {
    Misspelled,
    Repeated,
    Uncapitalized,
}

/// A problem in checked text: the byte range of the word and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub word: String,
    pub issue: Issue,
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’')
}

/// Words of `text`: runs of letters, digits and inner apostrophes or hyphens → (start, end).
pub fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start: Option<usize> = None;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let next_is_word = it.peek().is_some_and(|&(_, n)| n.is_alphanumeric());
        let inner = (is_apostrophe(c) || c == '-') && start.is_some() && next_is_word;
        if c.is_alphanumeric() || inner {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push((s, i));
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

fn is_roman(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| matches!(c.to_ascii_uppercase(), 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
}

/// Is `w` a known word: in `list` or `user` (both lowercase), or a simple inflection of one?
/// Hyphenated words are known when each part is.
pub fn known(list: &WordList, user: &[String], w: &str) -> bool {
    let w = w.to_lowercase().replace('’', "'");
    if w.contains('-') {
        return w.split('-').all(|p| p.is_empty() || known(list, user, p));
    }
    let has = |x: &str| x.chars().count() > 1 && (list.contains(x) || user.iter().any(|u| u == x));
    if w.chars().count() <= 1 || has(&w) {
        return true;
    }
    // Possessives and contractions of known words.
    for suf in ["'s", "'d", "'ll", "'re", "'ve", "n't", "'"] {
        if let Some(base) = w.strip_suffix(suf)
            && has(base)
        {
            return true;
        }
    }
    const STEMS: [(&str, &[&str]); 11] = [
        ("ies", &["y"]),
        ("ied", &["y"]),
        ("ily", &["y"]),
        ("es", &["", "e"]),
        ("s", &[""]),
        ("ed", &["", "e"]),
        ("ing", &["", "e"]),
        ("ly", &["", "le"]),
        ("er", &["", "e"]),
        ("est", &["", "e"]),
        ("ness", &[""]),
    ];
    for (suf, adds) in STEMS {
        let Some(base) = w.strip_suffix(suf) else { continue };
        if adds.iter().any(|add| has(&format!("{base}{add}"))) {
            return true;
        }
        // A doubled final consonant: "stopped" → "stop".
        let mut cs = base.chars().rev();
        if let (Some(a), Some(b)) = (cs.next(), cs.next())
            && a == b
            && !"aeiou".contains(a)
            && has(base.get(..base.len().saturating_sub(a.len_utf8())).unwrap_or(""))
        {
            return true;
        }
    }
    false
}

/// The problems in `text`, in order.
pub fn check(text: &str, list: &WordList, user: &[String], opts: &SpellOptions) -> Vec<Finding> {
    let mut out = vec![];
    let mut prev: Option<(usize, usize)> = None;
    for (a, b) in words(text) {
        let Some(w) = text.get(a..b) else { continue };
        let finding = |issue| Finding { start: a, end: b, word: w.to_string(), issue };
        let between = prev.and_then(|(_, pe)| text.get(pe..a));
        if opts.repeated_words
            && let (Some((pa, pb)), Some(gap)) = (prev, between)
            && !gap.is_empty()
            && gap.chars().all(char::is_whitespace)
            && text.get(pa..pb).is_some_and(|p| p.to_lowercase() == w.to_lowercase())
            && w.chars().any(char::is_alphabetic)
        {
            out.push(finding(Issue::Repeated));
            prev = Some((a, b));
            continue;
        }
        let sentence_start = match between {
            None => text.get(..a).is_some_and(|t| !t.chars().any(char::is_alphanumeric)),
            Some(gap) => gap.trim_end_matches(|c: char| c.is_whitespace() || "\"'“‘(".contains(c)).ends_with(['.', '!', '?']),
        };
        prev = Some((a, b));
        if opts.ignore_words_with_numbers && w.chars().any(|c| c.is_numeric()) {
            continue;
        }
        if !w.chars().any(char::is_alphabetic) {
            continue;
        }
        let letters: Vec<char> = w.chars().filter(|c| c.is_alphabetic()).collect();
        let upper = letters.len() > 1 && letters.iter().all(|c| c.is_uppercase());
        if (opts.ignore_uppercase && upper) || (opts.ignore_roman_numerals && is_roman(w) && w.chars().all(char::is_uppercase)) {
            continue;
        }
        // Only words in a script the list covers are checked.
        if !w.chars().all(|c| !c.is_alphabetic() || c.is_ascii() || ('\u{C0}'..='\u{24F}').contains(&c)) {
            continue;
        }
        if !known(list, user, w) {
            out.push(finding(Issue::Misspelled));
        } else if opts.uncapitalized_start && sentence_start && w.chars().next().is_some_and(char::is_lowercase) {
            out.push(finding(Issue::Uncapitalized));
        }
    }
    out
}

/// Damerau–Levenshtein distance of `a` and `b`, or None when it exceeds `max`.
fn distance(a: &[char], b: &[char], max: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let w = b.len() + 1;
    let mut prev2: Vec<usize> = vec![];
    let mut prev: Vec<usize> = (0..w).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; w];
        let ai = *a.get(i - 1)?;
        let mut best = i;
        for j in 1..w {
            let bj = *b.get(j - 1)?;
            let cost = usize::from(ai != bj);
            let mut v = (prev.get(j)? + 1).min(cur.get(j - 1)? + 1).min(prev.get(j - 1)? + cost);
            if i > 1 && j > 1 && a.get(i - 2) == Some(&bj) && b.get(j - 2) == Some(&ai) {
                v = v.min(prev2.get(j - 2)? + 1);
            }
            if let Some(slot) = cur.get_mut(j) {
                *slot = v;
            }
            best = best.min(v);
        }
        if best > max {
            return None;
        }
        prev2 = std::mem::replace(&mut prev, cur);
    }
    prev.last().copied().filter(|&d| d <= max)
}

/// `word` with the capitalization of `like` (all caps, capitalized or as is).
fn match_case(word: &str, like: &str) -> String {
    let letters: Vec<char> = like.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return word.to_uppercase();
    }
    if like.chars().next().is_some_and(char::is_uppercase) {
        let mut cs = word.chars();
        return cs.next().map(|f| f.to_uppercase().chain(cs).collect()).unwrap_or_default();
    }
    word.to_string()
}

/// Every string one deletion, transposition, substitution or insertion (of a–z) from `w`.
fn edits1(w: &[char]) -> Vec<String> {
    let mut out = vec![];
    let s = |v: &[char]| v.iter().collect::<String>();
    for i in 0..=w.len() {
        let (a, b) = w.split_at(i);
        if let Some((_, rest)) = b.split_first() {
            out.push(s(a) + &s(rest));
            if let Some((&x, rest2)) = rest.split_first() {
                out.push(s(a) + &x.to_string() + &b.first().map(char::to_string).unwrap_or_default() + &s(rest2));
            }
            for c in 'a'..='z' {
                out.push(s(a) + &c.to_string() + &s(rest));
            }
        }
        for c in 'a'..='z' {
            out.push(s(a) + &c.to_string() + &s(b));
        }
    }
    out
}

/// Up to `max` suggestions for `word`: known words within edit distance 2 (1 for short words),
/// closest first, in the word's capitalization. A word reported as uncapitalized gets its
/// capitalized form.
pub fn suggestions(word: &str, list: &WordList, user: &[String], max: usize) -> Vec<String> {
    let lower = word.to_lowercase().replace('’', "'");
    if known(list, user, &lower) {
        let mut cs = word.chars();
        return cs.next().map(|f| f.to_uppercase().chain(cs).collect::<String>()).into_iter().filter(|s| s != word).collect();
    }
    let target: Vec<char> = lower.chars().collect();
    if target.is_empty() || target.len() > 40 {
        return vec![];
    }
    let limit = if target.len() <= 4 { 1 } else { 2 };
    let first = target.first().copied();
    let sorted = |v: &[char]| {
        let mut v = v.to_vec();
        v.sort_unstable();
        v
    };
    let letters = sorted(&target);
    // Closest first; among equals, the same letters swapped ("teh" → "the"), then the same first
    // letter, then the same length.
    let mut found: Vec<(usize, bool, bool, usize, String)> = vec![];
    for cand in list.iter().chain(user.iter().map(String::as_str)) {
        let n = cand.chars().count();
        if n.abs_diff(target.len()) > limit || n < 2 {
            continue;
        }
        let c: Vec<char> = cand.chars().collect();
        if let Some(d) = distance(&target, &c, limit) {
            found.push((d, sorted(&c) != letters, c.first().copied() != first, n.abs_diff(target.len()), cand.to_string()));
        }
    }
    // One edit away and known through an inflection ("jumpd" → "jumped"): the list holds stems.
    let mut seen: std::collections::HashSet<String> = found.iter().map(|f| f.4.clone()).collect();
    for cand in edits1(&target) {
        let c: Vec<char> = cand.chars().collect();
        if c.len() > 1 && !seen.contains(&cand) && known(list, user, &cand) {
            found.push((1, sorted(&c) != letters, c.first().copied() != first, c.len().abs_diff(target.len()), cand.clone()));
            seen.insert(cand);
        }
    }
    found.sort();
    found.dedup_by(|a, b| a.4 == b.4);
    found.into_iter().take(max).map(|(.., w)| match_case(&w, word)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> SpellOptions {
        SpellOptions::default()
    }

    #[test]
    fn the_bundled_list_decodes() {
        let l = WordList::en_us();
        assert!(l.len() > 160_000, "{}", l.len());
        for w in ["the", "colour", "color", "typography", "website", "artboard"] {
            assert!(l.contains(w), "{w}");
        }
        assert!(!l.contains("teh") && !l.contains("recieve"));
        assert!(WordList::from_bytes(b"not deflate").is_err());
        assert!(WordList::from_bytes(&[]).is_err());
    }

    #[test]
    fn misspellings_repeats_and_lowercase_starts_are_found() {
        let l = WordList::en_us();
        let text = "The quick brown fox jumpd over the the lazy dog. it recieved NASA's XIV mp3 files, didn't it?";
        let f = check(text, l, &[], &opts());
        let got: Vec<(&str, Issue)> = f.iter().map(|f| (&text[f.start..f.end], f.issue)).collect();
        assert_eq!(got, [("jumpd", Issue::Misspelled), ("the", Issue::Repeated), ("it", Issue::Uncapitalized), ("recieved", Issue::Misspelled)]);
        let off = SpellOptions { repeated_words: false, uncapitalized_start: false, ignore_uppercase: false, ..opts() };
        let got: Vec<String> = check(text, l, &[], &off).into_iter().map(|f| f.word).collect();
        assert_eq!(got, ["jumpd", "recieved"], "NASA's is a known word in capitals");
        // The user's words count.
        assert!(check("Jumpd", l, &["jumpd".into()], &opts()).is_empty());
    }

    #[test]
    fn inflections_of_known_words_are_known() {
        let l = WordList::en_us();
        for w in ["stopped", "running", "cats", "boxes", "happily", "tried", "makers", "Hello", "don't", "well-known", "Typography's"] {
            assert!(known(l, &[], w), "{w}");
        }
        for w in ["jumpd", "qwzx", "recieve"] {
            assert!(!known(l, &[], w), "{w}");
        }
    }

    #[test]
    fn suggestions_are_close_and_keep_the_case() {
        let l = WordList::en_us();
        let s = suggestions("recieve", l, &[], 5);
        assert_eq!(s.first().map(String::as_str), Some("receive"), "{s:?}");
        assert!(suggestions("Teh", l, &[], 5).contains(&"The".to_string()));
        assert!(suggestions("WROLD", l, &[], 8).contains(&"WORLD".to_string()));
        assert_eq!(suggestions("it", l, &[], 3), ["It"], "an uncapitalized start gets its capital");
        assert!(suggestions(&"x".repeat(200), l, &[], 3).is_empty());
    }

    #[test]
    fn hostile_text_is_fine() {
        let l = WordList::en_us();
        for t in ["", "''", "-a-", "’’’s", "日本語のテキスト", "a\u{301}b", "--x--", "x''y", "1 1 1", ". . ."] {
            for f in check(t, l, &[], &opts()) {
                assert!(t.get(f.start..f.end).is_some(), "{t:?}");
                let _ = suggestions(&f.word, l, &[], 5);
            }
        }
    }
}
