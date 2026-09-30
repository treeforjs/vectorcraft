//! The native `.drawcraft` format.
//!
//! A `.drawcraft` file is UTF-8 JSON:
//! ```json
//! { "format": "drawcraft", "version": 1, "generator": "DrawCraft 0.1.0",
//!   "document": { …drawcraft_doc::Document… },
//!   "images": { "<key>": { "mime": "image/png", "data": "<base64>" } } }
//! ```
//! It is lossless for everything in the document model and preserves unknown fields under
//! `document.unknown`. Readers must reject files whose `version` is newer than they support.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use drawcraft_doc::{Document, ImageBlob};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "drawcraft";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a DrawCraft file: {0}")]
    NotDrawcraft(String),
    #[error("file version {0} is newer than this DrawCraft supports ({VERSION})")]
    TooNew(u32),
    #[error("invalid image data for `{0}`")]
    BadImage(String),
}

#[derive(Serialize, Deserialize)]
struct Image {
    mime: String,
    data: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    #[serde(default)]
    generator: String,
    document: Document,
    #[serde(default)]
    images: BTreeMap<String, Image>,
}

/// Serialize a document (pretty = human-diffable).
pub fn save(doc: &Document, pretty: bool) -> Vec<u8> {
    let images = doc.images.iter().map(|(k, b)| (k.clone(), Image { mime: b.mime.clone(), data: base64_encode(&b.bytes) })).collect();
    let f = File { format: "drawcraft".into(), version: VERSION, generator: format!("DrawCraft {}", env!("CARGO_PKG_VERSION")), document: doc.clone(), images };
    if pretty { serde_json::to_vec_pretty(&f).unwrap_or_default() } else { serde_json::to_vec(&f).unwrap_or_default() }
}

pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    let f: File = serde_json::from_slice(bytes).map_err(|e| FormatError::NotDrawcraft(e.to_string()))?;
    if f.format != "drawcraft" {
        return Err(FormatError::NotDrawcraft(format!("format is `{}`", f.format)));
    }
    if f.version > VERSION {
        return Err(FormatError::TooNew(f.version));
    }
    let mut doc = f.document;
    for (k, img) in f.images {
        let bytes = base64_decode(&img.data).ok_or_else(|| FormatError::BadImage(k.clone()))?;
        doc.images.insert(k, ImageBlob { mime: img.mime, bytes: Arc::new(bytes) });
    }
    doc.fix_next_id();
    Ok(doc)
}

/// Does this look like a `.drawcraft` file?
pub fn sniff(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(256)];
    std::str::from_utf8(head).is_ok_and(|s| s.trim_start().starts_with('{') && s.contains("\"drawcraft\""))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| B64.iter().position(|&b| b == c).map(|p| p as u32);
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !clean.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for c in clean.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &b in c {
            n <<= 6;
            if b == b'=' {
                pad += 1;
            } else {
                n |= val(b)?;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use drawcraft_doc::{Appearance, Node};
    use drawcraft_geom::{Rect, shapes};

    #[test]
    fn roundtrip() {
        let mut d = Document::new(100.0, 100.0);
        let l = d.layers[0].id;
        let id = d.alloc_id();
        d.insert(Some(l), 0, Node::path(id, shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0)), Appearance::default_art())).unwrap();
        d.images.insert("k".into(), ImageBlob { mime: "image/png".into(), bytes: Arc::new(vec![1, 2, 3, 250]) });
        let bytes = save(&d, true);
        assert!(sniff(&bytes));
        let back = load(&bytes).unwrap();
        assert_eq!(back.node_count(), d.node_count());
        assert_eq!(back.images["k"].bytes.as_slice(), &[1, 2, 3, 250]);
        assert_eq!(back.node(id).unwrap().path_data(), d.node(id).unwrap().path_data());
    }

    #[test]
    fn rejects_foreign_and_future() {
        assert!(load(b"{}").is_err());
        let d = Document::new(1.0, 1.0);
        let mut v: serde_json::Value = serde_json::from_slice(&save(&d, false)).unwrap();
        v["version"] = 99.into();
        assert!(matches!(load(&serde_json::to_vec(&v).unwrap()), Err(FormatError::TooNew(99))));
    }

    #[test]
    fn base64() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(base64_decode(&base64_encode(s)).unwrap(), s);
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
