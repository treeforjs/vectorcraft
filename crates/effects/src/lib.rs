//! VectorCraft live effects (Illustrator's Effect menu).
//!
//! Effects live in appearance stacks as [`Effect`] `{id, params, visible}` records. This crate
//! interprets them:
//!
//! - **Geometry effects** (Distort & Transform, Path, Convert to Shape, Round Corners, Scribble,
//!   Warp) rewrite a path: [`apply_geometry`] evaluates them in stack order.
//! - **Raster effects** (Drop Shadow, Inner/Outer Glow, Feather, Gaussian Blur, and the
//!   Photoshop-style filters of [`pixel`]: Radial Blur, Smart Blur, Brush Strokes, Distort, Pixelate, Unsharp Mask, Glowing Edges, Sketch, Texture and Video) are described by
//!   [`raster_effects`] and painted by the renderer; [`outset`] says how far they reach beyond
//!   the geometry.
//! - **Stroke geometry** ([`stroke`]): arrowheads, dash patterns and width profiles, shared by the
//!   renderer, the exporters and Outline Stroke.
//! - [`effect_catalog`] lists every effect with its menu path, parameter documentation and the
//!   defaults of Illustrator's dialogs. Missing parameters always fall back to those defaults.
//! - [`reshape`] applies geometry effects to type, images, symbol instances and live objects
//!   through their outlines.
//! - [`evaluate_container`] turns a group's or layer's own fills, strokes and geometry effects into
//!   art painting its members (its raster effects then apply to the composite).
//! - [`clip_outline`] is the region a clip group clips to, shared by the renderer and the SVG and
//!   PDF writers.
//! - **Plug-in effects** (`plugin.<plug-in id>`, Effect › Plug-ins) are geometry effects run by an
//!   installed WebAssembly plug-in (`vectorcraft-plugins`); [`effect_info`] describes them like
//!   built-in ones, and an effect whose plug-in isn't installed leaves the geometry as it is.
//! - **Colour adjustments** (Brightness/Contrast, Curves, Levels, Hue/Saturation, Shift to Color,
//!   Temperature/Tint) recolour what an object paints with: [`adjust`] evaluates them for the
//!   renderer and the exporters.
//!
//! Everything is deterministic: "random" effects (Roughen, Tweak, Scribble) use a seeded hash
//! noise (`seed` parameter, default 0).
#![forbid(unsafe_code)]

mod adjust;
mod bake;
mod clip;
mod distort;
mod group;
mod live;
mod marks;
pub mod pixel;
mod raster;
mod reshape;
pub mod stroke;
mod stylize;
pub mod svgfilter;
mod three_d;
mod util;
mod warp;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_scale;

use serde_json::{Map, Value, json};
use vectorcraft_doc::{AppearanceItem, Effect, Node, NodeKind, StrokeLayer};
use vectorcraft_geom::{BezPath, FillRule, PathData, Rect};

pub use adjust::{ADJUSTMENTS, ColorMap, ImageHook, adjust, adjust_in_document, color_map, curve_at, curve_points, has_adjustment, is_adjustment};
pub use bake::{StrokeArt, bake_appearance, bake_document, expand_art, expand_leaf, fresh_ids, needs_bake};
pub use clip::clip_outline;
pub use group::{
    OutlineHook, PATHFINDER_EFFECTS, evaluate_container, has_container_appearance, has_pathfinder, is_pathfinder, member_shapes, paints,
    pathfinder_children,
};
pub use live::{expand_live, expand_live_deep, expanded_live_group, text_outliner};
pub use marks::{CROP_MARKS, crop_marks_art, has_crop_marks};
pub use pixel::{PIXEL_EFFECTS, PixelFx, PixelSpace};
pub use raster::{RasterFx, outset, raster_effects};
pub use reshape::{expand_outlined, needs_outline, outline_art, outline_text, reshape};
pub use three_d::{EXTRUDE, REVOLVE, ROTATE, has_three_d, is_three_d, three_d_art};
pub use warp::{WarpStyle, warp_point};

/// Catalogue entry for one effect.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectInfo {
    /// Stable id, e.g. `distort.roughen`.
    pub id: &'static str,
    /// Menu label (with the ellipsis when the effect has a dialog).
    pub label: &'static str,
    /// Effect-menu path, e.g. `["Effect", "Distort & Transform"]`.
    pub menu: &'static [&'static str],
    /// Human/agent-readable parameter documentation.
    pub params: &'static str,
    /// Dialog defaults.
    pub defaults: Value,
    /// Raster (painted) rather than geometry effect.
    pub raster: bool,
    /// The parameters that are distances in points (Scale Strokes & Effects scales them).
    pub lengths: Lengths,
}

/// Which of an effect's parameters are distances in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lengths {
    /// Always distances.
    pub always: &'static [&'static str],
    /// Distances while the effect's `relative` parameter is false (percentages of the object's
    /// size otherwise).
    pub absolute: &'static [&'static str],
}

impl Lengths {
    /// The distance parameters while the effect's `relative` parameter is `relative`.
    pub fn keys(&self, relative: bool) -> impl Iterator<Item = &'static str> + '_ {
        self.always.iter().chain(if relative { &[][..] } else { self.absolute }).copied()
    }
}

/// Is parameter `key` of effect `id` a distance in points while its `relative` parameter is
/// `relative`?
pub fn is_length(id: &str, key: &str, relative: bool) -> bool {
    catalog_index().get(id).is_some_and(|info| info.lengths.keys(relative).any(|k| k == key))
}

/// The distance parameters of effect `id` (none for unknown effects).
fn lengths_of(id: &str) -> Lengths {
    let always = |always| Lengths { always, absolute: &[] };
    match id {
        "convertToShape.rectangle" | "convertToShape.ellipse" => always(&["extraW", "extraH", "width", "height"]),
        "convertToShape.roundedRectangle" => always(&["extraW", "extraH", "width", "height", "radius"]),
        "distort.roughen" | "distort.zigZag" => Lengths { always: &[], absolute: &["size"] },
        "distort.tweak" => Lengths { always: &[], absolute: &["h", "v"] },
        "distort.transform" => always(&["moveH", "moveV"]),
        "path.offsetPath" => always(&["offset"]),
        EXTRUDE => always(&["depth"]),
        REVOLVE => always(&["offset"]),
        "path.outlineStroke" => always(&["width"]),
        "stylize.roundCorners" | "stylize.feather" | "blur.gaussian" | "blur.smart" | "sharpen.unsharpMask" => always(&["radius"]),
        "stylize.scribble" => always(&["overlap", "strokeWidth", "spacing", "variation"]),
        "stylize.dropShadow" => always(&["x", "y", "blur"]),
        "stylize.innerGlow" | "stylize.outerGlow" => always(&["blur"]),
        "stylize.glowingEdges" => always(&["edgeWidth", "smoothness"]),
        "pixelate.colorHalftone" => always(&["maxRadius"]),
        "distort.glass" => always(&["distortion", "smoothness"]),
        "distort.oceanRipple" => always(&["rippleSize", "rippleMagnitude"]),
        "pixelate.crystallize" | "pixelate.pointillize" | "texture.stainedGlass" => always(&["cellSize"]),
        "texture.craquelure" => always(&["crackSpacing"]),
        "brushStrokes.angledStrokes" | "brushStrokes.crosshatch" | "brushStrokes.inkOutlines" | "brushStrokes.sprayedStrokes" => {
            always(&["strokeLength"])
        }
        "texture.mosaicTiles" => always(&["tileSize"]),
        "rasterize" => always(&["addAround"]),
        _ => Lengths::default(),
    }
}

