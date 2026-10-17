//! Photoshop-style raster effects (Effect › Rasterize, Artistic › Colored Pencil, Cutout, Dry Brush, Film
//! Grain, Fresco, Neon Glow, Paint Daubs, Palette Knife, Plastic Wrap, Poster Edges, Rough Pastels,
//! Smudge Stick, Sponge, Underpainting and Watercolor, Blur › Radial Blur and Smart Blur, Brush Strokes ›
//! Accented Edges, Angled Strokes, Crosshatch, Dark Strokes, Ink Outlines, Spatter, Sprayed Strokes
//! and Sumi-e, Distort › Diffuse
//! Glow, Glass and Ocean Ripple, Pixelate › Color
//! Halftone, Crystallize, Mezzotint and Pointillize, Sharpen › Unsharp Mask, Stylize › Glowing
//! Edges, Sketch › Bas Relief, Chalk & Charcoal, Charcoal, Chrome, Conté Crayon, Graphic Pen,
//! Halftone Pattern, Note Paper, Photocopy, Plaster, Reticulation, Stamp, Torn Edges and Water
//! Paper, Texture › Craquelure, Grain, Mosaic Tiles, Patchwork, Stained Glass and Texturizer, and
//! Video › De-Interlace and NTSC Colors): filters over premultiplied RGBA8 pixels.
//!
//! - Every distance is in document units (points) and becomes pixels through the raster's
//!   [`PixelSpace::px`], so an effect looks the same at any zoom and at any Document Raster Effects
//!   Settings resolution (as Gaussian Blur's radius does).
//! - The work per pixel is bounded whatever the parameters: blurs are box approximations whose
//!   cost doesn't grow with the radius, and sample counts come from the effect's quality.
//! - Beyond the raster's edges is transparency.

mod artistic;
mod blur;
mod brush;
mod distort;
mod edges;
mod noise;
mod pixelate;
mod rasterize;
mod sharpen;
mod sketch;
mod texture;
mod video;

pub use artistic::{Artistic, Daub};
pub use brush::{BrushStroke, Direction};
pub use distort::{Glass, GlassTexture};
pub use pixelate::Mezzotint;
pub use rasterize::Rasterize;
pub use sketch::{Screen, Sketch};
pub use texture::{Grain, Light, Surface, Texturizer};

use serde_json::Value;
use vectorcraft_geom::{Affine, Point, Rect};

use crate::util::{num, text};

/// The Photoshop-style effect ids (all raster effects, see [`crate::is_raster`]).
pub const PIXEL_EFFECTS: [&str; 57] = [
    "artistic.coloredPencil",
    "artistic.cutout",
    "artistic.dryBrush",
    "artistic.filmGrain",
    "artistic.fresco",
    "artistic.neonGlow",
    "artistic.paintDaubs",
    "artistic.paletteKnife",
    "artistic.plasticWrap",
    "artistic.posterEdges",
    "artistic.roughPastels",
    "artistic.smudgeStick",
    "artistic.sponge",
    "artistic.underpainting",
    "artistic.watercolor",
    "blur.radial",
    "blur.smart",
    "brushStrokes.accentedEdges",
    "brushStrokes.angledStrokes",
    "brushStrokes.crosshatch",
    "brushStrokes.darkStrokes",
    "brushStrokes.inkOutlines",
    "brushStrokes.spatter",
    "brushStrokes.sprayedStrokes",
    "brushStrokes.sumiE",
    "distort.diffuseGlow",
    "distort.glass",
    "distort.oceanRipple",
    "pixelate.colorHalftone",
    "pixelate.crystallize",
    "pixelate.mezzotint",
    "pixelate.pointillize",
    "rasterize",
    "sharpen.unsharpMask",
    "sketch.basRelief",
    "sketch.chalkCharcoal",
    "sketch.charcoal",
    "sketch.chrome",
    "sketch.conteCrayon",
    "sketch.graphicPen",
    "sketch.halftonePattern",
    "sketch.notePaper",
    "sketch.photocopy",
    "sketch.plaster",
    "sketch.reticulation",
    "sketch.stamp",
    "sketch.tornEdges",
    "sketch.waterPaper",
    "stylize.glowingEdges",
    "texture.craquelure",
    "texture.grain",
    "texture.mosaicTiles",
    "texture.patchwork",
    "texture.stainedGlass",
    "texture.texturizer",
    "video.deInterlace",
    "video.ntscColors",
];

