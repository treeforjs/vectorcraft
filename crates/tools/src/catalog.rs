//! The tool catalogue: ids, labels, shortcuts and toolbar groups (Illustrator's Advanced toolbar order).

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ToolInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// Single-key shortcut (Shift+key shown as "Shift+X").
    pub shortcut: Option<&'static str>,
    /// Icon name in the UI icon set.
    pub icon: &'static str,
    /// One plain-language line on what the tool does (and how to use it), for its tooltip.
    pub about: &'static str,
}

const fn t(id: &'static str, label: &'static str, shortcut: Option<&'static str>, icon: &'static str, about: &'static str) -> ToolInfo {
    ToolInfo { id, label, shortcut, icon, about }
}

/// Toolbar groups in order; the first tool of each group is the default visible one.
pub const TOOL_GROUPS: &[&[ToolInfo]] = &[
    &[t("selection", "Selection Tool", Some("V"), "tool-selection", "Pick, move and resize whole objects.")],
    &[
        t("directSelection", "Direct Selection Tool", Some("A"), "tool-direct", "Pick single points and path segments to reshape them."),
        t("groupSelection", "Group Selection Tool", None, "tool-group-select", "Pick one object inside a group, then the group around it."),
    ],
    &[t("magicWand", "Magic Wand Tool", Some("Y"), "tool-magic-wand", "Select every object with a similar color or stroke.")],
    &[t("lasso", "Lasso Tool", Some("Q"), "tool-lasso", "Draw around points or objects to select them.")],
    &[
        t("pen", "Pen Tool", Some("P"), "tool-pen", "Draw precise curves: click for corners, drag for curves."),
        t("addAnchor", "Add Anchor Point Tool", Some("+"), "tool-pen-add", "Click a path to add a point to it."),
        t("deleteAnchor", "Delete Anchor Point Tool", Some("-"), "tool-pen-delete", "Click a point on a path to remove it."),
        t("anchorPoint", "Anchor Point Tool", Some("Shift+C"), "tool-anchor", "Turn a point into a sharp corner or a smooth curve."),
    ],
    &[t("curvature", "Curvature Tool", Some("Shift+~"), "tool-curvature", "Draw smooth curves by clicking where the path should pass.")],
    &[
        t("type", "Type Tool", Some("T"), "tool-type", "Add text: click for a line, drag for a text box."),
        t("areaType", "Area Type Tool", None, "tool-type-area", "Fill a shape with flowing text."),
        t("typeOnPath", "Type on a Path Tool", None, "tool-type-path", "Make text follow a line or curve."),
        t("verticalType", "Vertical Type Tool", None, "tool-type-vertical", "Add text that runs top to bottom."),
        t("verticalAreaType", "Vertical Area Type Tool", None, "tool-type-vertical", "Fill a shape with text that runs top to bottom."),
        t("verticalTypeOnPath", "Vertical Type on a Path Tool", None, "tool-type-path", "Make top-to-bottom text follow a path."),
        t("touchType", "Touch Type Tool", Some("Shift+T"), "tool-touch-type", "Move, turn and resize single letters."),
    ],
    &[
        t("lineSegment", "Line Segment Tool", Some("\\"), "tool-line", "Draw straight lines."),
        t("arc", "Arc Tool", None, "tool-arc", "Draw curved arcs."),
        t("spiral", "Spiral Tool", None, "tool-spiral", "Draw spirals."),
        t("rectangularGrid", "Rectangular Grid Tool", None, "tool-rect-grid", "Draw a grid of rows and columns."),
        t("polarGrid", "Polar Grid Tool", None, "tool-polar-grid", "Draw a target of circles and spokes."),
    ],
    &[
        t("rectangle", "Rectangle Tool", Some("M"), "tool-rect", "Draw rectangles and squares."),
        t("roundedRectangle", "Rounded Rectangle Tool", None, "tool-rounded-rect", "Draw rectangles with rounded corners."),
        t("ellipse", "Ellipse Tool", Some("L"), "tool-ellipse", "Draw ovals and circles."),
        t("polygon", "Polygon Tool", None, "tool-polygon", "Draw shapes with equal sides, like triangles and hexagons."),
        t("star", "Star Tool", None, "tool-star", "Draw stars."),
        t("flare", "Flare Tool", None, "tool-flare", "Draw a lens flare with a glow, rings and rays."),
    ],
    &[
        t("paintbrush", "Paintbrush Tool", Some("B"), "tool-brush", "Paint freehand strokes with a brush."),
        t("blobBrush", "Blob Brush Tool", Some("Shift+B"), "tool-blob-brush", "Paint filled shapes that merge where they overlap."),
    ],
    &[
        t("shaper", "Shaper Tool", Some("Shift+N"), "tool-shaper", "Scribble a rough shape and get a clean one."),
        t("pencil", "Pencil Tool", Some("N"), "tool-pencil", "Draw freehand lines that smooth themselves."),
        t("smooth", "Smooth Tool", None, "tool-smooth", "Drag along a path to smooth out bumps."),
        t("pathEraser", "Path Eraser Tool", None, "tool-path-eraser", "Drag along a path to erase part of it."),
        t("join", "Join Tool", None, "tool-join", "Scribble over two open ends to join them."),
    ],
    &[
        t("eraser", "Eraser Tool", Some("Shift+E"), "tool-eraser", "Rub out parts of shapes."),
        t("scissors", "Scissors Tool", Some("C"), "tool-scissors", "Cut a path in two at the point you click."),
        t("knife", "Knife", None, "tool-knife", "Slice shapes apart along a freehand line."),
        t("mirrorCut", "Mirror & Cut Tool", None, "tool-mirror-cut", "Cut an object along a line and mirror one half."),
        t("lineCut", "Line Cut Tool", None, "tool-line-cut", "Cut objects along a straight line."),
        t("rectCut", "Rectangle Cut Tool", None, "tool-rect-cut", "Cut a rectangle out of objects."),
    ],
    &[
        t("rotate", "Rotate Tool", Some("R"), "tool-rotate", "Turn objects around a point."),
        t("reflect", "Reflect Tool", Some("O"), "tool-reflect", "Flip objects like a mirror."),
    ],
    &[
        t("scale", "Scale Tool", Some("S"), "tool-scale", "Make objects bigger or smaller."),
        t("shear", "Shear Tool", None, "tool-shear", "Slant objects sideways."),
        t("reshape", "Reshape Tool", None, "tool-reshape", "Drag part of a path to reshape it smoothly."),
    ],
    &[
        t("width", "Width Tool", Some("Shift+W"), "tool-width", "Make a stroke thicker or thinner at points along it."),
        t("warp", "Warp Tool", Some("Shift+R"), "tool-warp", "Push paths around like soft clay."),
        t("twirl", "Twirl Tool", None, "tool-twirl", "Swirl paths into twists."),
        t("pucker", "Pucker Tool", None, "tool-pucker", "Pull paths in toward the brush."),
        t("bloat", "Bloat Tool", None, "tool-bloat", "Puff paths out from the brush."),
        t("scallop", "Scallop Tool", None, "tool-scallop", "Add curved bumps along outlines."),
        t("crystallize", "Crystallize Tool", None, "tool-crystallize", "Add spikes along outlines."),
        t("wrinkle", "Wrinkle Tool", None, "tool-wrinkle", "Add wrinkles along outlines."),
    ],
    &[
        t("freeTransform", "Free Transform Tool", Some("E"), "tool-free-transform", "Move, scale, rotate, slant and distort in one tool."),
        t("puppetWarp", "Puppet Warp Tool", None, "tool-puppet", "Pin points on art and drag them to bend it."),
    ],
    &[
        t("shapeBuilder", "Shape Builder Tool", Some("Shift+M"), "tool-shape-builder", "Merge or remove overlapping parts of shapes."),
        t("livePaintBucket", "Live Paint Bucket", Some("K"), "tool-bucket", "Fill the areas between overlapping lines with color."),
        t("livePaintSelection", "Live Paint Selection Tool", Some("Shift+L"), "tool-live-select", "Select the areas and edges of Live Paint art."),
    ],
    &[
        t("perspectiveGrid", "Perspective Grid Tool", Some("Shift+P"), "tool-perspective", "Show a grid for drawing in 3D perspective."),
        t(
            "perspectiveSelection",
            "Perspective Selection Tool",
            Some("Shift+V"),
            "tool-perspective-select",
            "Move and place objects on the perspective grid.",
        ),
    ],
    &[t("mesh", "Mesh Tool", Some("U"), "tool-mesh", "Add color points inside a shape for soft color blends.")],
    &[t("gradient", "Gradient Tool", Some("G"), "tool-gradient", "Set where a color gradient starts and ends.")],
    &[
        t("eyedropper", "Eyedropper Tool", Some("I"), "tool-eyedropper", "Copy the color and style of one object onto another."),
        t("measure", "Measure Tool", None, "tool-measure", "Measure distances and angles."),
    ],
    &[t("blend", "Blend Tool", Some("W"), "tool-blend", "Make the in-between steps from one object to another.")],
    &[
        t("symbolSprayer", "Symbol Sprayer Tool", Some("Shift+S"), "tool-symbol", "Spray copies of a symbol onto the page."),
        t("symbolShifter", "Symbol Shifter Tool", None, "tool-symbol", "Push sprayed symbols around."),
        t("symbolScruncher", "Symbol Scruncher Tool", None, "tool-symbol", "Pull sprayed symbols together or spread them out."),
        t("symbolSizer", "Symbol Sizer Tool", None, "tool-symbol", "Make sprayed symbols bigger or smaller."),
        t("symbolSpinner", "Symbol Spinner Tool", None, "tool-symbol", "Turn sprayed symbols."),
        t("symbolStainer", "Symbol Stainer Tool", None, "tool-symbol", "Tint sprayed symbols with the fill color."),
        t("symbolScreener", "Symbol Screener Tool", None, "tool-symbol", "Make sprayed symbols more or less see-through."),
        t("symbolStyler", "Symbol Styler Tool", None, "tool-symbol", "Apply the selected graphic style to sprayed symbols."),
    ],
    &[
        t("columnGraph", "Column Graph Tool", Some("J"), "tool-graph", "Make a chart with vertical bars."),
        t("stackedColumnGraph", "Stacked Column Graph Tool", None, "tool-graph", "Make a chart with stacked vertical bars."),
        t("barGraph", "Bar Graph Tool", None, "tool-graph", "Make a chart with horizontal bars."),
        t("stackedBarGraph", "Stacked Bar Graph Tool", None, "tool-graph", "Make a chart with stacked horizontal bars."),
        t("lineGraph", "Line Graph Tool", None, "tool-graph", "Make a chart with lines through points."),
        t("areaGraph", "Area Graph Tool", None, "tool-graph", "Make a chart with filled areas."),
        t("scatterGraph", "Scatter Graph Tool", None, "tool-graph", "Make a chart of dots."),
        t("pieGraph", "Pie Graph Tool", None, "tool-graph", "Make a pie chart."),
        t("radarGraph", "Radar Graph Tool", None, "tool-graph", "Make a radar (spider) chart."),
    ],
    &[
        t("artboard", "Artboard Tool", Some("Shift+O"), "tool-artboard", "Add, move and resize pages."),
        t("slice", "Slice Tool", Some("Shift+K"), "tool-slice", "Mark parts of the art to export as separate images."),
        t("sliceSelection", "Slice Selection Tool", None, "tool-slice", "Pick and adjust slices."),
    ],
    &[
        t("hand", "Hand Tool", Some("H"), "tool-hand", "Drag to move around the page. Hold Space with any tool."),
        t("rotateView", "Rotate View Tool", Some("Shift+H"), "tool-rotate-view", "Turn the view without turning the art."),
        t("printTiling", "Print Tiling Tool", None, "tool-print-tiling", "Move where the printed pages fall on the art."),
    ],
    &[t("zoom", "Zoom Tool", Some("Z"), "tool-zoom", "Zoom in and out of the page.")],
];