const WARP_DOC: &str =
    "{bend: % (-100..100, 50), horizontal: % distortion (0), vertical: % distortion (0), orientation: \"horizontal\"|\"vertical\"}";
const DT: &[&str] = &["Effect", "Distort & Transform"];
const PATH: &[&str] = &["Effect", "Path"];
const SHAPE: &[&str] = &["Effect", "Convert to Shape"];
const STYLIZE: &[&str] = &["Effect", "Stylize"];
const WARP: &[&str] = &["Effect", "Warp"];
const BLUR: &[&str] = &["Effect", "Blur"];
const SHARPEN: &[&str] = &["Effect", "Sharpen"];
const PIXELATE: &[&str] = &["Effect", "Pixelate"];
const DISTORT: &[&str] = &["Effect", "Distort"];
const VIDEO: &[&str] = &["Effect", "Video"];
const TEXTURE: &[&str] = &["Effect", "Texture"];
const BRUSH_STROKES: &[&str] = &["Effect", "Brush Strokes"];
const SKETCH: &[&str] = &["Effect", "Sketch"];
const ARTISTIC: &[&str] = &["Effect", "Artistic"];
const PATHFINDER: &[&str] = &["Effect", "Pathfinder"];
const PLUGINS: &[&str] = &["Effect", "Plug-ins"];
const ADJUST: &[&str] = &["Effect", "Color Adjustments"];
const THREE_D: &[&str] = &["Effect", "3D and Materials"];
const SVG_FILTERS: &[&str] = &["Effect", "SVG Filters"];

/// The warp styles in Illustrator's Style menu order: (id suffix, label).
pub const WARP_STYLES: [(&str, &str); 15] = [
    ("arc", "Arc…"),
    ("arcLower", "Arc Lower…"),
    ("arcUpper", "Arc Upper…"),
    ("arch", "Arch…"),
    ("bulge", "Bulge…"),
    ("shellLower", "Shell Lower…"),
    ("shellUpper", "Shell Upper…"),
    ("flag", "Flag…"),
    ("wave", "Wave…"),
    ("fish", "Fish…"),
    ("rise", "Rise…"),
    ("fisheye", "Fisheye…"),
    ("inflate", "Inflate…"),
    ("squeeze", "Squeeze…"),
    ("twist", "Twist…"),
];