/// A Photoshop-style raster effect with its parameters read and clamped to their ranges. Lengths
/// are in document units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PixelFx {
    /// Blur › Radial Blur around the object's centre: along arcs (spin, an arc of `amount`°) or
    /// along rays (zoom, the content scaled by up to about ±`amount`/2 %), averaging 2^`passes` samples.
    RadialBlur { amount: f64, zoom: bool, passes: u32 },
    /// Blur › Smart Blur (Normal mode): each pixel averaged with the neighbours within `radius`
    /// whose colour differs from its own by no more than `threshold` levels, over a grid of
    /// `samples` × `samples` neighbours at most.
    SmartBlur { radius: f64, threshold: f64, samples: u32 },
    /// Sharpen › Unsharp Mask: colours pushed away from a Gaussian blur of σ = `radius` by `amount`
    /// (1 = 100 %), where they differ from it by at least `threshold` levels.
    UnsharpMask { amount: f64, radius: f64, threshold: f64 },
    /// Stylize › Glowing Edges: bright coloured Sobel outlines on black.
    GlowingEdges { width: f64, brightness: f64, smoothness: f64 },
    /// Pixelate › Color Halftone: each colour channel as dots of up to `radius` on a screen turned
    /// by its angle (degrees; red, green, blue).
    ColorHalftone { radius: f64, angles: [f64; 3] },
    /// Pixelate › Crystallize: Voronoi cells about `size` across, each its average colour.
    Crystallize { size: f64 },
    /// Pixelate › Mezzotint: every channel full or none against random grain of `kind`.
    Mezzotint { kind: Mezzotint },
    /// Pixelate › Pointillize: dots about `size` apart of the colour under them, on white.
    Pointillize { size: f64 },
    /// Distort › Diffuse Glow: white glow over the highlights with white grain.
    DiffuseGlow { grain: f64, glow: f64, clear: f64 },
    /// Distort › Glass: the content seen through a textured pane.
    Glass(Glass),
    /// Distort › Ocean Ripple: the content wobbled under ripples of `size`, by `magnitude`.
    OceanRipple { size: f64, magnitude: f64 },
    /// Video › De-Interlace: the `odd` (else even) scan lines rebuilt from their neighbours, by
    /// `interpolate`-ing the lines either side or duplicating the one above.
    DeInterlace { odd: bool, interpolate: bool },
    /// Video › NTSC Colors: colours desaturated into the broadcast-safe range.
    NtscColors,
    /// Texture › Craquelure: cracked plaster with plates about `spacing` across.
    Craquelure { spacing: f64, depth: f64, brightness: f64 },
    /// Texture › Grain: noise of `kind` after a contrast change.
    Grain { intensity: f64, contrast: f64, kind: Grain },
    /// Texture › Mosaic Tiles: raised square tiles `tile` across in lightened grout.
    MosaicTiles { tile: f64, grout: f64, lighten: f64 },
    /// Texture › Patchwork: raised squares `square` across, each its average colour.
    Patchwork { square: f64, relief: f64 },
    /// Texture › Stained Glass: leaded panes about `size` across, each its average colour.
    StainedGlass { size: f64, border: f64, light: f64 },
    /// Texture › Texturizer: the art on a lit surface.
    Texturizer(Texturizer),
    /// Effect › Rasterize: the object shown as an image of its own resolution and colour model.
    Rasterize(Rasterize),
    /// Brush Strokes › any of its filters.
    Brush(BrushStroke),
    /// Artistic › any of its filters.
    Artistic(Artistic),
    /// Sketch › any of its filters.
    Sketch(Sketch),
}

/// Where a raster's pixels lie in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelSpace {
    /// Pixel coordinates (x right, y down, (0, 0) the top-left corner of the first pixel) →
    /// document coordinates.
    pub to_doc: Affine,
    /// The size of a pixel in document units.
    pub px: f64,
    /// The object's centre (document coordinates): where Radial Blur blurs around.
    pub center: Point,
    /// The bounds of the object's painted content (document coordinates): where Rasterize's
    /// background and image grid lie.
    pub bounds: Rect,
}

/// The kind in `kinds` (id, label, kind) named `name` by id or label, else `default`.
fn pick<T: Copy, const N: usize>(kinds: &[(&str, &str, T); N], name: &str, default: T) -> T {
    kinds.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(name) || label.eq_ignore_ascii_case(name)).map_or(default, |k| k.2)
}

/// A Texturizer surface read from `p` (`texture`, `scaling`, `relief`, `light`, `invert`).
fn texturizer(p: &Value, relief: f64, light: Light) -> Texturizer {
    Texturizer {
        surface: pick(&Surface::KINDS, text(p, "texture", "canvas"), Surface::Canvas),
        scaling: num(p, "scaling", 100.0).clamp(50.0, 200.0) / 100.0,
        relief: num(p, "relief", relief).clamp(0.0, 50.0),
        light: pick(&Light::KINDS, text(p, "light", ""), light),
        invert: crate::util::flag(p, "invert", false),
    }
}

