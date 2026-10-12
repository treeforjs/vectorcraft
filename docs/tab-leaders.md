# Tab leaders

Select a text object, open **Window → Type → Tabs**, select a stop, and enter up to eight
characters in **Leader**. Enter or leaving the field commits the pattern in one undo step;
Escape discards the draft. Spaces are part of the pattern: `. ` produces dots separated by a
space. An empty pattern leaves the tab gap unpainted.

The repeated marks use the tab character's font, size, fill, stroke, tracking, baseline shift,
and character scaling. Left, center, right, and decimal stops retain their existing text
alignment. Point, area, vertical, and path type carry leader geometry through their respective
layout transforms. Paragraph-scoped tab settings remain paragraph-scoped.

Leaders are paint associated with the source tab, so they do not add characters, caret stops, or
repeated punctuation to copied text. Native files preserve the editable pattern and paragraph
settings. Create Outlines includes the painted leaders. PDF and EPS include their vector paths;
ordinary live SVG text keeps its text plus separate leader paths. SVG path type with leaders
uses outlines and reports that conversion to preserve the curved placement. Interchange formats
do not preserve the editable VectorCraft tab-stop settings.

The automation command is:

```json
{"command":"text.tabs.set","params":{"stops":[{"position":180,"align":"decimal","alignOn":".","leader":". "}]}}
```

Positions are points. The command rejects control characters and patterns longer than eight
Unicode characters atomically. Native input also has bounded leader shaping and repetition:
one text layout can paint at most 65,536 leader glyphs and two million outline elements. If the
budget is exceeded, the layout reports overflow. Zero-width and control-only patterns do not
start a repetition loop.

This work adds leader paint and persistent editing fields. Broader tab composition behavior,
including tab-aware line breaking and paragraph justification, remains separate work; the
leader renderer follows the tab gap produced by the text layout.

Behavior reference: [public tab-stop documentation](https://helpx.adobe.com/illustrator/desktop/measure-and-align/grids-and-guides/manage-tab-stops.html).
The implementation and test documents are original; no Adobe application was run or used as
an output oracle.

Regression coverage is in `text/src/tests_leaders.rs`, the Tabs panel's real pointer/keyboard
frame tests, `engine/tests/tab_leaders.rs`, `svg/tests/text_layout.rs`, and the EPS exporter tests.
The SVG placement oracle uses an independent SVG text/layout parser with the repository's
licensed font assets.
