# Effect Gallery

The first Gallery workflow is available under **Effect → Effect Gallery**. Select artwork, add a
filter, edit its parameters, and use the stack to change order, toggle visibility, or delete a
filter. The preview shows the first selected object. **OK** keeps the whole session as one undo
step; **Cancel** restores the original. Turning Preview off temporarily restores the original
artwork while keeping the proposed edits in the dialog. OK still applies those edits.

The catalog offers the six filters already implemented: Gaussian Blur, Feather, Radial Blur,
Smart Blur, Unsharp Mask and Glowing Edges. This milestone adds the editing workflow; it does not
add filter algorithms or claim output equivalence with another application.

## Appearance targeting

With a fill or stroke active in the Appearance panel, the Gallery edits that item's effects.
Otherwise it edits the object's effects. Existing vector effects remain in the stack and retain
their parameters. Several selected objects can be edited together only when their targeted stacks
are identical. When they differ, select and edit them separately. Opening the Gallery and accepting
it without changes does not dirty the document or discard redo history.

The renderer processes geometry effects before raster effects. Content-changing raster filters
run in stack order; shadows and outer glows are layered separately. Moving a vector effect past a
raster effect therefore does not change that existing rendering rule. The Gallery does not change
the renderer's composition model.

## Automation

The same engine operation works headlessly, through the CLI and through MCP/control:

```json
{"command":"effect.setStack","params":{"ids":[12],"item":null,"effects":[{"id":"blur.radial","params":{"amount":30,"method":"spin","quality":"good"},"visible":true},{"id":"sharpen.unsharpMask","params":{"amount":150,"radius":2,"threshold":0},"visible":false}]}}
```

`ids` defaults to the selection. `item` is a fill/stroke's paint-order index; explicit `null` targets
the object. The complete list replaces only that target list. `effects: []` clears it. Unknown
filter IDs, invalid records, unavailable items, and lists exceeding 128 effects fail before any
object changes. Omitted parameters get filter defaults; supplied parameter objects are retained,
with omitted defaults resolved at render time. Unchanged lists do not create an undo entry.

Open the dialog with `engine.execute` command `effect.gallery`, params `{}` (or `{"item":0}`).
`ui.dialog.set` accepts `field: "effects"` with the same effect array, `field: "selected"` with an
index, and `field: "preview"` with a boolean. Use `ui.dialog.confirm` or `ui.dialog.cancel` to finish.
The dialog pins the opening document, selected object IDs and appearance item. Changing selection
from automation cannot redirect the edit. A document switch cancels its preview; return to the
original document to continue editing. Cancel never rolls back another document's interaction.

## Validation and remaining scope

Original synthetic tests cover actual pointer clicks on stack controls, live preview and rollback,
one-step undo/redo, unchanged/sparse stacks, multiple targets, appearance items, invalid input,
document switches, native save/reopen, and raster appearance through SVG/PDF and Expand Appearance.
No proprietary application, source code, bundled asset or generated test fixture is used. The workflow
reference is [public effects documentation](https://helpx.adobe.com/illustrator/using/effects.html).

The preview is fitted to one object and bounded to 300 pixels. Zoom/pan, comparison views, filter
thumbnails, mixed-stack editing and the remaining filter library are future work. Large-document
performance and physical GPU behavior require separate validation. Native files keep live stack
records; SVG/PDF use the existing vector/filter/raster export representations as appropriate.
