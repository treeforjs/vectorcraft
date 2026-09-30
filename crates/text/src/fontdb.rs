//! Font database: bundled OFL fonts, user fonts, optional system font catalog, outline cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use kurbo::BezPath;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider};

/// The family used when a requested family is unknown (Illustrator's Myriad Pro analogue).
pub const FALLBACK_FAMILY: &str = "Source Sans 3";

static BUNDLED: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-Semibold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-Bold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-It.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
];

enum FontBytes {
    Static(&'static [u8]),
    Owned(Arc<Vec<u8>>),
}

/// One loaded font face.
pub struct FontFace {
    id: u32,
    /// Typographic family name (e.g. "Source Sans 3").
    pub family: String,
    /// Typographic style name (e.g. "Semibold", "Italic").
    pub style: String,
    /// usWeightClass-style weight (400 = regular).
    pub weight: f32,
    pub italic: bool,
    bytes: FontBytes,
    index: u32,
    pub(crate) upem: f64,
    /// Ascender in font units (positive = up).
    pub(crate) ascent: f64,
    /// Descender in font units (positive = down).
    pub(crate) descent: f64,
    pub(crate) shaper: harfrust::ShaperData,
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FontFace({} {})", self.family, self.style)
    }
}

impl FontFace {
    pub(crate) fn data(&self) -> &[u8] {
        match &self.bytes {
            FontBytes::Static(b) => b,
            FontBytes::Owned(v) => v.as_slice(),
        }
    }
    pub(crate) fn skrifa(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(self.data(), self.index).ok()
    }
    pub(crate) fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(self.data(), self.index).ok()
    }
    /// Unique id of this face within the process.
    pub fn id(&self) -> u32 {
        self.id
    }
    /// Does the face map `c` to a glyph?
    pub fn covers(&self, c: char) -> bool {
        self.skrifa().is_some_and(|f| f.charmap().map(c).is_some())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct CatalogEntry {
    family: String,
    style: String,
    path: std::path::PathBuf,
}

/// Process-wide font database.
pub struct FontDb {
    faces: RwLock<Vec<Arc<FontFace>>>,
    outlines: Mutex<HashMap<(u32, u32), Arc<BezPath>>>,
    #[cfg(not(target_arch = "wasm32"))]
    catalog: RwLock<Vec<CatalogEntry>>,
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
const OUTLINE_CACHE_MAX: usize = 50_000;

fn name(font: &skrifa::FontRef<'_>, ids: &[StringId]) -> Option<String> {
    ids.iter().find_map(|id| font.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
}

/// Parse every face in `data` (a font file or collection). Returns `(index, family, style)`.
fn enumerate_faces(data: &[u8]) -> Vec<(u32, String, String)> {
    let count = match FileRef::new(data) {
        Ok(FileRef::Font(_)) => 1,
        Ok(FileRef::Collection(c)) => c.len(),
        Err(_) => 0,
    };
    (0..count)
        .filter_map(|i| {
            let f = skrifa::FontRef::from_index(data, i).ok()?;
            let family = name(&f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME])?;
            let style = name(&f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
            Some((i, family, style))
        })
        .collect()
}

fn make_face(bytes: FontBytes, index: u32, family: String, style: String) -> Option<FontFace> {
    let data: &[u8] = match &bytes {
        FontBytes::Static(b) => b,
        FontBytes::Owned(v) => v.as_slice(),
    };
    let f = skrifa::FontRef::from_index(data, index).ok()?;
    let m = f.metrics(Size::unscaled(), LocationRef::default());
    let a = f.attributes();
    let shaper = harfrust::ShaperData::new(&harfrust::FontRef::from_index(data, index).ok()?);
    Some(FontFace {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        family,
        style,
        weight: a.weight.value(),
        italic: !matches!(a.style, skrifa::attribute::Style::Normal),
        upem: m.units_per_em.max(1) as f64,
        ascent: m.ascent as f64,
        descent: -(m.descent as f64),
        shaper,
        bytes,
        index,
    })
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Weight implied by a style name.
fn style_weight(style: &str) -> f32 {
    let s = norm(style);
    const TABLE: &[(&str, f32)] = &[
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("semibold", 600.0),
        ("demibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("hairline", 100.0),
        ("thin", 100.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

fn style_italic(style: &str) -> bool {
    let s = norm(style);
    s.contains("italic") || s.contains("oblique") || s == "it"
}

impl FontDb {
    fn new_bundled() -> Self {
        let mut faces = Vec::new();
        for data in BUNDLED {
            for (i, family, style) in enumerate_faces(data) {
                if let Some(f) = make_face(FontBytes::Static(data), i, family, style) {
                    faces.push(Arc::new(f));
                }
            }
        }
        Self {
            faces: RwLock::new(faces),
            outlines: Mutex::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            catalog: RwLock::new(Vec::new()),
        }
    }

    /// Process-wide database preloaded with the bundled fonts.
    pub fn global() -> &'static FontDb {
        static DB: std::sync::OnceLock<FontDb> = std::sync::OnceLock::new();
        DB.get_or_init(FontDb::new_bundled)
    }

    fn read_faces(&self) -> std::sync::RwLockReadGuard<'_, Vec<Arc<FontFace>>> {
        self.faces.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Family names available (loaded plus cataloged system fonts), sorted and deduplicated.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.read_faces().iter().map(|f| f.family.clone()).collect();
        #[cfg(not(target_arch = "wasm32"))]
        v.extend(self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter().map(|c| c.family.clone()));
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// Style names available for `family` (Regular first, then by weight).
    pub fn styles(&self, family: &str) -> Vec<String> {
        let mut v: Vec<(bool, f32, String)> =
            self.read_faces().iter().filter(|f| f.family.eq_ignore_ascii_case(family)).map(|f| (f.italic, f.weight, f.style.clone())).collect();
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter() {
            if c.family.eq_ignore_ascii_case(family) && !v.iter().any(|(_, _, s)| s.eq_ignore_ascii_case(&c.style)) {
                v.push((style_italic(&c.style), style_weight(&c.style), c.style.clone()));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        v.dedup_by(|a, b| a.2 == b.2);
        v.into_iter().map(|t| t.2).collect()
    }

    /// Add a user font (TTF/OTF/TTC bytes). Returns the number of faces added (0 if unparseable or
    /// every face was already present).
    pub fn add_font(&self, bytes: Vec<u8>) -> usize {
        let data = Arc::new(bytes);
        let mut added = 0;
        for (i, family, style) in enumerate_faces(&data) {
            if self.read_faces().iter().any(|f| f.family.eq_ignore_ascii_case(&family) && f.style.eq_ignore_ascii_case(&style)) {
                continue;
            }
            if let Some(f) = make_face(FontBytes::Owned(data.clone()), i, family, style) {
                self.faces.write().unwrap_or_else(|e| e.into_inner()).push(Arc::new(f));
                added += 1;
            }
        }
        added
    }

    /// Scan the platform's font directories and catalog the faces found (native only; not called by
    /// default). Font data is loaded lazily when a cataloged family is first resolved. Returns the
    /// number of faces cataloged.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts(&self) -> usize {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        if cfg!(target_os = "macos") {
            dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join("Library/Fonts"));
            }
        } else if cfg!(windows) {
            let root = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
            dirs.push(root.join("Fonts"));
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(std::path::PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
            }
        } else {
            dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join(".fonts"));
                dirs.push(h.join(".local/share/fonts"));
            }
        }
        let mut found = Vec::new();
        let mut stack = dirs;
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
                if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
                    continue;
                }
                let Ok(data) = std::fs::read(&p) else { continue };
                for (_, family, style) in enumerate_faces(&data) {
                    found.push(CatalogEntry { family, style, path: p.clone() });
                }
            }
        }
        let n = found.len();
        log::debug!("cataloged {n} system font faces");
        *self.catalog.write().unwrap_or_else(|e| e.into_inner()) = found;
        n
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_cataloged(&self, family: &str) -> bool {
        let paths: Vec<std::path::PathBuf> = {
            let cat = self.catalog.read().unwrap_or_else(|e| e.into_inner());
            let mut p: Vec<_> = cat.iter().filter(|c| c.family.eq_ignore_ascii_case(family)).map(|c| c.path.clone()).collect();
            p.dedup();
            p
        };
        let mut any = false;
        for p in paths {
            if let Ok(data) = std::fs::read(&p) {
                any |= self.add_font(data) > 0;
            }
        }
        any
    }

    /// Resolve a family + style to a face, falling back to the closest style of the family, then to
    /// Source Sans 3 Regular.
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        if let Some(f) = self.find(family, style) {
            return f;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.load_cataloged(family)
            && let Some(f) = self.find(family, style)
        {
            return f;
        }
        self.find(FALLBACK_FAMILY, style)
            .or_else(|| self.find(FALLBACK_FAMILY, "Regular"))
            .or_else(|| self.read_faces().first().cloned())
            .expect("bundled fonts are always present")
    }

    /// Is `family` available (loaded)?
    pub fn has_family(&self, family: &str) -> bool {
        self.read_faces().iter().any(|f| f.family.eq_ignore_ascii_case(family))
    }

    fn find(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        let faces = self.read_faces();
        let cands: Vec<&Arc<FontFace>> = faces.iter().filter(|f| f.family.eq_ignore_ascii_case(family)).collect();
        if cands.is_empty() {
            return None;
        }
        let ns = norm(style);
        if let Some(f) = cands.iter().find(|f| norm(&f.style) == ns) {
            return Some((*f).clone());
        }
        let (tw, ti) = (style_weight(style), style_italic(style));
        cands
            .iter()
            .min_by(|a, b| {
                let sa = (a.weight - tw).abs() + if a.italic != ti { 1000.0 } else { 0.0 };
                let sb = (b.weight - tw).abs() + if b.italic != ti { 1000.0 } else { 0.0 };
                sa.total_cmp(&sb)
            })
            .map(|f| (*f).clone())
    }

    /// First face (fallback family first, then load order) that covers `c`.
    pub(crate) fn fallback_for(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        let faces = self.read_faces();
        let mut order: Vec<&Arc<FontFace>> = faces.iter().filter(|f| f.id != exclude).collect();
        order.sort_by_key(|f| (!f.family.eq_ignore_ascii_case(FALLBACK_FAMILY), f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph).
    pub(crate) fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        let key = (face.id, gid);
        if let Some(p) = self.outlines.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return p.clone();
        }
        let mut pen = FlipPen(BezPath::new());
        if let Some(f) = face.skrifa()
            && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
        {
            let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut pen);
        }
        let p = Arc::new(pen.0);
        let mut cache = self.outlines.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= OUTLINE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, p.clone());
        p
    }
}

struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, -y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, -y as f64));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, -cy0 as f64), (x as f64, -y as f64));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, -cy0 as f64), (cx1 as f64, -cy1 as f64), (x as f64, -y as f64));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}
