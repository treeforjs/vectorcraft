# Ruler coordinates

The ruler mode and origins are native document view settings. They change position
readouts; they never translate artwork, guides, pattern fills or command geometry.
The public workflow reference is
<https://helpx.adobe.com/illustrator/using/rulers-grids-guides-crop-marks.html>.
Implementation and fixtures are original; no reference application was run.

## Coordinates and controls

- **View → Rulers → Change to Artboard Rulers / Change to Global Rulers** switches
  modes. The shortcut is Ctrl+Alt+R on Windows/Linux and Command+Option+R on macOS.
- Global mode defaults to the first artboard's upper-left corner. A custom global
  origin is an absolute document point.
- Artboard mode defaults to the active artboard's upper-left corner. A custom
  origin is stored as an offset under that artboard's stable ID, so it follows
  artboard moves and survives reordering. Each artboard keeps its own origin.
- Drag the rulers' corner onto the canvas to set the current origin. Release
  outside the canvas or press Escape to cancel. Double-click the corner to reset
  the current origin. The preview does not edit the document. A document, mode,
  active artboard or origin change during the drag cancels it.
- Transform, Properties and Control bar X/Y fields and Info cursor positions use
  the same origin. Widths, heights, relative movements and rotation pivots retain
  their original document geometry. Guide position labels and Shift snapping to
  ruler ticks follow the origin; pixel and grid snapping retain their existing
  document coordinate system.
- Hold Alt/Option while dragging a new guide from a ruler to exchange its
  orientation. Releasing the modifier restores the original orientation; the
  final guide remains a single Undo step. Existing-guide Alt copying is unchanged.
- Ruler labels follow the turned canvas. Positive document X is right and Y is
  down before view rotation. Changing units changes labels, not stored points.

Settings persist on explicit native save, including native templates and the
native document carried by AI files. Like camera position, a ruler change does
not mark artwork dirty or add an Undo entry. Closing without saving can therefore
discard an origin change without a save prompt. Older files with no settings use
global mode. SVG/PDF-only exports contain artwork, not these editor settings.

## Automation

The engine commands take document-space points and an optional zero-based
`artboard` index (default 0). The desktop command route supplies the active
artboard when omitted.

```json
{"command":"view.rulers.global","params":{"on":false,"artboard":1}}
{"command":"view.rulers.origin","params":{"point":[820,-70],"artboard":1}}
{"command":"view.rulers.origin","params":{"reset":true,"artboard":1}}
{"command":"view.rulers.info","params":{"artboard":1}}
```

`view.rulers.global` toggles when `on` is absent. An origin command with no point
or reset only queries. `view.rulers.info` reports mode, effective origin and stored
settings. `ui.inspect` also exposes these values. Existing geometry commands such
as `object.setBounds` continue to take document coordinates, independent of UI
ruler mode. Nonfinite/out-of-range origins, invalid indexes and incompatible
parameters fail before mutation.

## Acceptance and remaining scope

Source regressions cover independent document/artboard origins, reset, invalid
requests, stable IDs, native persistence, unchanged artwork/history, bounded tick
generation, guide tick labels, and real pointer corner drags. Test results and
installed Ubuntu screenshots are recorded separately for the exact built commit.
Their existence here does not assert a completed runtime test.

Video rulers remain unfinished. Global-origin-dependent pattern-fill behavior is
not implemented: both ruler modes intentionally preserve existing pattern fills.
Proprietary-output equivalence, physical Windows/GPU rendering and calibrated
hardware behavior remain unmeasured. This feature does not establish full
Illustrator parity.
