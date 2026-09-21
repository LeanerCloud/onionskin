# M3: typing a comment where it is, and double-click to finish a shape

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain, GPUI's test platform. No macOS, Windows or hosted-CI run is
claimed.

This came out of a hand test. The text tools placed an empty comment and
there was nowhere to type into it, and Polygon, Connected Lines and Cloud
needed Enter to finish, which nothing on screen suggested. Acrobat's help
for its commenting tools was the reference for what the user expects:

- the Sticky Note tool opens a pop-up note beside the icon, with the cursor
  in it;
- Add Text Comment and Text Box put the cursor in the text on the page;
- Insert Text and Replace Text open the pop-up note for the inserted text;
- a polygon, connected lines or a cloud ends with a double-click on the
  last point, or a click back on the first.

## What the user gets

- **Sticky Note, Insert Text, Replace Text:** after the click, a field opens
  beside the comment with the cursor in it.
- **Add Text Comment, Text Box, Callout:** the field opens over the box.
- **Finishing the text:** Enter or Escape, or a click anywhere else on the
  page. The text becomes the comment's text as one undoable step ("Edit
  Comment Text"). An empty field writes nothing, so the comment stays as
  placed and Undo takes it away.
- **Free text redraws:** a text box shows the typed text in its own font
  and size. The appearance is drawn again from the `/DA` the comment
  already has.
- **Shapes:** a double-click on the last corner finishes a polygon,
  connected lines or a cloud. Enter still works, and Escape abandons it.
- **Hints:** each tool's hint says how to finish it.

## How it is built

- **`PointerInput::clicks`** (plugin API) carries the platform's click count,
  so a tool can tell a double-click from two clicks. The vertex tools finish
  on `clicks >= 2`.
- **`Tool::takes_text`** marks the tools that want text. It defaults to
  false. When one of them commits, the canvas finds the comment it placed by
  comparing the page's annotations before and after, and waits for text for
  that comment (`TextTarget`).
- **`shell/inline_text.rs`** is the field. Its key context binds Enter and
  Escape more specifically than the shell's, so Enter finishes the comment
  instead of activating the focus ring.
- **`core::annots::review::set_contents`** writes `/Contents` and `/M`. For
  a `/FreeText` it also rebuilds the model from the dictionary
  (`parse_default_appearance` reads `/DA` back) and writes a new `/AP`.

## Runs

- `tools-comment/tests/shapes.rs::a_double_click_ends_a_vertex_shape_at_the_last_point`.
- `tabs/tests/outline.rs::placing_a_text_comment_opens_a_field_and_the_typed_text_is_its_text`
  drives a Sticky Note and a Text Box through the window:
  - click;
  - type;
  - Enter;
  - check that the text is the comment's `/Contents`.
- `core/tests/review.rs`: a text box given "Hello there" draws
  `(Hello there) Tj` in the Courier 10 it was placed with.
- `review::tests::a_default_appearance_reads_back_as_the_style_that_wrote_it`.

## Not done here, and said

- **The field is one line.** Line breaks in a text box come with the
  Comments pane's editor (P20).
- **Callout field placement:** the field for a callout covers the
  annotation's whole rectangle, leader included, not just the box.