/// Every supported effect with its dialog defaults.
pub fn effect_catalog() -> Vec<EffectInfo> {
    let g = |id, label, menu, params, defaults| EffectInfo { id, label, menu, params, defaults, raster: false, lengths: lengths_of(id) };
    let r = |id, label, menu, params, defaults| EffectInfo { id, label, menu, params, defaults, raster: true, lengths: lengths_of(id) };
    let mut v = vec![
        g(
            "convertToShape.rectangle",
            "Rectangle…",
            SHAPE,
            "{relative: bool (true), extraW: pt (18), extraH: pt (18), width: pt (absolute, 100), height: pt (absolute, 100)}",
            json!({"relative": true, "extraW": 18.0, "extraH": 18.0, "width": 100.0, "height": 100.0}),
        ),
        g(
            "convertToShape.roundedRectangle",
            "Rounded Rectangle…",
            SHAPE,
            "{relative: bool (true), extraW: pt (18), extraH: pt (18), width, height (absolute), radius: pt (9)}",
            json!({"relative": true, "extraW": 18.0, "extraH": 18.0, "width": 100.0, "height": 100.0, "radius": 9.0}),
        ),
        g(
            "convertToShape.ellipse",
            "Ellipse…",
            SHAPE,
            "{relative: bool (true), extraW: pt (18), extraH: pt (18), width, height (absolute)}",
            json!({"relative": true, "extraW": 18.0, "extraH": 18.0, "width": 100.0, "height": 100.0}),
        ),
        g(
            "distort.freeDistort",
            "Free Distort…",
            DT,
            "{corners: [[x,y]×4] new positions of the bounding-box corners TL, TR, BR, BL in unit box coordinates (identity [[0,0],[1,0],[1,1],[0,1]])}",
            json!({"corners": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]}),
        ),
        g("distort.puckerBloat", "Pucker & Bloat…", DT, "{amount: % (-200 pucker … 200 bloat, default 0)}", json!({"amount": 0.0})),
        g(
            "distort.roughen",
            "Roughen…",
            DT,
            "{size: 0..100 (5), relative: bool (true: % of size; false: pt), detail: per inch 0..100 (10), points: \"smooth\"|\"corner\", seed: int (0)}",
            json!({"size": 5.0, "relative": true, "detail": 10.0, "points": "smooth", "seed": 0}),
        ),
        g(
            "distort.transform",
            "Transform…",
            DT,
            "{scaleH: % (100), scaleV: % (100), moveH: pt (0), moveV: pt (0), rotate: deg (0), copies: int (0), reflectX: bool, reflectY: bool}",
            json!({"scaleH": 100.0, "scaleV": 100.0, "moveH": 0.0, "moveV": 0.0, "rotate": 0.0, "copies": 0, "reflectX": false, "reflectY": false}),
        ),
        g(
            "distort.tweak",
            "Tweak…",
            DT,
            "{h: amount (10), v: amount (10), relative: bool (true: % of size), anchors: bool (true), in: bool (true), out: bool (true), seed: int (0)}",
            json!({"h": 10.0, "v": 10.0, "relative": true, "anchors": true, "in": true, "out": true, "seed": 0}),
        ),
        g("distort.twist", "Twist…", DT, "{angle: deg (-3600..3600, 10)}", json!({"angle": 10.0})),
        g(
            "distort.zigZag",
            "Zig Zag…",
            DT,
            "{size: (10), relative: bool (false: pt; true: % of size), ridges: per segment 0..100 (4), points: \"smooth\"|\"corner\"}",
            json!({"size": 10.0, "relative": false, "ridges": 4, "points": "smooth"}),
        ),
        g(
            "path.offsetPath",
            "Offset Path…",
            PATH,
            "{offset: pt (10; negative insets), joins: \"miter\"|\"round\"|\"bevel\", miterLimit: (4)}",
            json!({"offset": 10.0, "joins": "miter", "miterLimit": 4.0}),
        ),
        g(
            "path.outlineObject",
            "Outline Object",
            PATH,
            "{} the object as its outlines (type as glyph shapes, live objects and symbols as their art) for the effects after it, Expand Appearance and the exporters; paths stay as they are",
            json!({}),
        ),
        g(
            "path.outlineStroke",
            "Outline Stroke",
            PATH,
            "{width?: pt (defaults to the stroke's weight)} the outline of the stroke the effect is on (on a fill or the object: the top stroke), with its caps, joins, dashes, alignment, width profile and arrowheads",
            json!({}),
        ),
        g("stylize.roundCorners", "Round Corners…", STYLIZE, "{radius: pt (10)}", json!({"radius": 10.0})),
        g(
            "stylize.scribble",
            "Scribble…",
            STYLIZE,
            "{angle: deg (30), overlap: pt (0), strokeWidth: pt (3), curviness: % (5), spacing: pt (5), variation: pt (0.5), seed: int (0)} (simplified: a single hatching scribble outlined to a filled shape)",
            json!({"angle": 30.0, "overlap": 0.0, "strokeWidth": 3.0, "curviness": 5.0, "spacing": 5.0, "variation": 0.5, "seed": 0}),
        ),
        r(
            "stylize.dropShadow",
            "Drop Shadow…",
            STYLIZE,
            "{mode: blend mode (\"multiply\"), opacity: % (75), x: pt (7), y: pt (7), blur: pt (5), color: \"#rrggbb\" (\"#000000\")}",
            json!({"mode": "multiply", "opacity": 75.0, "x": 7.0, "y": 7.0, "blur": 5.0, "color": "#000000"}),
        ),
        r(
            "stylize.innerGlow",
            "Inner Glow…",
            STYLIZE,
            "{mode: blend mode (\"screen\"), opacity: % (75), blur: pt (5), color: \"#rrggbb\" (\"#ffffff\"), source: \"edge\"|\"center\"}",
            json!({"mode": "screen", "opacity": 75.0, "blur": 5.0, "color": "#ffffff", "source": "edge"}),
        ),
        r(
            "stylize.outerGlow",
            "Outer Glow…",
            STYLIZE,
            "{mode: blend mode (\"screen\"), opacity: % (75), blur: pt (5), color: \"#rrggbb\" (\"#ffff00\")}",
            json!({"mode": "screen", "opacity": 75.0, "blur": 5.0, "color": "#ffff00"}),
        ),
        r("stylize.feather", "Feather…", STYLIZE, "{radius: pt (5)}", json!({"radius": 5.0})),
        r("blur.gaussian", "Gaussian Blur…", BLUR, "{radius: pt (5)}", json!({"radius": 5.0})),
        r(
            "blur.radial",
            "Radial Blur…",
            BLUR,
            "{amount: 1..100 (10; spin: an arc of `amount`°, zoom: the content scaled by up to about ±amount/2 %), method: \"spin\"|\"zoom\" (\"spin\"), quality: \"draft\"|\"good\"|\"best\" (\"good\"; 16, 64 or 256 samples)} blurs around the object's centre",
            json!({"amount": 10.0, "method": "spin", "quality": "good"}),
        ),
        r(
            "blur.smart",
            "Smart Blur…",
            BLUR,
            "{radius: pt 0.1..100 (3), threshold: levels 0.1..100 (25; only colours closer than this blur together, so edges stay sharp), quality: \"low\"|\"medium\"|\"high\" (\"medium\"; 5, 7 or 9 samples across), mode: \"normal\"|\"edgeOnly\"|\"overlayEdge\" (\"normal\"; the edge modes draw the steps past the threshold in white, on black or over the blurred image)}",
            json!({"radius": 3.0, "threshold": 25.0, "quality": "medium", "mode": "normal"}),
        ),
        r(
            "sharpen.unsharpMask",
            "Unsharp Mask…",
            SHARPEN,
            "{amount: % 1..500 (50), radius: pt 0.1..250 (1; σ of the blur edges are found against), threshold: levels 0..255 (0; smaller differences are left alone)}",
            json!({"amount": 50.0, "radius": 1.0, "threshold": 0.0}),
        ),
        r(
            "stylize.glowingEdges",
            "Glowing Edges…",
            STYLIZE,
            "{edgeWidth: pt 1..14 (2), edgeBrightness: 0..20 (6), smoothness: pt 1..15 (5)} finds alpha-weighted Sobel edges and draws bright coloured outlines on black",
            json!({"edgeWidth": 2.0, "edgeBrightness": 6.0, "smoothness": 5.0}),
        ),
        r(
            "pixelate.colorHalftone",
            "Color Halftone…",
            PIXELATE,
            "{maxRadius: pt 4..127 (8; the largest dot), angle1: ° (108; red's screen), angle2: ° (162; green's), angle3: ° (90; blue's)} turns each colour channel into dots on its own screen, larger where the channel is brighter",
            json!({"maxRadius": 8.0, "angle1": 108.0, "angle2": 162.0, "angle3": 90.0}),
        ),
        r(
            "pixelate.crystallize",
            "Crystallize…",
            PIXELATE,
            "{cellSize: pt 3..300 (10)} clumps colours into polygons, each the average colour of its cell",
            json!({"cellSize": 10.0}),
        ),
        r(
            "pixelate.mezzotint",
            "Mezzotint…",
            PIXELATE,
            "{type: \"fineDots\"|\"mediumDots\"|\"grainyDots\"|\"coarseDots\"|\"shortLines\"|\"mediumLines\"|\"longLines\"|\"shortStrokes\"|\"mediumStrokes\"|\"longStrokes\" (\"fineDots\")} turns every colour channel full or empty against random dots, lines or strokes",
            json!({"type": "fineDots"}),
        ),
        r(
            "rasterize",
            "Rasterize…",
            &["Effect"],
            "{resolution: \"document\"|\"screen\" (72 ppi)|\"medium\" (150)|\"high\" (300)|\"other\" (\"document\"), ppi: 1..2400 (300; with resolution other), colorModel: \"document\"|\"grayscale\"|\"bitmap\" (\"document\"), background: \"white\"|\"transparent\" (\"white\"), antiAlias: bool (true), clippingMask: bool (false; the white only under the art), addAround: pt 0..1000 (0; white room around the object)} shows the object as an image of that resolution and colour model while it stays editable art",
            json!({"resolution": "document", "ppi": 300.0, "colorModel": "document", "background": "white", "antiAlias": true, "clippingMask": false, "addAround": 0.0}),
        ),
        r(
            "pixelate.pointillize",
            "Pointillize…",
            PIXELATE,
            "{cellSize: pt 3..300 (5)} breaks the colours into randomly placed dots on a white background",
            json!({"cellSize": 5.0}),
        ),
        r(
            "distort.diffuseGlow",
            "Diffuse Glow…",
            DISTORT,
            "{graininess: 0..10 (6; white specks), glowAmount: 0..20 (10), clearAmount: 0..20 (15; the higher, the fewer tones glow)} lays a soft white glow over the highlights",
            json!({"graininess": 6.0, "glowAmount": 10.0, "clearAmount": 15.0}),
        ),
        r(
            "distort.glass",
            "Glass…",
            DISTORT,
            "{distortion: 0..20 (5; the content moves up to half this in points), smoothness: 1..15 (3), texture: \"frosted\"|\"blocks\"|\"canvas\"|\"tinyLens\" (\"frosted\"), scaling: % 50..200 (100), invert: bool (false)} shows the content through a textured pane of glass",
            json!({"distortion": 5.0, "smoothness": 3.0, "texture": "frosted", "scaling": 100.0, "invert": false}),
        ),
        r(
            "distort.oceanRipple",
            "Ocean Ripple…",
            DISTORT,
            "{rippleSize: 1..15 (9; ripples about twice this in points), rippleMagnitude: 0..20 (9; the content moves up to half this in points)} wobbles the content as if under water",
            json!({"rippleSize": 9.0, "rippleMagnitude": 9.0}),
        ),
        r(
            "brushStrokes.accentedEdges",
            "Accented Edges…",
            BRUSH_STROKES,
            "{edgeWidth: 1..14 (2; edges about half this in points), edgeBrightness: 0..50 (38; above 25 lightens the edges, below darkens them), smoothness: 1..15 (5)} accents the art's edges like chalk or ink",
            json!({"edgeWidth": 2.0, "edgeBrightness": 38.0, "smoothness": 5.0}),
        ),
        r(
            "brushStrokes.angledStrokes",
            "Angled Strokes…",
            BRUSH_STROKES,
            "{directionBalance: 0..100 (50; the tone splitting the two diagonals), strokeLength: pt 3..50 (15), sharpness: 0..10 (3)} repaints the light tones in strokes along one diagonal and the dark tones along the other",
            json!({"directionBalance": 50.0, "strokeLength": 15.0, "sharpness": 3.0}),
        ),
        r(
            "brushStrokes.crosshatch",
            "Crosshatch…",
            BRUSH_STROKES,
            "{strokeLength: pt 3..50 (9), sharpness: 0..20 (6), strength: 1..3 (1; passes)} repaints the art in crossing diagonal pencil hatching",
            json!({"strokeLength": 9.0, "sharpness": 6.0, "strength": 1.0}),
        ),
        r(
            "brushStrokes.darkStrokes",
            "Dark Strokes…",
            BRUSH_STROKES,
            "{balance: 0..10 (5; the tone splitting dark from light), blackIntensity: 0..10 (6), whiteIntensity: 0..10 (2)} paints the dark tones in short dark strokes and the light tones in long light ones",
            json!({"balance": 5.0, "blackIntensity": 6.0, "whiteIntensity": 2.0}),
        ),
        r(
            "brushStrokes.inkOutlines",
            "Ink Outlines…",
            BRUSH_STROKES,
            "{strokeLength: pt 1..50 (4), darkIntensity: 0..50 (20), lightIntensity: 0..50 (10)} redraws the art in fine strokes with inked outlines",
            json!({"strokeLength": 4.0, "darkIntensity": 20.0, "lightIntensity": 10.0}),
        ),
        r(
            "brushStrokes.spatter",
            "Spatter…",
            BRUSH_STROKES,
            "{sprayRadius: 0..25 (10; the art scatters by up to half this in points), smoothness: 1..15 (5)} sprays the art as if through an airbrush",
            json!({"sprayRadius": 10.0, "smoothness": 5.0}),
        ),
        r(
            "brushStrokes.sprayedStrokes",
            "Sprayed Strokes…",
            BRUSH_STROKES,
            "{strokeLength: pt 0..20 (12), sprayRadius: 0..25 (7), strokeDirection: \"rightDiagonal\"|\"horizontal\"|\"leftDiagonal\"|\"vertical\" (\"rightDiagonal\")} repaints the art in sprayed, angled strokes",
            json!({"strokeLength": 12.0, "sprayRadius": 7.0, "strokeDirection": "rightDiagonal"}),
        ),
        r(
            "brushStrokes.sumiE",
            "Sumi-e…",
            BRUSH_STROKES,
            "{strokeWidth: 3..15 (10; strokes half this in points wide), strokePressure: 0..15 (2), contrast: 0..40 (16)} repaints the art in wide black-ink strokes on rice paper",
            json!({"strokeWidth": 10.0, "strokePressure": 2.0, "contrast": 16.0}),
        ),
        r(
            "artistic.coloredPencil",
            "Colored Pencil…",
            ARTISTIC,
            "{pencilWidth: 1..24 (4), strokePressure: 0..15 (8), paperBrightness: 0..50 (25)} redraws the art in crosshatched coloured pencil on paper",
            json!({"pencilWidth": 4.0, "strokePressure": 8.0, "paperBrightness": 25.0}),
        ),
        r(
            "artistic.cutout",
            "Cutout…",
            ARTISTIC,
            "{numberOfLevels: 2..8 (4), edgeSimplicity: 0..10 (4), edgeFidelity: 1..3 (2)} cuts the art into flat shapes of a few colours, like coloured paper",
            json!({"numberOfLevels": 4.0, "edgeSimplicity": 4.0, "edgeFidelity": 2.0}),
        ),
        r(
            "artistic.dryBrush",
            "Dry Brush…",
            ARTISTIC,
            "{brushSize: 0..10 (2), brushDetail: 0..10 (8), texture: 1..3 (1)} paints the art with a dry brush in a few tones",
            json!({"brushSize": 2.0, "brushDetail": 8.0, "texture": 1.0}),
        ),
        r(
            "artistic.filmGrain",
            "Film Grain…",
            ARTISTIC,
            "{grain: 0..20 (4), highlightArea: 0..20 (0), intensity: 0..10 (10)} adds film grain to the shadows and midtones and brightens the highlights",
            json!({"grain": 4.0, "highlightArea": 0.0, "intensity": 10.0}),
        ),
        r(
            "artistic.fresco",
            "Fresco…",
            ARTISTIC,
            "{brushSize: 0..10 (2), brushDetail: 0..10 (8), texture: 1..3 (1)} paints the art in short, dark, contrasty dabs",
            json!({"brushSize": 2.0, "brushDetail": 8.0, "texture": 1.0}),
        ),
        r(
            "artistic.neonGlow",
            "Neon Glow…",
            ARTISTIC,
            "{glowSize: -24..24 (5; negative glows from the dark tones), glowBrightness: 0..50 (15), color: colour (\"#4080ff\")} lights the art with a coloured neon glow",
            json!({"glowSize": 5.0, "glowBrightness": 15.0, "color": "#4080ff"}),
        ),
        r(
            "artistic.paintDaubs",
            "Paint Daubs…",
            ARTISTIC,
            "{brushSize: 1..50 (8), sharpness: 0..40 (7), brushType: \"simple\"|\"lightRough\"|\"darkRough\"|\"wideSharp\"|\"wideBlurry\"|\"sparkle\" (\"simple\")} paints the art in daubs",
            json!({"brushSize": 8.0, "sharpness": 7.0, "brushType": "simple"}),
        ),
        r(
            "artistic.paletteKnife",
            "Palette Knife…",
            ARTISTIC,
            "{strokeSize: 1..50 (25), strokeDetail: 1..3 (3), softness: 0..10 (0)} lays the colours on in flat knife strokes",
            json!({"strokeSize": 25.0, "strokeDetail": 3.0, "softness": 0.0}),
        ),
        r(
            "artistic.plasticWrap",
            "Plastic Wrap…",
            ARTISTIC,
            "{highlightStrength: 0..20 (15), detail: 1..15 (9), smoothness: 1..15 (7)} wraps the art in shiny plastic film",
            json!({"highlightStrength": 15.0, "detail": 9.0, "smoothness": 7.0}),
        ),
        r(
            "artistic.posterEdges",
            "Poster Edges…",
            ARTISTIC,
            "{edgeThickness: 0..10 (2), edgeIntensity: 0..10 (1), posterization: 0..6 (2)} posterizes the art and outlines its edges in black",
            json!({"edgeThickness": 2.0, "edgeIntensity": 1.0, "posterization": 2.0}),
        ),
        r(
            "artistic.roughPastels",
            "Rough Pastels…",
            ARTISTIC,
            "{strokeLength: 0..40 (6), strokeDetail: 1..20 (4), texture: \"brick\"|\"burlap\"|\"canvas\"|\"sandstone\" (\"canvas\"), scaling: % 50..200 (100), relief: 0..50 (20), light: \"bottom\"|\"bottomLeft\"|\"left\"|\"topLeft\"|\"top\"|\"topRight\"|\"right\"|\"bottomRight\" (\"bottom\"), invert: bool (false)} redraws the art in pastel chalk strokes on textured paper",
            json!({"strokeLength": 6.0, "strokeDetail": 4.0, "texture": "canvas", "scaling": 100.0, "relief": 20.0, "light": "bottom", "invert": false}),
        ),
        r(
            "artistic.smudgeStick",
            "Smudge Stick…",
            ARTISTIC,
            "{strokeLength: 0..10 (2), highlightArea: 0..20 (0), intensity: 0..10 (10)} smudges the art in short diagonal strokes and brightens the highlights",
            json!({"strokeLength": 2.0, "highlightArea": 0.0, "intensity": 10.0}),
        ),
        r(
            "artistic.sponge",
            "Sponge…",
            ARTISTIC,
            "{brushSize: 0..10 (2), definition: 0..25 (12), smoothness: 1..15 (5)} dabs the art on with a sponge",
            json!({"brushSize": 2.0, "definition": 12.0, "smoothness": 5.0}),
        ),
        r(
            "artistic.underpainting",
            "Underpainting…",
            ARTISTIC,
            "{brushSize: 0..40 (6), textureCoverage: 0..40 (16), texture: \"brick\"|\"burlap\"|\"canvas\"|\"sandstone\" (\"canvas\"), scaling: % 50..200 (100), relief: 0..50 (4), light: \"bottom\"|\"bottomLeft\"|\"left\"|\"topLeft\"|\"top\"|\"topRight\"|\"right\"|\"bottomRight\" (\"top\"), invert: bool (false)} paints a soft underpainting of the art on a textured surface",
            json!({"brushSize": 6.0, "textureCoverage": 16.0, "texture": "canvas", "scaling": 100.0, "relief": 4.0, "light": "top", "invert": false}),
        ),
        r(
            "artistic.watercolor",
            "Watercolor…",
            ARTISTIC,
            "{brushDetail: 1..14 (9), shadowIntensity: 0..10 (1), texture: 1..3 (1)} paints the art in watercolour, the pigment pooling at the edges",
            json!({"brushDetail": 9.0, "shadowIntensity": 1.0, "texture": 1.0}),
        ),
        r(
            "sketch.basRelief",
            "Bas Relief…",
            SKETCH,
            "{detail: 1..15 (13), smoothness: 1..15 (3), light: \"bottom\"|\"bottomLeft\"|\"left\"|\"topLeft\"|\"top\"|\"topRight\"|\"right\"|\"bottomRight\" (\"bottom\")} carves the art in low relief, lit from one side, in black and white",
            json!({"detail": 13.0, "smoothness": 3.0, "light": "bottom"}),
        ),
        r(
            "sketch.chalkCharcoal",
            "Chalk & Charcoal…",
            SKETCH,
            "{charcoalArea: 0..20 (6), chalkArea: 0..20 (6), strokePressure: 0..5 (1)} redraws the shadows in black charcoal and the highlights in white chalk on grey",
            json!({"charcoalArea": 6.0, "chalkArea": 6.0, "strokePressure": 1.0}),
        ),
        r(
            "sketch.charcoal",
            "Charcoal…",
            SKETCH,
            "{charcoalThickness: 1..7 (1), detail: 0..5 (5), lightDarkBalance: 0..100 (50)} redraws the art in smudged diagonal charcoal strokes",
            json!({"charcoalThickness": 1.0, "detail": 5.0, "lightDarkBalance": 50.0}),
        ),
        r(
            "sketch.chrome",
            "Chrome…",
            SKETCH,
            "{detail: 0..10 (4), smoothness: 0..10 (7)} turns the art into polished chrome",
            json!({"detail": 4.0, "smoothness": 7.0}),
        ),
        r(
            "sketch.conteCrayon",
            "Conté Crayon…",
            SKETCH,
            "{foregroundLevel: 1..15 (11), backgroundLevel: 1..15 (7), texture: \"brick\"|\"burlap\"|\"canvas\"|\"sandstone\" (\"canvas\"), scaling: % 50..200 (100), relief: 0..50 (4), light: \"bottom\"|\"bottomLeft\"|\"left\"|\"topLeft\"|\"top\"|\"topRight\"|\"right\"|\"bottomRight\" (\"top\"), invert: bool (false)} redraws the art in dense black and white crayon on textured paper",
            json!({"foregroundLevel": 11.0, "backgroundLevel": 7.0, "texture": "canvas", "scaling": 100.0, "relief": 4.0, "light": "top", "invert": false}),
        ),
        r(
            "sketch.graphicPen",
            "Graphic Pen…",
            SKETCH,
            "{strokeLength: pt 1..15 (15), lightDarkBalance: 0..100 (50), strokeDirection: \"rightDiagonal\"|\"horizontal\"|\"leftDiagonal\"|\"vertical\" (\"rightDiagonal\")} redraws the art in fine, straight ink strokes",
            json!({"strokeLength": 15.0, "lightDarkBalance": 50.0, "strokeDirection": "rightDiagonal"}),
        ),
        r(
            "sketch.halftonePattern",
            "Halftone Pattern…",
            SKETCH,
            "{size: 1..12 (1; cells 1.5 pt per step), contrast: 0..50 (5), patternType: \"circle\"|\"dot\"|\"line\" (\"dot\")} screens the art with a black and white halftone",
            json!({"size": 1.0, "contrast": 5.0, "patternType": "dot"}),
        ),
        r(
            "sketch.notePaper",
            "Note Paper…",
            SKETCH,
            "{imageBalance: 0..50 (25), graininess: 0..20 (10), relief: 0..25 (11)} embosses the art into grainy handmade paper",
            json!({"imageBalance": 25.0, "graininess": 10.0, "relief": 11.0}),
        ),
        r(
            "sketch.photocopy",
            "Photocopy…",
            SKETCH,
            "{detail: 1..24 (7), darkness: 1..50 (8)} copies the art's edges in black as a photocopier would",
            json!({"detail": 7.0, "darkness": 8.0}),
        ),
        r(
            "sketch.plaster",
            "Plaster…",
            SKETCH,
            "{imageBalance: 0..50 (20), smoothness: 1..15 (2), light: \"bottom\"|\"bottomLeft\"|\"left\"|\"topLeft\"|\"top\"|\"topRight\"|\"right\"|\"bottomRight\" (\"top\")} moulds the dark areas in raised plaster",
            json!({"imageBalance": 20.0, "smoothness": 2.0, "light": "top"}),
        ),
        r(
            "sketch.reticulation",
            "Reticulation…",
            SKETCH,
            "{density: 0..50 (12), foregroundLevel: 0..50 (40), backgroundLevel: 0..50 (5)} clumps the art into the grain of shrunken film emulsion",
            json!({"density": 12.0, "foregroundLevel": 40.0, "backgroundLevel": 5.0}),
        ),
        r(
            "sketch.stamp",
            "Stamp…",
            SKETCH,
            "{lightDarkBalance: 0..50 (25), smoothness: 1..50 (5)} prints the art as a black rubber stamp",
            json!({"lightDarkBalance": 25.0, "smoothness": 5.0}),
        ),
        r(
            "sketch.tornEdges",
            "Torn Edges…",
            SKETCH,
            "{imageBalance: 0..50 (25), smoothness: 1..15 (11), contrast: 1..25 (17)} redraws the art as black and white torn paper",
            json!({"imageBalance": 25.0, "smoothness": 11.0, "contrast": 17.0}),
        ),
        r(
            "sketch.waterPaper",
            "Water Paper…",
            SKETCH,
            "{fiberLength: 3..50 (15; fibres 0.3 pt per step), brightness: 0..100 (60), contrast: 0..100 (80)} bleeds the colours along the fibres of damp paper",
            json!({"fiberLength": 15.0, "brightness": 60.0, "contrast": 80.0}),
        ),
        r(
            "texture.craquelure",
            "Craquelure…",
            TEXTURE,
            "{crackSpacing: pt 2..100 (15), crackDepth: 0..10 (6), crackBrightness: 0..10 (9)} paints the art on plaster cracked into raised plates",
            json!({"crackSpacing": 15.0, "crackDepth": 6.0, "crackBrightness": 9.0}),
        ),
        r(
            "texture.grain",
            "Grain…",
            TEXTURE,
            "{intensity: 0..100 (40), contrast: 0..100 (50; 50 keeps it), grainType: \"regular\"|\"soft\"|\"sprinkles\"|\"clumped\"|\"contrasty\"|\"enlarged\"|\"stippled\"|\"horizontal\"|\"vertical\"|\"speckle\" (\"regular\")} adds film grain, specks or streaks",
            json!({"intensity": 40.0, "contrast": 50.0, "grainType": "regular"}),
        ),
        r(
            "texture.mosaicTiles",
            "Mosaic Tiles…",
            TEXTURE,
            "{tileSize: pt 2..100 (12), groutWidth: 1..15 (3; the grout is 0.3 pt per step), lightenGrout: 0..10 (9)} cuts the art into raised square tiles set in grout",
            json!({"tileSize": 12.0, "groutWidth": 3.0, "lightenGrout": 9.0}),
        ),
        r(
            "texture.patchwork",
            "Patchwork…",
            TEXTURE,
            "{squareSize: 0..10 (4; squares 2 pt + this across), relief: 0..25 (8)} turns the art into raised squares, each its average colour",
            json!({"squareSize": 4.0, "relief": 8.0}),
        ),
        r(
            "texture.stainedGlass",
            "Stained Glass…",
            TEXTURE,
            "{cellSize: pt 2..50 (10), borderThickness: 1..20 (4; the lead is 0.3 pt per step), lightIntensity: 0..10 (3)} remakes the art as leaded panes of glass, each its average colour, lit from the middle",
            json!({"cellSize": 10.0, "borderThickness": 4.0, "lightIntensity": 3.0}),
        ),
        r(
            "texture.texturizer",
            "Texturizer…",
            TEXTURE,
            "{texture: \"brick\"|\"burlap\"|\"canvas\"|\"sandstone\" (\"canvas\"), scaling: % 50..200 (100), relief: 0..50 (4), light: \"bottom\"|\"bottomLeft\"|\"left\"|\"topLeft\"|\"top\"|\"topRight\"|\"right\"|\"bottomRight\" (\"top\"), invert: bool (false)} prints the art on a surface lit from one side",
            json!({"texture": "canvas", "scaling": 100.0, "relief": 4.0, "light": "top", "invert": false}),
        ),
        r(
            "video.deInterlace",
            "De-Interlace…",
            VIDEO,
            "{eliminate: \"odd\"|\"even\" (\"odd\"; which scan lines go, each 1 pt tall), create: \"duplication\"|\"interpolation\" (\"interpolation\"; how they are rebuilt)} smooths away the comb of interlaced video stills",
            json!({"eliminate": "odd", "create": "interpolation"}),
        ),
        r(
            "video.ntscColors",
            "NTSC Colors",
            VIDEO,
            "{} desaturates colours too strong for a broadcast television signal, keeping their hue and brightness",
            json!({}),
        ),
        r(
            SVG_FILTER,
            "Apply SVG Filter…",
            SVG_FILTERS,
            "{filter: \"<filter>…</filter>\" (an SVG filter element; lengths in points, a bounding-box region of the object's painted bounds; at most 256 KB), name: string (what the Appearance panel calls it)} runs the SVG filter primitives over the object's pixels",
            json!({"name": svgfilter::PRESETS[0].0, "filter": svgfilter::PRESETS[0].1}),
        ),
    ];
    v.extend([
        g(
            "adjust.brightnessContrast",
            "Brightness/Contrast…",
            ADJUST,
            "{brightness: -100..100 (0; bends the tones, black and white stay), contrast: -100..100 (0)} recolours the object's fills, strokes, type, meshes and embedded images (each colour keeps its model)",
            json!({"brightness": 0.0, "contrast": 0.0}),
        ),
        g(
            "adjust.curves",
            "Curves…",
            ADJUST,
            "{points: \"x,y x,y …\" or [[x, y], …] (input, output 0..255; a smooth monotone curve through them, flat past the ends; \"0,0 128,128 255,255\"), channel: \"rgb\"|\"red\"|\"green\"|\"blue\" (\"rgb\")} recolours as Brightness/Contrast does",
            json!({"points": "0,0 128,128 255,255", "channel": "rgb"}),
        ),
        g(
            "adjust.hueSaturation",
            "Hue/Saturation…",
            ADJUST,
            "{hue: deg -180..180 (0), saturation: -100..100 (0), lightness: -100..100 (0), colorize: bool (false; true: every colour takes hue `hue` (0..360) at saturation (100 + saturation) / 2 %)} recolours as Brightness/Contrast does",
            json!({"hue": 0.0, "saturation": 0.0, "lightness": 0.0, "colorize": false}),
        ),
        g(
            "adjust.levels",
            "Levels…",
            ADJUST,
            "{inputBlack: 0..255 (0), inputWhite: 0..255 (255), gamma: 0.1..10 (1; above 1 lightens the midtones), outputBlack: 0..255 (0), outputWhite: 0..255 (255), channel: \"rgb\"|\"red\"|\"green\"|\"blue\" (\"rgb\")} recolours as Brightness/Contrast does",
            json!({"inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.0, "outputBlack": 0.0, "outputWhite": 255.0, "channel": "rgb"}),
        ),
        g(
            "adjust.shiftToColor",
            "Shift to Color…",
            ADJUST,
            "{color: \"#rrggbb\" (\"#ff8000\"), amount: 0..100 % (50), preserveLightness: bool (true: colours take the target's hue and saturation and keep their lightness; false: they mix with it)} recolours as Brightness/Contrast does",
            json!({"color": "#ff8000", "amount": 50.0, "preserveLightness": true}),
        ),
        g(
            "adjust.temperatureTint",
            "Temperature/Tint…",
            ADJUST,
            "{temperature: -100 (cooler) .. 100 (warmer) (0), tint: -100 (greener) .. 100 (more magenta) (0)} recolours as Brightness/Contrast does",
            json!({"temperature": 0.0, "tint": 0.0}),
        ),
    ]);
    for (suffix, label) in WARP_STYLES {
        let id: &'static str = match suffix {
            "arc" => "warp.arc",
            "arcLower" => "warp.arcLower",
            "arcUpper" => "warp.arcUpper",
            "arch" => "warp.arch",
            "bulge" => "warp.bulge",
            "shellLower" => "warp.shellLower",
            "shellUpper" => "warp.shellUpper",
            "flag" => "warp.flag",
            "wave" => "warp.wave",
            "fish" => "warp.fish",
            "rise" => "warp.rise",
            "fisheye" => "warp.fisheye",
            "inflate" => "warp.inflate",
            "squeeze" => "warp.squeeze",
            _ => "warp.twist",
        };
        v.push(g(id, label, WARP, WARP_DOC, json!({"bend": 50.0, "horizontal": 0.0, "vertical": 0.0, "orientation": "horizontal"})));
    }
    for (id, label, _) in PATHFINDER_EFFECTS {
        v.push(g(id, label, PATHFINDER, "{} (groups and layers: live Pathfinder over the members)", json!({})));
    }
    let view = |mut o: Value| {
        if let Some(m) = o.as_object_mut() {
            for (k, v) in [
                ("rotateX", json!(-18.0)),
                ("rotateY", json!(-26.0)),
                ("rotateZ", json!(8.0)),
                ("perspective", json!(0.0)),
                ("surface", json!("plastic")),
                ("lightIntensity", json!(100.0)),
                ("ambientLight", json!(50.0)),
                ("highlightIntensity", json!(60.0)),
                ("highlightSize", json!(90.0)),
            ] {
                m.insert(k.into(), v);
            }
        }
        o
    };
    v.push(g(
        EXTRUDE,
        "Extrude & Bevel…",
        THREE_D,
        "{depth: pt (0..2000, 50), cap: bool (true), rotateX, rotateY, rotateZ: deg (-180..180; -18, -26, 8), perspective: deg (0..160, 0), surface: \"wireframe\"|\"noShading\"|\"diffuse\"|\"plastic\", lightIntensity: % (100), ambientLight: % (50), highlightIntensity: % (60), highlightSize: % (90)} the art pushed back into a shaded solid",
        view(json!({"depth": 50.0, "cap": true})),
    ));
    v.push(g(
        REVOLVE,
        "Revolve…",
        THREE_D,
        "{angle: deg (1..360, 360), offset: pt (0), from: \"leftEdge\"|\"rightEdge\", cap: bool (true), rotateX, rotateY, rotateZ: deg (-180..180; -18, -26, 8), perspective: deg (0..160, 0), surface: \"wireframe\"|\"noShading\"|\"diffuse\"|\"plastic\", lightIntensity: % (100), ambientLight: % (50), highlightIntensity: % (60), highlightSize: % (90)} the art swept around a vertical axis at its edge",
        view(json!({"angle": 360.0, "offset": 0.0, "from": "leftEdge", "cap": true})),
    ));
    v.push(g(ROTATE, "Rotate…", THREE_D, "{rotateX, rotateY, rotateZ: deg (-180..180; -18, -26, 8), perspective: deg (0..160, 0), surface: \"wireframe\"|\"noShading\"|\"diffuse\"|\"plastic\", lightIntensity: % (100), ambientLight: % (50), highlightIntensity: % (60), highlightSize: % (90)} the flat art turned in space", view(json!({}))));
    v.push(g(
        CROP_MARKS,
        "Crop Marks",
        &["Effect"],
        "{style?: \"roman\"|\"japanese\" (default: the japaneseCropMarks preference when applied)} trim marks in [Registration] around the object's bounds, following it",
        json!({}),
    ));
    v
}