/// Effect `id`'s parameters `p` (defaults merged in) as a [`PixelFx`]; `None` for other effects.
pub(crate) fn parse(id: &str, p: &Value) -> Option<PixelFx> {
    let quality = |levels: [(&str, u32); 3], default: u32| {
        let q = text(p, "quality", "");
        levels.iter().find(|(name, _)| name.eq_ignore_ascii_case(q)).map_or(default, |(_, v)| *v)
    };
    Some(match id {
        "blur.radial" => PixelFx::RadialBlur {
            amount: num(p, "amount", 10.0).clamp(1.0, 100.0),
            zoom: text(p, "method", "spin").eq_ignore_ascii_case("zoom"),
            passes: quality([("draft", 4), ("good", 6), ("best", 8)], 6),
        },
        "blur.smart" => PixelFx::SmartBlur {
            radius: num(p, "radius", 3.0).clamp(0.1, 100.0),
            threshold: num(p, "threshold", 25.0).clamp(0.1, 100.0),
            samples: quality([("low", 5), ("medium", 7), ("high", 9)], 7),
        },
        "sharpen.unsharpMask" => PixelFx::UnsharpMask {
            amount: num(p, "amount", 50.0).clamp(1.0, 500.0) / 100.0,
            radius: num(p, "radius", 1.0).clamp(0.1, 250.0),
            threshold: num(p, "threshold", 0.0).clamp(0.0, 255.0),
        },
        "stylize.glowingEdges" => PixelFx::GlowingEdges {
            width: num(p, "edgeWidth", 2.0).clamp(1.0, 14.0),
            brightness: num(p, "edgeBrightness", 6.0).clamp(0.0, 20.0),
            smoothness: num(p, "smoothness", 5.0).clamp(1.0, 15.0),
        },
        "pixelate.colorHalftone" => {
            let angle = |key: &str, default: f64| {
                let a = num(p, key, default);
                if a.is_finite() { a.clamp(-360.0, 360.0) } else { default }
            };
            PixelFx::ColorHalftone {
                radius: num(p, "maxRadius", 8.0).clamp(4.0, 127.0),
                angles: [angle("angle1", 108.0), angle("angle2", 162.0), angle("angle3", 90.0)],
            }
        }
        "pixelate.crystallize" => PixelFx::Crystallize { size: num(p, "cellSize", 10.0).clamp(3.0, 300.0) },
        "pixelate.mezzotint" => {
            let kind = text(p, "type", "fineDots");
            let kind = Mezzotint::KINDS.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(kind) || label.eq_ignore_ascii_case(kind));
            PixelFx::Mezzotint { kind: kind.map_or(Mezzotint::FineDots, |k| k.2) }
        }
        "pixelate.pointillize" => PixelFx::Pointillize { size: num(p, "cellSize", 5.0).clamp(3.0, 300.0) },
        "distort.diffuseGlow" => PixelFx::DiffuseGlow {
            grain: num(p, "graininess", 6.0).clamp(0.0, 10.0),
            glow: num(p, "glowAmount", 10.0).clamp(0.0, 20.0),
            clear: num(p, "clearAmount", 15.0).clamp(0.0, 20.0),
        },
        "distort.glass" => {
            let texture = text(p, "texture", "frosted");
            let texture = GlassTexture::KINDS.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(texture) || label.eq_ignore_ascii_case(texture));
            PixelFx::Glass(Glass {
                distortion: num(p, "distortion", 5.0).clamp(0.0, 20.0),
                smoothness: num(p, "smoothness", 3.0).clamp(1.0, 15.0),
                texture: texture.map_or(GlassTexture::Frosted, |t| t.2),
                scaling: num(p, "scaling", 100.0).clamp(50.0, 200.0) / 100.0,
                invert: crate::util::flag(p, "invert", false),
            })
        }
        "distort.oceanRipple" => {
            PixelFx::OceanRipple { size: num(p, "rippleSize", 9.0).clamp(1.0, 15.0), magnitude: num(p, "rippleMagnitude", 9.0).clamp(0.0, 20.0) }
        }
        "video.deInterlace" => PixelFx::DeInterlace {
            odd: !text(p, "eliminate", "odd").eq_ignore_ascii_case("even"),
            interpolate: !text(p, "create", "interpolation").eq_ignore_ascii_case("duplication"),
        },
        "video.ntscColors" => PixelFx::NtscColors,
        "brushStrokes.accentedEdges" => PixelFx::Brush(BrushStroke::AccentedEdges {
            width: num(p, "edgeWidth", 2.0).clamp(1.0, 14.0),
            brightness: num(p, "edgeBrightness", 38.0).clamp(0.0, 50.0),
            smoothness: num(p, "smoothness", 5.0).clamp(1.0, 15.0),
        }),
        "brushStrokes.angledStrokes" => PixelFx::Brush(BrushStroke::AngledStrokes {
            balance: num(p, "directionBalance", 50.0).clamp(0.0, 100.0),
            length: num(p, "strokeLength", 15.0).clamp(3.0, 50.0),
            sharpness: num(p, "sharpness", 3.0).clamp(0.0, 10.0),
        }),
        "brushStrokes.crosshatch" => PixelFx::Brush(BrushStroke::Crosshatch {
            length: num(p, "strokeLength", 9.0).clamp(3.0, 50.0),
            sharpness: num(p, "sharpness", 6.0).clamp(0.0, 20.0),
            strength: num(p, "strength", 1.0).round().clamp(1.0, 3.0) as u32,
        }),
        "brushStrokes.darkStrokes" => PixelFx::Brush(BrushStroke::DarkStrokes {
            balance: num(p, "balance", 5.0).clamp(0.0, 10.0),
            black: num(p, "blackIntensity", 6.0).clamp(0.0, 10.0),
            white: num(p, "whiteIntensity", 2.0).clamp(0.0, 10.0),
        }),
        "brushStrokes.inkOutlines" => PixelFx::Brush(BrushStroke::InkOutlines {
            length: num(p, "strokeLength", 4.0).clamp(1.0, 50.0),
            dark: num(p, "darkIntensity", 20.0).clamp(0.0, 50.0),
            light: num(p, "lightIntensity", 10.0).clamp(0.0, 50.0),
        }),
        "brushStrokes.spatter" => PixelFx::Brush(BrushStroke::Spatter {
            radius: num(p, "sprayRadius", 10.0).clamp(0.0, 25.0),
            smoothness: num(p, "smoothness", 5.0).clamp(1.0, 15.0),
        }),
        "brushStrokes.sprayedStrokes" => {
            let dir = text(p, "strokeDirection", "rightDiagonal");
            let dir = Direction::KINDS.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(dir) || label.eq_ignore_ascii_case(dir));
            PixelFx::Brush(BrushStroke::SprayedStrokes {
                length: num(p, "strokeLength", 12.0).clamp(0.0, 20.0),
                radius: num(p, "sprayRadius", 7.0).clamp(0.0, 25.0),
                direction: dir.map_or(Direction::RightDiagonal, |d| d.2),
            })
        }
        "brushStrokes.sumiE" => PixelFx::Brush(BrushStroke::SumiE {
            width: num(p, "strokeWidth", 10.0).clamp(3.0, 15.0),
            pressure: num(p, "strokePressure", 2.0).clamp(0.0, 15.0),
            contrast: num(p, "contrast", 16.0).clamp(0.0, 40.0),
        }),
        "artistic.coloredPencil" => PixelFx::Artistic(Artistic::ColoredPencil {
            width: num(p, "pencilWidth", 4.0).clamp(1.0, 24.0),
            pressure: num(p, "strokePressure", 8.0).clamp(0.0, 15.0),
            paper: num(p, "paperBrightness", 25.0).clamp(0.0, 50.0),
        }),
        "artistic.cutout" => PixelFx::Artistic(Artistic::Cutout {
            levels: num(p, "numberOfLevels", 4.0).clamp(2.0, 8.0),
            simplicity: num(p, "edgeSimplicity", 4.0).clamp(0.0, 10.0),
            fidelity: num(p, "edgeFidelity", 2.0).clamp(1.0, 3.0),
        }),
        "artistic.dryBrush" => PixelFx::Artistic(Artistic::DryBrush {
            size: num(p, "brushSize", 2.0).clamp(0.0, 10.0),
            detail: num(p, "brushDetail", 8.0).clamp(0.0, 10.0),
            texture: num(p, "texture", 1.0).clamp(1.0, 3.0),
        }),
        "artistic.filmGrain" => PixelFx::Artistic(Artistic::FilmGrain {
            grain: num(p, "grain", 4.0).clamp(0.0, 20.0),
            highlight: num(p, "highlightArea", 0.0).clamp(0.0, 20.0),
            intensity: num(p, "intensity", 10.0).clamp(0.0, 10.0),
        }),
        "artistic.fresco" => PixelFx::Artistic(Artistic::Fresco {
            size: num(p, "brushSize", 2.0).clamp(0.0, 10.0),
            detail: num(p, "brushDetail", 8.0).clamp(0.0, 10.0),
            texture: num(p, "texture", 1.0).clamp(1.0, 3.0),
        }),
        "artistic.neonGlow" => PixelFx::Artistic(Artistic::NeonGlow {
            size: num(p, "glowSize", 5.0).clamp(-24.0, 24.0),
            brightness: num(p, "glowBrightness", 15.0).clamp(0.0, 50.0),
            color: crate::raster::color_param(p, vectorcraft_doc::color::Color::rgb(0.25, 0.5, 1.0)).to_rgb(),
        }),
        "artistic.paintDaubs" => PixelFx::Artistic(Artistic::PaintDaubs {
            size: num(p, "brushSize", 8.0).clamp(1.0, 50.0),
            sharpness: num(p, "sharpness", 7.0).clamp(0.0, 40.0),
            brush: pick(&Daub::KINDS, text(p, "brushType", "simple"), Daub::Simple),
        }),
        "artistic.paletteKnife" => PixelFx::Artistic(Artistic::PaletteKnife {
            size: num(p, "strokeSize", 25.0).clamp(1.0, 50.0),
            detail: num(p, "strokeDetail", 3.0).clamp(1.0, 3.0),
            softness: num(p, "softness", 0.0).clamp(0.0, 10.0),
        }),
        "artistic.plasticWrap" => PixelFx::Artistic(Artistic::PlasticWrap {
            strength: num(p, "highlightStrength", 15.0).clamp(0.0, 20.0),
            detail: num(p, "detail", 9.0).clamp(1.0, 15.0),
            smoothness: num(p, "smoothness", 7.0).clamp(1.0, 15.0),
        }),
        "artistic.posterEdges" => PixelFx::Artistic(Artistic::PosterEdges {
            thickness: num(p, "edgeThickness", 2.0).clamp(0.0, 10.0),
            intensity: num(p, "edgeIntensity", 1.0).clamp(0.0, 10.0),
            posterization: num(p, "posterization", 2.0).clamp(0.0, 6.0),
        }),
        "artistic.roughPastels" => PixelFx::Artistic(Artistic::RoughPastels {
            length: num(p, "strokeLength", 6.0).clamp(0.0, 40.0),
            detail: num(p, "strokeDetail", 4.0).clamp(1.0, 20.0),
            paper: texturizer(p, 20.0, Light::Bottom),
        }),
        "artistic.smudgeStick" => PixelFx::Artistic(Artistic::SmudgeStick {
            length: num(p, "strokeLength", 2.0).clamp(0.0, 10.0),
            highlight: num(p, "highlightArea", 0.0).clamp(0.0, 20.0),
            intensity: num(p, "intensity", 10.0).clamp(0.0, 10.0),
        }),
        "artistic.sponge" => PixelFx::Artistic(Artistic::Sponge {
            size: num(p, "brushSize", 2.0).clamp(0.0, 10.0),
            definition: num(p, "definition", 12.0).clamp(0.0, 25.0),
            smoothness: num(p, "smoothness", 5.0).clamp(1.0, 15.0),
        }),
        "artistic.underpainting" => PixelFx::Artistic(Artistic::Underpainting {
            size: num(p, "brushSize", 6.0).clamp(0.0, 40.0),
            coverage: num(p, "textureCoverage", 16.0).clamp(0.0, 40.0),
            paper: texturizer(p, 4.0, Light::Top),
        }),
        "artistic.watercolor" => PixelFx::Artistic(Artistic::Watercolor {
            detail: num(p, "brushDetail", 9.0).clamp(1.0, 14.0),
            shadow: num(p, "shadowIntensity", 1.0).clamp(0.0, 10.0),
            texture: num(p, "texture", 1.0).clamp(1.0, 3.0),
        }),
        "rasterize" => {
            let ppi = match text(p, "resolution", "document") {
                r if r.eq_ignore_ascii_case("screen") => Some(72.0),
                r if r.eq_ignore_ascii_case("medium") => Some(150.0),
                r if r.eq_ignore_ascii_case("high") => Some(300.0),
                r if r.eq_ignore_ascii_case("other") => Some(num(p, "ppi", 300.0).clamp(1.0, 2400.0)),
                _ => None,
            };
            let model = text(p, "colorModel", "document");
            PixelFx::Rasterize(Rasterize {
                ppi,
                color_model: vectorcraft_doc::RasterColorModel::ALL.into_iter().find(|m| m.id().eq_ignore_ascii_case(model)).unwrap_or_default(),
                white: !text(p, "background", "white").eq_ignore_ascii_case("transparent"),
                anti_alias: crate::util::flag(p, "antiAlias", true),
                clipping_mask: crate::util::flag(p, "clippingMask", false),
                add_around: num(p, "addAround", 0.0).clamp(0.0, vectorcraft_doc::rastersettings::MAX_ADD_AROUND),
            })
        }
        "sketch.basRelief" => PixelFx::Sketch(Sketch::BasRelief {
            detail: num(p, "detail", 13.0).clamp(1.0, 15.0),
            smoothness: num(p, "smoothness", 3.0).clamp(1.0, 15.0),
            light: pick(&Light::KINDS, text(p, "light", "bottom"), Light::Bottom),
        }),
        "sketch.chalkCharcoal" => PixelFx::Sketch(Sketch::ChalkCharcoal {
            charcoal: num(p, "charcoalArea", 6.0).clamp(0.0, 20.0),
            chalk: num(p, "chalkArea", 6.0).clamp(0.0, 20.0),
            pressure: num(p, "strokePressure", 1.0).clamp(0.0, 5.0),
        }),
        "sketch.charcoal" => PixelFx::Sketch(Sketch::Charcoal {
            thickness: num(p, "charcoalThickness", 1.0).clamp(1.0, 7.0),
            detail: num(p, "detail", 5.0).clamp(0.0, 5.0),
            balance: num(p, "lightDarkBalance", 50.0).clamp(0.0, 100.0),
        }),
        "sketch.chrome" => {
            PixelFx::Sketch(Sketch::Chrome { detail: num(p, "detail", 4.0).clamp(0.0, 10.0), smoothness: num(p, "smoothness", 7.0).clamp(0.0, 10.0) })
        }
        "sketch.conteCrayon" => PixelFx::Sketch(Sketch::ConteCrayon {
            foreground: num(p, "foregroundLevel", 11.0).clamp(1.0, 15.0),
            background: num(p, "backgroundLevel", 7.0).clamp(1.0, 15.0),
            paper: Texturizer {
                surface: pick(&Surface::KINDS, text(p, "texture", "canvas"), Surface::Canvas),
                scaling: num(p, "scaling", 100.0).clamp(50.0, 200.0) / 100.0,
                relief: num(p, "relief", 4.0).clamp(0.0, 50.0),
                light: pick(&Light::KINDS, text(p, "light", "top"), Light::Top),
                invert: crate::util::flag(p, "invert", false),
            },
        }),
        "sketch.graphicPen" => PixelFx::Sketch(Sketch::GraphicPen {
            length: num(p, "strokeLength", 15.0).clamp(1.0, 15.0),
            balance: num(p, "lightDarkBalance", 50.0).clamp(0.0, 100.0),
            direction: pick(&Direction::KINDS, text(p, "strokeDirection", "rightDiagonal"), Direction::RightDiagonal),
        }),
        "sketch.halftonePattern" => PixelFx::Sketch(Sketch::HalftonePattern {
            size: num(p, "size", 1.0).clamp(1.0, 12.0),
            contrast: num(p, "contrast", 5.0).clamp(0.0, 50.0),
            screen: pick(&Screen::KINDS, text(p, "patternType", "dot"), Screen::Dot),
        }),
        "sketch.notePaper" => PixelFx::Sketch(Sketch::NotePaper {
            balance: num(p, "imageBalance", 25.0).clamp(0.0, 50.0),
            grain: num(p, "graininess", 10.0).clamp(0.0, 20.0),
            relief: num(p, "relief", 11.0).clamp(0.0, 25.0),
        }),
        "sketch.photocopy" => {
            PixelFx::Sketch(Sketch::Photocopy { detail: num(p, "detail", 7.0).clamp(1.0, 24.0), darkness: num(p, "darkness", 8.0).clamp(1.0, 50.0) })
        }
        "sketch.plaster" => PixelFx::Sketch(Sketch::Plaster {
            balance: num(p, "imageBalance", 20.0).clamp(0.0, 50.0),
            smoothness: num(p, "smoothness", 2.0).clamp(1.0, 15.0),
            light: pick(&Light::KINDS, text(p, "light", "top"), Light::Top),
        }),
        "sketch.reticulation" => PixelFx::Sketch(Sketch::Reticulation {
            density: num(p, "density", 12.0).clamp(0.0, 50.0),
            foreground: num(p, "foregroundLevel", 40.0).clamp(0.0, 50.0),
            background: num(p, "backgroundLevel", 5.0).clamp(0.0, 50.0),
        }),
        "sketch.stamp" => PixelFx::Sketch(Sketch::Stamp {
            balance: num(p, "lightDarkBalance", 25.0).clamp(0.0, 50.0),
            smoothness: num(p, "smoothness", 5.0).clamp(1.0, 50.0),
        }),
        "sketch.tornEdges" => PixelFx::Sketch(Sketch::TornEdges {
            balance: num(p, "imageBalance", 25.0).clamp(0.0, 50.0),
            smoothness: num(p, "smoothness", 11.0).clamp(1.0, 15.0),
            contrast: num(p, "contrast", 17.0).clamp(1.0, 25.0),
        }),
        "sketch.waterPaper" => PixelFx::Sketch(Sketch::WaterPaper {
            fiber: num(p, "fiberLength", 15.0).clamp(3.0, 50.0),
            brightness: num(p, "brightness", 60.0).clamp(0.0, 100.0),
            contrast: num(p, "contrast", 80.0).clamp(0.0, 100.0),
        }),
        "texture.craquelure" => PixelFx::Craquelure {
            spacing: num(p, "crackSpacing", 15.0).clamp(2.0, 100.0),
            depth: num(p, "crackDepth", 6.0).clamp(0.0, 10.0),
            brightness: num(p, "crackBrightness", 9.0).clamp(0.0, 10.0),
        },
        "texture.grain" => {
            let kind = text(p, "grainType", "regular");
            let kind = Grain::KINDS.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(kind) || label.eq_ignore_ascii_case(kind));
            PixelFx::Grain {
                intensity: num(p, "intensity", 40.0).clamp(0.0, 100.0),
                contrast: num(p, "contrast", 50.0).clamp(0.0, 100.0),
                kind: kind.map_or(Grain::Regular, |k| k.2),
            }
        }
        "texture.mosaicTiles" => PixelFx::MosaicTiles {
            tile: num(p, "tileSize", 12.0).clamp(2.0, 100.0),
            grout: num(p, "groutWidth", 3.0).clamp(1.0, 15.0),
            lighten: num(p, "lightenGrout", 9.0).clamp(0.0, 10.0),
        },
        "texture.patchwork" => {
            PixelFx::Patchwork { square: num(p, "squareSize", 4.0).clamp(0.0, 10.0) + 2.0, relief: num(p, "relief", 8.0).clamp(0.0, 25.0) }
        }
        "texture.stainedGlass" => PixelFx::StainedGlass {
            size: num(p, "cellSize", 10.0).clamp(2.0, 50.0),
            border: num(p, "borderThickness", 4.0).clamp(1.0, 20.0),
            light: num(p, "lightIntensity", 3.0).clamp(0.0, 10.0),
        },
        "texture.texturizer" => {
            let surface = text(p, "texture", "canvas");
            let surface = Surface::KINDS.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(surface) || label.eq_ignore_ascii_case(surface));
            let light = text(p, "light", "top");
            let light = Light::KINDS.iter().find(|(id, label, _)| id.eq_ignore_ascii_case(light) || label.eq_ignore_ascii_case(light));
            PixelFx::Texturizer(Texturizer {
                surface: surface.map_or(Surface::Canvas, |t| t.2),
                scaling: num(p, "scaling", 100.0).clamp(50.0, 200.0) / 100.0,
                relief: num(p, "relief", 4.0).clamp(0.0, 50.0),
                light: light.map_or(Light::Top, |l| l.2),
                invert: crate::util::flag(p, "invert", false),
            })
        }
        _ => return None,
    })
}