pub fn all_tools() -> impl Iterator<Item = &'static ToolInfo> {
    TOOL_GROUPS.iter().flat_map(|g| g.iter())
}

pub fn tool_info(id: &str) -> Option<&'static ToolInfo> {
    all_tools().find(|t| t.id == id)
}

/// Tool for a single-key shortcut such as "V" or "Shift+M".
pub fn tool_for_shortcut(s: &str) -> Option<&'static ToolInfo> {
    all_tools().find(|t| t.shortcut == Some(s))
}

/// Index of the group containing `id`.
pub fn group_of(id: &str) -> Option<usize> {
    TOOL_GROUPS.iter().position(|g| g.iter().any(|t| t.id == id))
}

/// Selection, Direct Selection and Group Selection: the tools Cmd held lends the others for a drag.
pub fn is_selection_tool(id: &str) -> bool {
    matches!(id, "selection" | "directSelection" | "groupSelection")
}

/// The tools that show a selected path's anchors and handles as Direct Selection does, and drag
/// them: a Cmd drag with one of them is a Direct Selection drag until a selection tool is chosen.
pub fn edits_anchors(id: &str) -> bool {
    matches!(id, "directSelection" | "pen" | "addAnchor" | "deleteAnchor" | "anchorPoint" | "curvature")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_ids_and_shortcuts() {
        let mut ids: Vec<&str> = all_tools().map(|t| t.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate tool ids");
        let mut sc: Vec<&str> = all_tools().filter_map(|t| t.shortcut).collect();
        let n = sc.len();
        sc.sort();
        sc.dedup();
        assert_eq!(sc.len(), n, "duplicate shortcuts");
        assert!(n > 40);
    }

    /// Every tool says in a plain sentence what it does (its tooltip's second line).
    #[test]
    fn every_tool_has_an_about_line() {
        for t in all_tools() {
            assert!(!t.about.trim().is_empty(), "{}", t.id);
            assert!(t.about.ends_with('.') && t.about.len() < 80, "{}: {:?}", t.id, t.about);
            assert!(!t.about.contains(" Tool"), "{}: plain words, not tool names: {:?}", t.id, t.about);
        }
        assert_eq!(all_tools().count(), 90);
    }

    #[test]
    fn lookups() {
        assert_eq!(tool_for_shortcut("V").unwrap().id, "selection");
        assert_eq!(tool_for_shortcut("Shift+M").unwrap().id, "shapeBuilder");
        assert_eq!(group_of("star"), group_of("rectangle"));
        assert!(is_selection_tool("groupSelection") && !is_selection_tool("pen"));
        assert!(edits_anchors("pen") && !edits_anchors("rectangle"));
    }
}