/// The catalogue built once, indexed by id (lookups run per effect per rendered frame).
fn catalog_index() -> &'static std::collections::HashMap<&'static str, EffectInfo> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<&'static str, EffectInfo>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| effect_catalog().into_iter().map(|e| (e.id, e)).collect())
}

/// Catalogue entry for `id` (built-in, or an installed plug-in effect).
pub fn effect_info(id: &str) -> Option<EffectInfo> {
    catalog_index().get(id).cloned().or_else(|| plugin_info(id))
}

/// The catalogue entry of the installed effect plug-in with effect id `id` (`plugin.<id>`).
fn plugin_info(id: &str) -> Option<EffectInfo> {
    use vectorcraft_plugins::registry::intern;
    let p = vectorcraft_plugins::effect::installed(id)?;
    let m = p.manifest();
    let dots = if m.params.is_empty() || m.name.ends_with('…') { "" } else { "…" };
    Some(EffectInfo {
        id: intern(id),
        label: intern(&format!("{}{dots}", m.name)),
        menu: PLUGINS,
        params: intern(&m.params_doc()),
        defaults: Value::Object(m.defaults()),
        raster: false,
        lengths: Lengths::default(),
    })
}

/// The installed effect plug-ins as catalogue entries (Effect › Plug-ins), by plug-in id.
pub fn plugin_effects() -> Vec<EffectInfo> {
    vectorcraft_plugins::registry::list().iter().filter_map(|p| plugin_info(&vectorcraft_plugins::effect::effect_id(p.id()))).collect()
}