impl PixelFx {
    /// How far beyond `bounds` (the content's, document units) the effect can paint.
    pub fn outset(&self, bounds: Rect) -> f64 {
        // From the centre to the farthest corner, and to the nearest side.
        let far = 0.5 * bounds.width().hypot(bounds.height());
        let near = 0.5 * bounds.width().min(bounds.height());
        match *self {
            // A rotated corner reaches the circle through the corners.
            PixelFx::RadialBlur { zoom: false, .. } => (far - near).max(0.0),
            PixelFx::RadialBlur { amount, zoom: true, .. } => far * (blur::zoom_extent(amount).exp() - 1.0),
            PixelFx::SmartBlur { radius, .. } => radius,
            PixelFx::UnsharpMask { .. } => 0.0,
            PixelFx::GlowingEdges { width, smoothness, .. } => width.max(smoothness * 3.0),
            // Cells and dots straddling the edge reach out by up to a cell.
            PixelFx::Crystallize { size } | PixelFx::Pointillize { size } => size,
            PixelFx::ColorHalftone { .. } | PixelFx::Mezzotint { .. } | PixelFx::DiffuseGlow { .. } => 0.0,
            PixelFx::DeInterlace { .. } | PixelFx::NtscColors => 0.0,
            PixelFx::Craquelure { .. }
            | PixelFx::Grain { .. }
            | PixelFx::MosaicTiles { .. }
            | PixelFx::Patchwork { .. }
            | PixelFx::StainedGlass { .. }
            | PixelFx::Texturizer(_) => 0.0,
            PixelFx::Brush(b) => b.outset(),
            // Sketches and artistic media repaint the art's own coverage.
            PixelFx::Sketch(_) | PixelFx::Artistic(_) => 0.0,
            PixelFx::Rasterize(r) => r.outset(),
            // Displacements pull content out by up to their reach.
            PixelFx::Glass(g) => g.distortion * 0.5,
            PixelFx::OceanRipple { magnitude, .. } => magnitude * 0.5,
        }
    }

