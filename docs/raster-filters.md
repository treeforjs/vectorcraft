# Raster filter contracts

The live effects below are available through **Effect → Pixelate**, the Effect Gallery,
`effect.apply`, `effect.setParams` and `effect.setStack`. Native files retain editable parameters.
PNG rendering, SVG/PDF export and Expand Appearance use the same raster-filter pipeline.

These are original algorithms based on the qualitative behavior in the
[public filter reference](https://helpx.adobe.com/photoshop/using/filter-effects-reference.html)
and [public Illustrator effects documentation](https://helpx.adobe.com/illustrator/using/effects.html).
No Adobe application, output, installed files, assets or code were used. Those documents do not
specify exact kernels or transfer functions, so matching names is not evidence of pixel parity.

| Effect ID | Parameters | Implemented behavior |
| --- | --- | --- |
| `blur.smart` | `radius` 0.1–100 pt, `threshold` 0.1–100 levels, `quality` low/medium/high, `mode` normal/edgeOnly/overlayEdge | Normal retains the existing edge-preserving blur. Edge Only draws white color transitions over black; Overlay Edge draws those white transitions over the original colors. Missing/unknown mode defaults to Normal for older files. |
| `pixelate.mosaic` | `cellSize` 2–200 pt (10) | Square cells receive their alpha-weighted mean color. |
| `pixelate.colorHalftone` | `radius` 2–127 pt (8), `angle1` (108°), `angle2` (162°), `angle3` (90°) | Independent red, green and blue dot screens. Angles wrap through 360°. Cell period is √2 times the maximum dot radius. Dot area follows channel darkness, including full black and white endpoints. |
| `pixelate.crystallize` | `cellSize` 3–300 pt (10), `seed` (0) | Jittered Voronoi polygons take their alpha-weighted mean colors. Nine candidate sites suffice because each site stays in its cell's central half. |
| `pixelate.pointillize` | `cellSize` 3–300 pt (10), `seed` (0), `backgroundColor` (white) | Dots at the same deterministic jittered sites use local mean colors. Spaces between dots use the explicit background color. |
| `pixelate.fragment` | `offset` 0.1–100 pt (2) | Averages four copies displaced by ±offset on both document axes, including their alpha. The painted bounds expand to include the copies. |
| `pixelate.mezzotint` | `type` (fineDots), `grainSize` 0.1–20 pt (1), `seed` (0) | Deterministic threshold grain produces binary RGB channels, preserving original alpha. Equal channels stay neutral; colored input can produce saturated colors. |
| `pixelate.facet` | `cellSize` 2–100 pt (3), `levels` 2–64 (8) | Groups colors by an RGB partition inside each small cell, then assigns each group its alpha-weighted mean. Dissimilar groups stay separate. |

Distances are document points and participate in Scale Strokes & Effects. Cell grids are anchored
to the unfiltered content's center in document coordinates, so panning or cropping does not
restart the pattern. Viewport filtering includes each cell's full source footprint. Seeds are
stored, making random-looking patterns repeatable after reopening and export. Crops and resolution
changes are checked with original synthetic fixtures.

Cell aggregation caps its initial map at 262,144 keys. Sparse rasters use 256×256 output tiles
with only the tile's needed keys and an unchanged source snapshot. Strongly skewed, singular,
or extreme grids use sorted sample keys when tile halos would scan too much of the source.
These paths preserve the same averages; they never reduce image resolution. The sorted fallback
uses at most 32 bytes per covered sample for current keys, in addition to the caller's raster.
Facet normally processes each cell independently with at most 64³ color groups. Regression tests
compare capped and sorted paths with source-only averages across affine transforms.

Pixelate filters other than Fragment, and Smart Blur's two edge modes, preserve original alpha coverage. Fragment averages the four copies' coverage along with their premultiplied colors. Cell color
averages divide summed premultiplied color by summed alpha; fully transparent pixels never contribute hidden
colors or black fringes. Pointillize's background therefore fills spaces inside the artwork,
not the surrounding transparent canvas. Smart Blur's edge modes compare covered straight colors
and ignore transparent neighbors; they do not outline the artwork's transparent silhouette.
Its quality controls the bounded sample grid (5, 7 or 9 samples across), and threshold decides
which color differences count as transitions. These choices are explicit implementation contracts,
not claims about another application's treatment of alpha.

Mezzotint types select grain dimensions in multiples of `grainSize`: Fine Dots 1×1,
Medium Dots 2×2, Grainy Dots 1.5×2.5, Coarse Dots 4×4; Short/Medium/Long Strokes 2×5,
2×10 and 2×20; Short/Medium/Long Lines 10×1, 20×1 and 40×1. These dimensions and the
Facet color partition are our explicit algorithms, not externally measured preset values.
Fragment's offset and Facet's cell size/color levels are extra editable controls.

Color Halftone is currently an RGB effect. Four independently controlled CMYK screens and
proofing of spot-color separations remain open; output color conversion still happens in the
existing export/print pipeline. The other missing raster-filter families and further fidelity work remain on the roadmap. Geometry effects still run before raster effects; the Gallery
does not change that composition rule.

Regression coverage checks malformed/extreme parameters, deterministic output, premultiplication,
crop/translation/resolution stability, seed and screen-angle changes, native parameters, undo/redo,
SVG/PDF appearance and Expand Appearance. GPU hardware and equivalence against commercial
implementations are not asserted by these tests.
