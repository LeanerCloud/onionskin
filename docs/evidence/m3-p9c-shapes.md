# M3 P9c verification: the seven shape tools

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P6, P7 and P9a.

Linux x86-64, stable toolchain. No macOS, Windows or hosted-CI run is claimed.

## Runs

- `cargo test -p onionskin-tools-comment`: 37 tests - 4 unit (`quads`) and 33
  integration across `markup`, `notes` and `shapes` - all passing.
- `cargo test -p onionskin-core --test annots`: 10 passing, with `Polygon` and
  `PolyLine` added to the subtype sweep.
- `cargo test -p onionskin-app`: all suites passing, including P7's contract
  suite over the real `build_registry()`, which now runs every one of the 16
  comment tools through its gesture lifecycle.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
  `cargo fmt --all -- --check`: clean.
- `cargo test -p onionskin-app --features shell --lib`: 440 pass, 6 fail. Same
  environmental set as P9a, see below.

## P9a's seam held: this package touches no shell file

The check the plan asks for by name - "whether this package reached back into
`canvas.rs` after all, which would mean P9a's seam was incomplete". It did not.
The seven tools use `Overlay::Line`, `Overlay::Rect`, `Overlay::Ellipse` and
`Overlay::Polyline { closed }` exactly as P9a left them, and the only files
here are `plugins/tools-comment/src/shapes.rs`, its `lib.rs` registration, and
the `core` model and appearance work the subtypes needed.

## The preview is the committed shape

`an_ovals_preview_is_the_ellipse_it_commits` drags a rectangle that is **not
square** - 240 by 80 - so a circle preview cannot match it by accident, and
compares the previewed `Overlay::Ellipse` bounds against the committed `/Rect`.
Under the mutation that previews an oval as a circle inscribed in the shorter
side, it fails with the two rectangles side by side:

```
left:  PageRect { x0: 180.0, y0: 620.0, x1: 260.0, y1: 700.0 }
right: PageRect { x0: 100.0, y0: 620.0, x1: 340.0, y1: 700.0 }
```

`a_polygons_preview_is_closed_and_a_polylines_is_not` makes the same claim for
the closing edge, over all three vertex tools, and asserts the vertex under the
pointer is part of what the user sees - a preview that only shows clicked
vertices lags the pointer by one click.

`an_ellipse_with_equal_axes_is_a_circle` is the one the plan asks for so that
removing `Overlay::Circle` demonstrably lost nothing.

## An arrow is a `/Line` with `/LE`

The review risk. Acrobat has no arrow subtype, and a `/Polygon` shaped like an
arrow renders in Acrobat as a line with no head. The test asserts the subtype,
the `/L` array and `/LE` as `[/None /ClosedArrow]` - the head on the end the
drag finished at, which is where the user was pointing.

**And the head is drawn into the appearance stream.** A reader synthesizes
endings from `/LE` only when there is no `/AP`; with one it draws the stream, so
a head living only in `/LE` is a line. The same reasoning put the callout's
leader in the stream in P9a and the cloud's scallops in it here - three
instances of one rule, which is why it is stated in `appearance.rs` rather than
at each call site.

## A cloud is a `/Polygon` with `/BE`, and it has to reach the page

Asserted three ways, in increasing strength: `/BE` `/S` is `/C` in the
dictionary; the appearance stream differs from a plain polygon's over the same
vertices and contains curve operators where the plain one has none; and the
**rendered page** differs from a plain polygon's by more than 200 pixels at
zoom 2. Compared against the plain polygon rather than against a pixel pattern,
because the claim is that the edge is not the straight one, not that the
scallops have a particular shape.

## One tool, one defaults struct, one commit path

The other review risk. There is one `ShapeTool`, one `Defaults`, one
`annotation()` that produces the shape and one `commit()` that writes it, and
the seven differ only in a `Shape` discriminant. Seven renderers would be seven
places for a default to drift.

The three vertex tools are built by clicking rather than dragging, which
`ToolPlugin` supports without a new hook: each click adds a vertex, Enter
(`on_commit`) finishes, and a click within 8 view pixels of the first vertex
closes the shape - which is how a polygon ends without a keyboard.

## Mutations run

| Mutation | Test that failed |
| --- | --- |
| `/BE` dropped from the annotation dictionary | `a_cloud_is_a_polygon_with_a_cloudy_border_effect` |
| The oval preview drawn as a circle | `an_ovals_preview_is_the_ellipse_it_commits`, with the two rectangles side by side |
| The cloud assuming one winding | `a_cloud_scallops_outward_whichever_way_its_vertices_were_clicked` |

## The scallops turned the wrong way, and a weak test said they did not

Which side of an edge is outward depends on the polygon's winding, and the user
picks that by the order of their clicks. The first version of this assumed one
winding; the second took it from the signed area but with the sign **backwards**
- in PDF user space, where y increases upward, the left of travel is the
*inside* of a counterclockwise polygon, and the lift is along the left.

The first test written for it counted non-background pixels and compared the
two windings. It passed under both the bug and its own mutation, because an
inward scallop covers about as many pixels as an outward one. What separates
them is **where** the ink is, so the test now maps the triangle's vertices into
the raster and counts ink landing outside it: over 300 pixels for each winding,
and 104 when the scallops turn in. That number is what found the sign error.

## The shell suite's failing set varies between runs

Recorded in `known-issues.md` and worth repeating: across four runs the shell
suite has failed five or six tests, but **not the same five or six**. That is
the strongest evidence available here that they are races against the geometry
and render workers rather than wrong expectations - and it means one green run
on macOS would not settle them either.

## Not done here

- **Cloud intensity is fixed at 1.** Acrobat exposes 0, 1 and 2 in its
  properties inspector, which is row 74 and belongs to P20; the value is a
  field on the defaults struct and reaches both `/BE` `/I` and the scallop
  size, so P20 changes one number.
- Rows 66-72 are closed in code; `ACROBAT-PARITY.md`'s totals stay **P15's**.