    /// How far (document units) from a pixel the content that decides it lies; `None` when that
    /// is anywhere in the object (Radial Blur).
    pub fn reach(&self) -> Option<f64> {
        match *self {
            PixelFx::RadialBlur { .. } => None,
            PixelFx::SmartBlur { radius, .. } => Some(radius),
            PixelFx::UnsharpMask { radius, .. } => Some(3.0 * radius),
            PixelFx::GlowingEdges { width, smoothness, .. } => Some(width.max(smoothness * 3.0)),
            // A Voronoi cell lies within its seed's neighbouring grid cells.
            PixelFx::Crystallize { size } => Some(3.0 * size),
            PixelFx::Pointillize { size } => Some(2.0 * size),
            PixelFx::ColorHalftone { radius, .. } => Some(4.0 * radius),
            PixelFx::Mezzotint { .. } => Some(0.0),
            PixelFx::DiffuseGlow { .. } => Some(4.5),
            // The steepest slope is found over the whole raster.
            PixelFx::Glass(_) => None,
            PixelFx::OceanRipple { magnitude, .. } => Some(magnitude * 0.5 + 1.0),
            PixelFx::DeInterlace { .. } => Some(2.0),
            PixelFx::NtscColors | PixelFx::Grain { .. } => Some(0.0),
            // Reliefs shade by the slopes between neighbouring pixels.
            PixelFx::Craquelure { .. } | PixelFx::MosaicTiles { .. } | PixelFx::Texturizer(_) => Some(1.0),
            PixelFx::Patchwork { square, .. } => Some(square),
            PixelFx::StainedGlass { size, .. } => Some(3.0 * size),
            PixelFx::Brush(b) => Some(b.reach()),
            // A pixel takes the average of its image cell.
            PixelFx::Rasterize(r) => Some(1.5 * r.cell().unwrap_or(0.0)),
            PixelFx::Sketch(k) => Some(k.reach()),
            PixelFx::Artistic(a) => Some(a.reach()),
        }
    }

