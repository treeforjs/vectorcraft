//! Effect › SVG Filters: an SVG `<filter>` run over the object's pixels. The filter is kept in the
//! effect's parameters as XML, so a document carries every filter it uses. Lengths in the filter
//! are document units (points), as in an exported SVG; a filter region in bounding-box units is
//! taken from the object's painted bounds.
//!
//! The primitives are evaluated by resvg on an SVG made of the object's pixels as an image with
//! the filter on it. The image is the only resource it can load: `feImage` and other references
//! to files or URLs read nothing.

use std::sync::Arc;

use serde_json::Value;
use usvg::roxmltree;
use vectorcraft_geom::{Affine, Rect};

use crate::pixel::PixelSpace;

/// The longest filter (bytes of XML) an effect runs.
pub const MAX_XML: usize = 256 * 1024;

/// How far (document units) a filter region may reach beyond the object.
const MAX_OUTSET: f64 = 2000.0;

/// The filters every document offers (name, `<filter>` XML), original to VectorCraft.
pub const PRESETS: &[(&str, &str)] = &[
    ("Soft Blur", r#"<filter><feGaussianBlur stdDeviation="3"/></filter>"#),
    (
        "Soft Shadow",
        r#"<filter x="-20%" y="-20%" width="150%" height="150%"><feGaussianBlur in="SourceAlpha" stdDeviation="3"/><feOffset dx="4" dy="4" result="shadow"/><feFlood flood-color="black" flood-opacity="0.5"/><feComposite in2="shadow" operator="in"/><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>"#,
    ),
    (
        "Halo",
        r#"<filter x="-30%" y="-30%" width="160%" height="160%"><feMorphology in="SourceAlpha" operator="dilate" radius="2"/><feGaussianBlur stdDeviation="4" result="halo"/><feFlood flood-color="rgb(255,230,128)"/><feComposite in2="halo" operator="in"/><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>"#,
    ),
    ("Thicken", r#"<filter><feMorphology operator="dilate" radius="1.5"/></filter>"#),
    ("Thin", r#"<filter><feMorphology operator="erode" radius="1"/></filter>"#),
    ("Emboss", r#"<filter><feConvolveMatrix order="3" kernelMatrix="-2 -1 0 -1 1 1 0 1 2" preserveAlpha="true"/></filter>"#),
    (
        "Raised Light",
        r#"<filter x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur in="SourceAlpha" stdDeviation="2" result="bump"/><feSpecularLighting in="bump" surfaceScale="4" specularConstant="0.8" specularExponent="18" lighting-color="white" result="light"><feDistantLight azimuth="225" elevation="40"/></feSpecularLighting><feComposite in="light" in2="SourceAlpha" operator="in" result="lit"/><feComposite in="SourceGraphic" in2="lit" operator="arithmetic" k2="1" k3="1"/></filter>"#,
    ),
    (
        "Ripple",
        r#"<filter x="-5%" y="-5%" width="110%" height="110%"><feTurbulence type="turbulence" baseFrequency="0.04" numOctaves="2" seed="3" result="noise"/><feDisplacementMap in="SourceGraphic" in2="noise" scale="8" xChannelSelector="R" yChannelSelector="G"/></filter>"#,
    ),
    (
        "Speckle",
        r#"<filter><feTurbulence type="fractalNoise" baseFrequency="0.8" numOctaves="1" seed="7" result="noise"/><feColorMatrix in="noise" type="saturate" values="0" result="grey"/><feComposite in="grey" in2="SourceGraphic" operator="arithmetic" k1="0.6" k2="0.15" k3="0.85" k4="-0.1" result="mixed"/><feComposite in="mixed" in2="SourceAlpha" operator="in"/></filter>"#,
    ),
    ("Grayscale", r#"<filter><feColorMatrix type="saturate" values="0"/></filter>"#),
    ("Sepia", r#"<filter><feColorMatrix type="matrix" values="0.39 0.77 0.19 0 0  0.35 0.69 0.17 0 0  0.27 0.53 0.13 0 0  0 0 0 1 0"/></filter>"#),
    (
        "Posterize",
        r#"<filter><feComponentTransfer><feFuncR type="discrete" tableValues="0 0.33 0.67 1"/><feFuncG type="discrete" tableValues="0 0.33 0.67 1"/><feFuncB type="discrete" tableValues="0 0.33 0.67 1"/></feComponentTransfer></filter>"#,
    ),
];

/// A filter region: where the filter paints, in fractions of the object's bounds or in document
/// units.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Region {
    bbox: bool,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Region {
    /// The region over an object painted within `bounds`.
    fn rect(&self, bounds: Rect) -> Rect {
        let r = if self.bbox {
            let (bw, bh) = (bounds.width(), bounds.height());
            Rect::from_origin_size((bounds.x0 + self.x * bw, bounds.y0 + self.y * bh), (self.w * bw, self.h * bh))
        } else {
            Rect::from_origin_size((self.x, self.y), (self.w, self.h))
        };
        r.intersect(bounds.inflate(MAX_OUTSET, MAX_OUTSET))
    }
}

/// An SVG filter with its region read.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgFilter {
    /// The `<filter>` element as written.
    xml: Arc<str>,
    region: Region,
}

/// A length attribute: a number (a fraction in bounding-box units), a percentage (in user space,
/// with no viewport to refer to, a fraction of the object's bounds too), or a number with a unit we
/// ignore (`px`, `pt`).
fn length(v: Option<&str>, default: f64) -> f64 {
    let Some(s) = v.map(str::trim) else { return default };
    let x = if let Some(p) = s.strip_suffix('%') {
        p.trim().parse::<f64>().ok().map(|p| p / 100.0)
    } else {
        let n = s.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        n.trim().parse::<f64>().ok()
    };
    match x {
        Some(x) if x.is_finite() => x,
        _ => default,
    }
}

/// The `<filter>` element of `xml` (it must be the root, in the SVG namespace or none).
fn filter_root<'a, 'i>(doc: &'a roxmltree::Document<'i>) -> Option<roxmltree::Node<'a, 'i>> {
    let root = doc.root_element();
    let ns = root.tag_name().namespace();
    (root.tag_name().name() == "filter" && ns.is_none_or(|n| n == "http://www.w3.org/2000/svg")).then_some(root)
}

impl SvgFilter {
    /// Read `xml`, a `<filter>` element; `None` when it isn't one or is too long.
    pub fn new(xml: &str) -> Option<SvgFilter> {
        let xml = xml.trim();
        if xml.len() > MAX_XML {
            return None;
        }
        let doc = roxmltree::Document::parse(xml).ok()?;
        let f = filter_root(&doc)?;
        let bbox = f.attribute("filterUnits").map(str::trim) != Some("userSpaceOnUse");
        let region = Region {
            bbox,
            x: length(f.attribute("x"), -0.1),
            y: length(f.attribute("y"), -0.1),
            w: length(f.attribute("width"), 1.2).max(0.0),
            h: length(f.attribute("height"), 1.2).max(0.0),
        };
        Some(SvgFilter { xml: xml.into(), region })
    }

    /// The filter of an effect's parameters (`filter`: the XML).
    pub fn parse(p: &Value) -> Option<SvgFilter> {
        SvgFilter::new(p.get("filter")?.as_str()?)
    }

    /// The `<filter>` element.
    pub fn xml(&self) -> &str {
        &self.xml
    }

    /// How far beyond `bounds` (document units) the filter can paint.
    pub fn outset(&self, bounds: Rect) -> f64 {
        let r = self.region.rect(bounds);
        [bounds.x0 - r.x0, bounds.y0 - r.y0, r.x1 - bounds.x1, r.y1 - bounds.y1].into_iter().fold(0.0, f64::max).clamp(0.0, MAX_OUTSET)
    }

    /// The filter as an SVG `<filter>` with `id`, its region in document units for an object
    /// painted within `bounds` (what an SVG export writes, and what [`SvgFilter::apply`] runs).
    pub fn resolved(&self, id: &str, bounds: Rect) -> String {
        let Ok(doc) = roxmltree::Document::parse(&self.xml) else { return String::new() };
        let Some(f) = filter_root(&doc) else { return String::new() };
        let r = self.region.rect(bounds);
        let mut out = format!(
            "<filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            r.x0,
            r.y0,
            r.width(),
            r.height()
        );
        for a in f.attributes() {
            if matches!(a.name(), "primitiveUnits" | "color-interpolation-filters" | "style") && a.namespace().is_none() {
                out.push_str(&format!(" {}=\"{}\"", a.name(), escape(a.value())));
            }
        }
        out.push('>');
        let (Some(first), Some(last)) = (f.first_child(), f.last_child()) else {
            out.push_str("</filter>");
            return out;
        };
        out.push_str(self.xml.get(first.range().start..last.range().end).unwrap_or(""));
        out.push_str("</filter>");
        out
    }

    /// Run the filter on `data`, `w` × `h` premultiplied RGBA8 pixels lying in the document as
    /// `space` says. Does nothing when the sizes don't match; a filter that can't run leaves the
    /// pixels as they were.
    pub fn apply(&self, data: &mut [u8], w: usize, h: usize, space: &PixelSpace) {
        let ok = w > 0 && h > 0 && w.checked_mul(h).and_then(|n| n.checked_mul(4)) == Some(data.len());
        let (Ok(wu), Ok(hu)) = (u32::try_from(w), u32::try_from(h)) else { return };
        if !ok {
            return;
        }
        let run = std::panic::AssertUnwindSafe(|| self.render(data, wu, hu, space));
        if let Ok(Some(out)) = std::panic::catch_unwind(run)
            && out.len() == data.len()
        {
            data.copy_from_slice(&out);
        }
    }

    fn render(&self, data: &[u8], w: u32, h: u32, space: &PixelSpace) -> Option<Vec<u8>> {
        let png = Arc::new(encode_png(data, w, h)?);
        let view = space.to_doc.inverse();
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\
             <defs>{}</defs><g transform=\"{}\"><g filter=\"url(#vc-filter)\">\
             <image width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" image-rendering=\"optimizeSpeed\" transform=\"{}\" xlink:href=\"vc:source\"/>\
             </g></g></svg>",
            self.resolved("vc-filter", space.bounds),
            matrix(view),
            matrix(space.to_doc),
        );
        let opt = usvg::Options {
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(move |href, _| (href == "vc:source").then(|| usvg::ImageKind::PNG(png.clone()))),
            },
            ..Default::default()
        };
        let tree = usvg::Tree::from_str(&svg, &opt).ok()?;
        let mut pm = resvg::tiny_skia::Pixmap::new(w, h)?;
        resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pm.as_mut());
        Some(pm.take())
    }
}

/// `t` as an SVG `matrix(…)`.
fn matrix(t: Affine) -> String {
    let [a, b, c, d, e, f] = t.as_coeffs();
    format!("matrix({a} {b} {c} {d} {e} {f})")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// Premultiplied RGBA8 pixels as a PNG (straight alpha), compressed for speed.
fn encode_png(data: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let straight: Vec<u8> = data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            [un(r), un(g), un(b), a]
        })
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fastest);
    enc.set_filter(png::Filter::NoFilter);
    let mut wr = enc.write_header().ok()?;
    wr.write_image_data(&straight).ok()?;
    wr.finish().ok()?;
    Some(out)
}