/// Dialog defaults for `id` (`None` for unknown effects).
pub fn default_params(id: &str) -> Option<Value> {
    match catalog_index().get(id) {
        Some(e) => Some(e.defaults.clone()),
        None => plugin_info(id).map(|e| e.defaults),
    }
}

/// `params` layered over the defaults of `id` (unknown keys are kept).
pub fn merged_params(id: &str, params: &Value) -> Value {
    let mut out = match catalog_index().get(id).map(|e| &e.defaults) {
        Some(Value::Object(m)) => m.clone(),
        Some(_) => Map::new(),
        None => vectorcraft_plugins::effect::installed(id).map(|p| p.manifest().defaults()).unwrap_or_default(),
    };
    if let Value::Object(p) = params {
        for (k, v) in p {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// A new effect record with default parameters (overridden by `params`).
pub fn new_effect(id: &str, params: &Value) -> Option<Effect> {
    if !catalog_index().contains_key(id) {
        vectorcraft_plugins::effect::installed(id)?;
    }
    Some(Effect { id: id.to_string(), params: merged_params(id, params), visible: true })
}

/// Scale the distance parameters of `e` by `s` (Scale Strokes & Effects; see
/// [`EffectInfo::lengths`]). A distance left at its default is written out scaled; one the
/// effect doesn't have (Outline Stroke's weight following its stroke) stays absent.
pub fn scale_effect(e: &mut Effect, s: f64) {
    let Some(info) = catalog_index().get(e.id.as_str()) else { return };
    let merged = merged_params(&e.id, &e.params);
    let relative = util::flag(&merged, "relative", false);
    for k in info.lengths.keys(relative) {
        let v = util::num(&merged, k, f64::NAN);
        if v.is_finite() {
            if !e.params.is_object() {
                e.params = Value::Object(Map::new());
            }
            e.params[k] = json!(v * s);
        }
    }
}

/// Is `id` a raster (painted) effect?
pub fn is_raster(id: &str) -> bool {
    matches!(id, "stylize.dropShadow" | "stylize.innerGlow" | "stylize.outerGlow" | "stylize.feather" | "blur.gaussian")
        || PIXEL_EFFECTS.contains(&id)
        || id == SVG_FILTER
}

/// Effect › SVG Filters › Apply SVG Filter: `{name, filter}` (see [`svgfilter`]).
pub const SVG_FILTER: &str = "svg.filter";

/// Does `id` change geometry? (Crop Marks and the 3D effects draw art of their own instead,
/// [`crop_marks_art`], [`three_d_art`]; colour
/// adjustments recolour, [`adjust`].) Plug-in effects do, installed or not (a missing plug-in
/// leaves the geometry as it is).
pub fn is_geometry(id: &str) -> bool {
    !is_raster(id)
        && !is_pathfinder(id)
        && !is_adjustment(id)
        && id != CROP_MARKS
        && !is_three_d(id)
        && (catalog_index().contains_key(id) || vectorcraft_plugins::effect::plugin_id(id).is_some())
}

/// Does `n` draw as art of its own effects' making ([`object_art`])?
pub fn has_object_art(n: &Node) -> bool {
    has_crop_marks(n) || has_three_d(n)
}

/// The art an object with Crop Marks or a 3D effect draws as ([`crop_marks_art`],
/// [`three_d_art`]; with both, the crop marks go around the solid).
pub fn object_art(n: &Node) -> Option<Node> {
    if has_crop_marks(n) && has_three_d(n) {
        let mut solid = three_d_art(n)?;
        solid.appearance.effects = n.appearance.effects.clone();
        solid.appearance.effects.retain(|e| e.id == CROP_MARKS || is_raster(&e.id));
        return crop_marks_art(&solid);
    }
    crop_marks_art(n).or_else(|| three_d_art(n))
}

/// Any visible geometry effect in the list?
pub fn has_geometry(effects: &[Effect]) -> bool {
    effects.iter().any(|e| e.visible && is_geometry(&e.id))
}

/// Context the geometry effects may need from the object's appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeomContext<'a> {
    /// The stroke `path.outlineStroke` outlines (its weight, caps, joins, dashes, alignment,
    /// profile and arrowheads): the stroke the effect sits on, else the object's top painted
    /// stroke. `None` outlines a plain 1 pt stroke.
    pub stroke: Option<&'a StrokeLayer>,
    /// The object's fill rule (inside and outside alignment).
    pub rule: FillRule,
}

impl<'a> GeomContext<'a> {
    /// The context of `n`'s object-level effects.
    pub fn of(n: &'a Node) -> Self {
        let rule = match &n.kind {
            NodeKind::Path { rule, .. } | NodeKind::Compound { rule, .. } => *rule,
            _ => FillRule::NonZero,
        };
        Self { stroke: n.appearance.stroke().filter(|s| !s.paint.is_none() && s.width > 0.0), rule }
    }

    /// The context of the effects on `item`, one of the same object's appearance items: a
    /// stroke's own effects outline that stroke.
    pub fn item(self, item: &'a AppearanceItem) -> Self {
        match item {
            AppearanceItem::Stroke(s) => Self { stroke: Some(s), ..self },
            AppearanceItem::Fill(_) => self,
        }
    }
}

/// Evaluate the visible geometry effects of `effects` on `path`, in order. `bounds` is the
/// reference box of the first effect (usually the object's geometric bounds); each later effect
/// uses the bounds of its input. Raster and unknown effects are skipped.
pub fn apply_geometry(effects: &[Effect], path: &PathData, bounds: Rect) -> PathData {
    apply_geometry_with(effects, path, bounds, &GeomContext::default())
}

/// [`apply_geometry`] with an explicit [`GeomContext`].
pub fn apply_geometry_with(effects: &[Effect], path: &PathData, bounds: Rect, ctx: &GeomContext) -> PathData {
    let mut cur = path.clone();
    let mut b = bounds;
    let mut first = true;
    for e in effects.iter().filter(|e| e.visible && is_geometry(&e.id)) {
        if !first {
            b = cur.bounds().unwrap_or(b);
        }
        first = false;
        if cur.is_empty() {
            break;
        }
        let p = merged_params(&e.id, &e.params);
        cur = apply_one(&e.id, &p, &cur, b, ctx);
    }
    cur
}

/// Convenience wrapper on kurbo paths.
pub fn apply_geometry_bez(effects: &[Effect], path: &BezPath, bounds: Rect, ctx: &GeomContext) -> BezPath {
    apply_geometry_with(effects, &PathData::from_bezpath(path), bounds, ctx).to_bezpath()
}

pub(crate) fn apply_one(id: &str, p: &Value, path: &PathData, b: Rect, ctx: &GeomContext) -> PathData {
    use util::*;
    if let Some(plugin) = vectorcraft_plugins::effect::plugin_id(id) {
        return vectorcraft_plugins::effect::apply(plugin, p, path, b, ctx.rule).unwrap_or_else(|| path.clone());
    }
    match id {
        "distort.freeDistort" => distort::free_distort(path, b, p),
        "distort.puckerBloat" => distort::pucker_bloat(path, b, num(p, "amount", 0.0)),
        "distort.roughen" => distort::roughen(path, b, p),
        "distort.transform" => distort::transform(path, b, p),
        "distort.tweak" => distort::tweak(path, b, p),
        "distort.twist" => distort::twist(path, b, num(p, "angle", 10.0)),
        "distort.zigZag" => distort::zig_zag(path, b, p),
        "path.offsetPath" => {
            let off = num(p, "offset", 10.0).clamp(-1e5, 1e5);
            if off.abs() < 1e-9 {
                return path.clone();
            }
            vectorcraft_pathops::offset_path(path, off, join(p, "joins"), num(p, "miterLimit", 4.0).clamp(1.0, 500.0))
        }
        // The outlines are the object's: reaching here, type and live objects already are.
        "path.outlineObject" => path.clone(),
        "path.outlineStroke" => {
            let fallback;
            let base = match ctx.stroke {
                Some(s) => s,
                None => {
                    fallback = StrokeLayer::new(vectorcraft_doc::color::Paint::None, 1.0);
                    &fallback
                }
            };
            let st = match p.get("width").and_then(Value::as_f64) {
                Some(w) => std::borrow::Cow::Owned(StrokeLayer { width: w, ..base.clone() }),
                None => std::borrow::Cow::Borrowed(base),
            };
            if st.width.is_nan() || st.width <= 0.0 {
                return path.clone();
            }
            stroke::outline_region(path, ctx.rule, &st)
        }
        "convertToShape.rectangle" | "convertToShape.roundedRectangle" | "convertToShape.ellipse" => stylize::convert_to_shape(id, b, p),
        "stylize.roundCorners" => stylize::round_corners(path, num(p, "radius", 10.0)),
        "stylize.scribble" => stylize::scribble(path, b, p),
        _ => match id.strip_prefix("warp.").and_then(WarpStyle::from_id) {
            Some(style) => warp::warp(path, b, style, p),
            None => path.clone(),
        },
    }
}