    /// Run the effect on `data`, `w` × `h` premultiplied RGBA8 pixels lying in the document as
    /// `space` says. Does nothing when the sizes don't match.
    pub fn apply(&self, data: &mut [u8], w: usize, h: usize, space: &PixelSpace) {
        let Some(px) = pixels(data, w, h) else { return };
        let to_px = |len: f64| len / space.px.max(1e-9);
        match *self {
            PixelFx::RadialBlur { amount, zoom, passes } => blur::radial(px, w, h, space, amount, zoom, passes),
            PixelFx::SmartBlur { radius, threshold, samples } => blur::smart(px, w, h, to_px(radius), threshold, samples),
            PixelFx::UnsharpMask { amount, radius, threshold } => sharpen::unsharp(px, w, h, amount, to_px(radius), threshold),
            PixelFx::GlowingEdges { width, brightness, smoothness } => edges::glow(px, w, h, to_px(width), brightness, to_px(smoothness)),
            PixelFx::ColorHalftone { radius, angles } => pixelate::halftone(px, w, h, space, radius, angles),
            PixelFx::Crystallize { size } => pixelate::crystallize(px, w, h, space, size),
            PixelFx::Mezzotint { kind } => pixelate::mezzotint(px, w, h, space, kind),
            PixelFx::Pointillize { size } => pixelate::pointillize(px, w, h, space, size),
            PixelFx::DiffuseGlow { grain, glow, clear } => distort::diffuse_glow(px, w, h, space, grain, glow, clear),
            PixelFx::Glass(g) => distort::glass(px, w, h, space, g),
            PixelFx::OceanRipple { size, magnitude } => distort::ocean_ripple(px, w, h, space, size, magnitude),
            PixelFx::DeInterlace { odd, interpolate } => video::de_interlace(px, w, h, space, odd, interpolate),
            PixelFx::NtscColors => video::ntsc_colors(px),
            PixelFx::Craquelure { spacing, depth, brightness } => texture::craquelure(px, w, h, space, spacing, depth, brightness),
            PixelFx::Grain { intensity, contrast, kind } => texture::grain(px, w, h, space, intensity, contrast, kind),
            PixelFx::MosaicTiles { tile, grout, lighten } => texture::mosaic_tiles(px, w, h, space, tile, grout, lighten),
            PixelFx::Patchwork { square, relief } => texture::patchwork(px, w, h, space, square, relief),
            PixelFx::StainedGlass { size, border, light } => texture::stained_glass(px, w, h, space, size, border, light),
            PixelFx::Texturizer(t) => texture::texturize(px, w, h, space, t),
            PixelFx::Brush(b) => b.apply(px, w, h, space),
            PixelFx::Rasterize(r) => r.apply(px, w, h, space),
            PixelFx::Sketch(k) => k.apply(px, w, h, space),
            PixelFx::Artistic(a) => a.apply(px, w, h, space),
        }
    }
}