/// The `<filter>` elements of an SVG document (name: its `id`, else "Filter n"), as written, at
/// most `max` of them.
pub fn filters_in_svg(svg: &str, max: usize) -> Vec<(String, String)> {
    let Ok(doc) = roxmltree::Document::parse(svg) else { return vec![] };
    doc.descendants()
        .filter(|n| n.tag_name().name() == "filter")
        .filter_map(|n| {
            let xml = with_namespaces(svg.get(n.range())?, n);
            SvgFilter::new(&xml)?;
            Some((n.attribute("id").unwrap_or("").to_string(), xml))
        })
        .take(max)
        .enumerate()
        .map(|(i, (id, xml))| (if id.trim().is_empty() { format!("Filter {}", i + 1) } else { id }, xml))
        .collect()
}

/// `xml` (element `n`'s text) with the `xlink` namespace declared when it uses it and its own
/// tag doesn't declare it, so it reads on its own.
pub fn with_namespaces(xml: &str, n: roxmltree::Node) -> String {
    let uses_xlink = n.descendants().any(|d| d.attributes().any(|a| a.namespace() == Some("http://www.w3.org/1999/xlink")));
    let declared = xml.split('>').next().is_some_and(|tag| tag.contains("xmlns:xlink"));
    if uses_xlink
        && !declared
        && let Some(rest) = xml.strip_prefix("<filter")
    {
        return format!("<filter xmlns:xlink=\"http://www.w3.org/1999/xlink\"{rest}");
    }
    xml.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::Point;

    fn space(w: usize, h: usize) -> PixelSpace {
        let bounds = Rect::new(0.0, 0.0, w as f64, h as f64);
        PixelSpace { to_doc: Affine::IDENTITY, px: 1.0, center: bounds.center(), bounds }
    }

    /// A `w` × `h` raster with an opaque red square in the middle third.
    fn square(w: usize, h: usize) -> Vec<u8> {
        let mut d = vec![0u8; w * h * 4];
        for y in h / 3..2 * h / 3 {
            for x in w / 3..2 * w / 3 {
                d[(y * w + x) * 4..][..4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        d
    }

    fn at(d: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [d[i], d[i + 1], d[i + 2], d[i + 3]]
    }

    #[test]
    fn every_preset_reads_and_runs() {
        for (name, xml) in PRESETS {
            let f = SvgFilter::new(xml).unwrap_or_else(|| panic!("{name} doesn't read"));
            // A mid-tone, so colour changes show too.
            let mut d: Vec<u8> = square(30, 30).as_chunks::<4>().0.iter().flat_map(|p| if p[3] == 0 { *p } else { [180, 90, 40, 255] }).collect();
            let before = d.clone();
            f.apply(&mut d, 30, 30, &space(30, 30));
            assert_ne!(d, before, "{name} changed nothing");
        }
    }

    #[test]
    fn a_blur_spreads_the_pixels_and_keeps_the_colour() {
        let f = SvgFilter::new(r#"<filter><feGaussianBlur stdDeviation="2"/></filter>"#).unwrap();
        let mut d = square(30, 30);
        f.apply(&mut d, 30, 30, &space(30, 30));
        let edge = at(&d, 30, 9, 15);
        assert!(edge[3] > 0 && edge[3] < 255, "{edge:?}");
        assert_eq!(edge[1], 0);
        assert!(at(&d, 30, 15, 15)[3] >= 250);
    }

    #[test]
    fn an_offset_is_in_document_units() {
        let f = SvgFilter::new(r#"<filter x="-1" y="-1" width="3" height="3"><feOffset dx="5" dy="0"/></filter>"#).unwrap();
        // Two pixels per point: the square moves 10 pixels.
        let (w, h) = (60, 60);
        let bounds = Rect::new(0.0, 0.0, 30.0, 30.0);
        let sp = PixelSpace { to_doc: Affine::scale(0.5), px: 0.5, center: Point::new(15.0, 15.0), bounds };
        let mut d = square(w, h);
        f.apply(&mut d, w, h, &sp);
        assert_eq!(at(&d, w, 20, 30)[3], 0);
        assert_eq!(at(&d, w, 31, 30), [255, 0, 0, 255]);
        assert_eq!(at(&d, w, 49, 30), [255, 0, 0, 255]);
    }

    #[test]
    fn the_region_crops_what_the_filter_paints() {
        // The default region is the bounds plus 10 % each side; a flood fills exactly that.
        let f = SvgFilter::new(r#"<filter><feFlood flood-color="blue"/></filter>"#).unwrap();
        let (w, h) = (40, 40);
        let mut d = vec![0u8; w * h * 4];
        let bounds = Rect::new(10.0, 10.0, 30.0, 30.0);
        let sp = PixelSpace { to_doc: Affine::IDENTITY, px: 1.0, center: bounds.center(), bounds };
        f.apply(&mut d, w, h, &sp);
        assert_eq!(at(&d, w, 9, 20), [0, 0, 255, 255]);
        assert_eq!(at(&d, w, 7, 20)[3], 0);
        assert_eq!(f.outset(bounds), 2.0);
    }

    #[test]
    fn user_space_regions_and_their_outset() {
        let f = SvgFilter::new(r#"<filter filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="50"><feFlood/></filter>"#).unwrap();
        assert_eq!(f.outset(Rect::new(10.0, 10.0, 20.0, 20.0)), 80.0);
        let huge = SvgFilter::new(r#"<filter filterUnits="userSpaceOnUse" x="-1e9" width="2e9"><feFlood/></filter>"#).unwrap();
        assert_eq!(huge.outset(Rect::new(0.0, 0.0, 10.0, 10.0)), MAX_OUTSET);
    }

    #[test]
    fn what_isnt_a_filter_is_refused_and_bad_filters_leave_the_pixels() {
        for bad in ["", "<g/>", "<filter>", "not xml", "<svg><filter/></svg>"] {
            assert!(SvgFilter::new(bad).is_none(), "{bad}");
        }
        assert!(SvgFilter::new(&format!("<filter>{}</filter>", " ".repeat(MAX_XML))).is_none());
        for odd in ["<filter/>", "<filter><feNothing/></filter>", r#"<filter><feGaussianBlur stdDeviation="-5"/></filter>"#] {
            let f = SvgFilter::new(odd).unwrap();
            let mut d = square(12, 12);
            f.apply(&mut d, 12, 12, &space(12, 12));
            f.apply(&mut d, 13, 12, &space(12, 12));
        }
    }

    #[test]
    fn feimage_reads_no_files() {
        let f = SvgFilter::new(r#"<filter xmlns:xlink="http://www.w3.org/1999/xlink"><feImage xlink:href="/etc/passwd"/></filter>"#).unwrap();
        let mut d = square(12, 12);
        f.apply(&mut d, 12, 12, &space(12, 12));
        assert!(d.iter().all(|&v| v == 0), "an unloadable feImage paints nothing");
    }

    #[test]
    fn filters_are_found_in_an_svg_file() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><defs>
            <filter id="glow"><feGaussianBlur stdDeviation="2"/></filter>
            <filter><feImage xlink:href="a.png"/></filter></defs><rect/></svg>"#;
        let found = filters_in_svg(svg, 10);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "glow");
        assert_eq!(found[1].0, "Filter 2");
        assert!(found[1].1.contains("xmlns:xlink"));
        assert!(SvgFilter::new(&found[1].1).is_some());
        assert_eq!(filters_in_svg(svg, 1).len(), 1);
        assert!(filters_in_svg("<svg", 10).is_empty());
    }

    #[test]
    fn resolved_filters_keep_their_primitives_and_options() {
        let f = SvgFilter::new(
            r#"<filter id="x" color-interpolation-filters="sRGB" primitiveUnits="userSpaceOnUse"><feOffset dx="1"/><feMerge/></filter>"#,
        )
        .unwrap();
        let s = f.resolved("f1", Rect::new(0.0, 0.0, 10.0, 20.0));
        assert!(s.starts_with(r#"<filter id="f1" filterUnits="userSpaceOnUse" x="-1" y="-2" width="12" height="24""#), "{s}");
        assert!(s.contains(r#"color-interpolation-filters="sRGB""#));
        assert!(s.ends_with(r#"<feOffset dx="1"/><feMerge/></filter>"#), "{s}");
    }
}