/// `data` as `w` × `h` RGBA pixels, `None` when it isn't that size (or is empty).
fn pixels(data: &mut [u8], w: usize, h: usize) -> Option<&mut [[u8; 4]]> {
    let (px, rest) = data.as_chunks_mut::<4>();
    (w > 0 && h > 0 && rest.is_empty() && w.checked_mul(h) == Some(px.len())).then_some(px)
}

/// The largest box radius the blurs use: wider boxes look the same on any raster we make.
const MAX_BOX: usize = 1 << 17;

/// Radii of three box blurs approximating a Gaussian of `sigma` pixels.
pub fn gauss_boxes(sigma: f64) -> [usize; 3] {
    let sigma = if sigma.is_finite() { sigma.clamp(0.0, MAX_BOX as f64 / 2.0) } else { 0.0 };
    let n = 3.0;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f64 - 4.0 * n * wl as f64 - 3.0 * n) / (-4.0 * wl as f64 - 4.0);
    let m = m_ideal.round() as i64;
    let r = |i: i64| ((((if i < m { wl } else { wu }) - 1) / 2).max(0) as usize).min(MAX_BOX);
    [r(0), r(1), r(2)]
}

/// One horizontal box blur of radius `r` over the rows of `w` values of a plane (running sum,
/// zero past the ends).
fn box_plane(src: &[f32], dst: &mut [f32], w: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let norm = 1.0 / (2 * r + 1) as f32;
    for (row, out) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        let mut acc: f32 = row.iter().take(r + 1).sum();
        for (x, o) in out.iter_mut().enumerate() {
            *o = acc * norm;
            if let Some(v) = row.get(x + r + 1) {
                acc += v;
            }
            if let Some(v) = x.checked_sub(r).and_then(|i| row.get(i)) {
                acc -= v;
            }
        }
    }
}

fn transpose<T: Copy>(src: &[T], dst: &mut [T], w: usize, h: usize) {
    for (y, row) in src.chunks_exact(w).enumerate().take(h) {
        for (x, v) in row.iter().enumerate() {
            if let Some(d) = dst.get_mut(x * h + y) {
                *d = *v;
            }
        }
    }
}

/// Gaussian-blur a `w` × `h` plane of values in place (three box passes per axis, zero past the
/// edges). Does nothing when the sizes don't match.
pub fn blur_plane(a: &mut Vec<f32>, w: usize, h: usize, sigma: f64) {
    if sigma < 0.2 || w == 0 || h == 0 || w.checked_mul(h) != Some(a.len()) {
        return;
    }
    let boxes = gauss_boxes(sigma);
    let mut tmp = vec![0.0; a.len()];
    for (len, other) in [(w, h), (h, w)] {
        for r in boxes {
            box_plane(a, &mut tmp, len, r);
            std::mem::swap(a, &mut tmp);
        }
        transpose(a, &mut tmp, len, other);
        std::mem::swap(a, &mut tmp);
    }
}

/// Premultiplied RGBA at 16 bits (×257): blurs and averages that keep their tones.
pub(super) type Px16 = [u16; 4];

pub(super) fn to16(px: &[[u8; 4]]) -> Vec<Px16> {
    px.iter().map(|p| p.map(|v| v as u16 * 257)).collect()
}

pub(super) fn to8(v: u16) -> u8 {
    ((v as u32 + 128) / 257).min(255) as u8
}

fn add(acc: &mut [u64; 4], p: &Px16) {
    for (a, v) in acc.iter_mut().zip(p) {
        *a += *v as u64;
    }
}

fn sub(acc: &mut [u64; 4], p: &Px16) {
    for (a, v) in acc.iter_mut().zip(p) {
        *a = a.saturating_sub(*v as u64);
    }
}

fn mean(acc: &[u64; 4], n: u64) -> Px16 {
    acc.map(|a| ((a + n / 2) / n.max(1)).min(65535) as u16)
}

/// One box blur of radius `r` along the rows of `w` pixels (transparent past the ends).
fn box_rows(src: &[Px16], dst: &mut [Px16], w: usize, r: usize) {
    let n = 2 * r as u64 + 1;
    for (row, out) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        let mut acc = [0u64; 4];
        row.iter().take(r + 1).for_each(|p| add(&mut acc, p));
        for (x, o) in out.iter_mut().enumerate() {
            *o = mean(&acc, n);
            if let Some(p) = row.get(x + r + 1) {
                add(&mut acc, p);
            }
            if let Some(p) = x.checked_sub(r).and_then(|i| row.get(i)) {
                sub(&mut acc, p);
            }
        }
    }
}

/// One box blur of radius `r` down the columns of `w`-wide rows (transparent past the ends), row
/// by row with a running sum per column.
fn box_cols(src: &[Px16], dst: &mut [Px16], w: usize, r: usize) {
    let n = 2 * r as u64 + 1;
    let rows: Vec<&[Px16]> = src.chunks_exact(w).collect();
    let mut acc = vec![[0u64; 4]; w];
    let add_row = |acc: &mut [[u64; 4]], row: &[Px16]| acc.iter_mut().zip(row).for_each(|(a, p)| add(a, p));
    rows.iter().take(r + 1).for_each(|row| add_row(&mut acc, row));
    for (y, out) in dst.chunks_exact_mut(w).enumerate() {
        for (o, a) in out.iter_mut().zip(&acc) {
            *o = mean(a, n);
        }
        if let Some(row) = rows.get(y + r + 1) {
            add_row(&mut acc, row);
        }
        if let Some(row) = y.checked_sub(r).and_then(|i| rows.get(i)) {
            acc.iter_mut().zip(*row).for_each(|(a, p)| sub(a, p));
        }
    }
}

/// Box-blur `w`-wide rows of pixels in place with radius `r` along both axes.
pub(super) fn box_blur(px: &mut [Px16], w: usize, r: usize) {
    if r == 0 || w == 0 {
        return;
    }
    let mut tmp = vec![[0u16; 4]; px.len()];
    box_rows(px, &mut tmp, w, r);
    box_cols(&tmp, px, w, r);
}

/// Gaussian-blur `w`-wide rows of premultiplied pixels in place (σ = `sigma` pixels, three box
/// passes per axis, transparent past the edges).
pub(super) fn gaussian(px: &mut [Px16], w: usize, sigma: f64) {
    if sigma < 0.2 || w == 0 {
        return;
    }
    let mut tmp = vec![[0u16; 4]; px.len()];
    for r in gauss_boxes(sigma) {
        box_rows(px, &mut tmp, w, r);
        px.copy_from_slice(&tmp);
    }
    for r in gauss_boxes(sigma) {
        box_cols(px, &mut tmp, w, r);
        px.copy_from_slice(&tmp);
    }
}

/// Gaussian-blur `data`, `w` × `h` premultiplied RGBA8 pixels, in place (σ = `sigma` pixels). Does
/// nothing when the sizes don't match.
pub fn gaussian_rgba(data: &mut [u8], w: usize, h: usize, sigma: f64) {
    if let Some(px) = pixels(data, w, h)
        && sigma >= 0.2
    {
        let mut wide = to16(px);
        gaussian(&mut wide, w, sigma);
        for (p, q) in px.iter_mut().zip(&wide) {
            *p = q.map(to8);
        }
    }
}

#[cfg(test)]
mod tests;
