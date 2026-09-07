# M2 acceptance: one real VoiceOver session

This is the last item in M2 and the only one nobody but you can run. Everything
up to it is built and tested; the session itself needs a person, a screen
reader and an accessibility grant this machine's automation does not have.

Budget about 20 minutes. Record the result at the bottom of this file,
including anything that failed. A session reported as a pass when it was not is
worse than no session.

## Why a person has to do this

The automated probe (`crates/app/tests/a11y_probe.rs`) sends the same
`NSAccessibility` messages VoiceOver sends, straight to the window's view. It
needs no permission because it never leaves the process, and that is exactly
its limit. It proves:

- AccessKit's `SubclassingAdapter` attaches to gpui's own view
  (`AccessKitSubclassOfGPUIView`), with no fork changes;
- the pinned fork still defines no accessibility selectors of its own, so the
  runtime subclass does not collide with it;
- the platform serves the tree the shell built, with its roles, names, states,
  identifiers and page text.

It proves nothing past the view. The AX server, cross-process marshalling,
notification delivery and speech are all outside it. In particular, **nothing
automated can tell you what a user hears**. That is this session.

## Before you start

1. Build the app:

   ```
   cd <repo>
   CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build -p onionskin-app --features shell
   ```

2. Grant accessibility permission if macOS asks: System Settings > Privacy &
   Security > Accessibility.

3. Learn the four keys, if they are not already muscle memory:

   | Keys | What it does |
   |------|--------------|
   | Cmd-F5 | VoiceOver on and off |
   | Ctrl-Option-Right / Left | Next / previous element |
   | Ctrl-Option-Shift-Down | Move into a group |
   | Ctrl-Option-Space | Press what the cursor is on |
   | Ctrl-Option-F1 F1 | Choose another application |
   | Ctrl-Option-A | Read continuously from here |

   Ctrl-Option is "VO" in Apple's documentation.

4. Turn on the caption panel so you can read what was spoken:
   VoiceOver Utility > Visuals > Caption Panel > Show caption panel. It makes
   this session much easier to report honestly.

## The session

Run the app on a seed with real text:

```
./target/debug/onionskin corpus/seeds/hello.pdf
```

Turn VoiceOver on with Cmd-F5.

### Step 1: the window is there and is not silent

Click the Onionskin window, or Cmd-Tab to it.

- **Pass** sounds like: "Onionskin, window", then the first element as you
  start moving.
- **Failure** sounds like: silence, or "unknown".

The window itself does not announce the open document, and that is expected:
AccessKit deliberately drops the label of the root node it exposes for a
top-level window, because a title there breaks VoiceOver's own handling
(accesskit_macos 0.26.3, `node.rs`). The document's name is on the document
node instead, which step 5 checks.

### Step 2: the chrome reads as controls, not as punctuation

Press Ctrl-Option-Right repeatedly and listen to each stop. Work along the top
bar, then down the left rail, then along the page controls at the bottom.

- **Pass** sounds like: "Main Menu, button", "Search Tools Or Document, search
  field", "First Page, button", "Previous Page, button, dimmed", "Zoom In,
  button", "Fit Width, button".
- **Failure** sounds like: "left single quotation mark", "vertical line, left
  single quotation mark", "black circle", any single punctuation character read
  as its Unicode name, or a stop that says only "button" with no name.

Write down every stop that failed. The names are all in one table per surface
in the source, so a missing one is a one-line fix.

### Step 3: state is state

Find the "Actual Size" button in the page controls (it draws as "1:1").

- **Pass** sounds like: "Actual Size, checkbox, checked" (or "unchecked"),
  and pressing Ctrl-Option-Space flips what it says.
- **Failure** sounds like: "1:1 check mark", or "Actual Size, button" with no
  checked state at all.

Then open a document with optional content, because the seeds have none:

```
./target/debug/onionskin corpus/external/hayro/pdfs/custom/issue175.pdf
```

That file has one page and seven named layers: Visible, Hidden, BORDER,
TITLEBLOCK, Smart Centers, FD_Dimensions and Hole Notes. It comes with the
default corpus fetch, so run `./corpus/fetch.sh` first if `corpus/external/`
is not on this machine.

Open the left navigation panes (the icon strip on the left) and select Layers.

- **Pass**: each of the seven reads by name, then ", checkbox, checked" or
  "unchecked", and Ctrl-Option-Space flips what it says.
- **Failure**: the tick is read as a character ("ballot box with check"), there
  is no state, or the pane reports no layers at all.

### Step 4: a control that is off says why

Still on page 1, reach "Previous Page".

- **Pass** sounds like: "Previous Page, button, dimmed", and VoiceOver's help
  (Ctrl-Option-Shift-H, or the caption panel) says "This is the first page".
- **Failure**: it reads as available, or it is missing from the tree entirely.

### Step 5: the document is a document, not a group

Keep pressing Ctrl-Option-Right until you reach the document.

- **Pass** sounds like: "hello.pdf, document." This is the fix M1's spike asked
  for: before it, the same node announced as "group".
- **Failure** sounds like: "hello.pdf, group", or the document is skipped.

### Step 6: the page reads

With the document selected, press Ctrl-Option-Shift-Down to move into it, then
Ctrl-Option-Right.

- **Pass** sounds like: "Page 1 of 1, page", then moving in again and pressing
  Ctrl-Option-A reads "Hello Onionskin".
- **Failure**: the page is empty, the text is read as one unbroken run you
  cannot navigate, or the words are wrong or out of order.

A page that is still being laid out announces "Page N is still loading" rather
than reading as an empty page. Hearing that once on a large document is fine;
hearing it and never getting the words is a failure.

Repeat on a longer document, for example one from `external/hayro-corpus/`, and
check that moving between pages announces the new page number.

### Step 7: the keyboard alone

Turn VoiceOver off (Cmd-F5) and drive the app with the keyboard only.

- Tab and Shift-Tab move between surfaces, not between controls: one press
  goes from the global bar to the tab strip, to the tool rail, to the pane
  strip, to an open pane, to the document, to the quick actions, to the find
  bar, to the page controls, to the side panel toggle, and wraps at both ends.
  Which of those are on screen depends on what you have open, and the ones
  that are not are skipped rather than being silent stops. Each surface is
  entered at its first control, so Shift-Tab undoes Tab.
- The arrow keys move inside the surface you are in, in both axes, and wrap
  there rather than leaving it. Down along the page controls reaches Zoom In;
  from the last control it comes back to the first. Right does the same until
  it reaches the page number field, where Right becomes that field's own key
  and moves the caret, so use Down to walk a row all the way.
- A surface whose first control is a text field, which the find bar is, is
  still one Tab to enter and one Tab to leave, and Up and Down carry you from
  the field to the buttons beside it. Left and Right in a field move the
  caret, because the field keeps those two for itself.
- **Failure**: Tab that steps one control at a time, so that crossing an open
  Page Thumbnails pane takes one press per page; an arrow that leaves the
  surface it started in; a surface Tab cannot reach at all.
- Open the Page Thumbnails pane and check this specifically: Tab reaches the
  pane, one more Tab leaves it whatever its length, and the arrows walk its
  rows.
- Enter or Space runs the focused control.
- Escape closes whatever is on top, in the order the window stacks them: a
  dialog, then the global search panel, then a context menu, then the main
  menu or the recents flyout, then the find bar. It closes one thing per
  press, and with none of them open it does nothing rather than swallowing
  the key.
- Typing in the find field still gets its own keys: Enter finds the next match
  rather than re-running the focused button, and Left and Right move the caret
  rather than the focus ring.

- **Failure**: focus that jumps somewhere unrelated, a Tab that does nothing, a
  control that focuses but does not run, or an Escape that closes the wrong
  thing.

Turn VoiceOver back on for the next steps.

### Step 7b: the reader's cursor and the keyboard agree

The screen reader's cursor and the keyboard's focus are one thing, and this is
where they used to come apart.

- Move VoiceOver's cursor onto a button in the page controls
  (Ctrl-Option-Right), then press Return without moving anything else.
  **Pass**: the button runs. **Failure**: nothing happens, or something else
  runs.
- Now click into the Find field, type a letter, and then move VoiceOver's
  cursor onto a button with Ctrl-Option-Right and press Return.
  **Pass**: the button runs. **Failure**: the letter goes into the find field,
  or the find bar acts on Return, which means the field kept the keyboard
  after the reader moved off it.
- The other direction: click into the Find field with the mouse.
  **Pass**: VoiceOver announces the find field, because the reader's cursor
  followed the keyboard into it. **Failure**: VoiceOver stays on whatever it
  was reading.

### Step 8: the window that is not in front

Open a second application over Onionskin and interact with it.

- **Pass**: VoiceOver reads the other application, and Onionskin does not
  announce anything.
- **Failure**: Onionskin keeps claiming focus. That would mean the adapter is
  being told the view is focused when the window is not key, which is the
  specific thing M1's spike could not test.

### Step 9: operating the window that is not in front

The window has to be **fully** covered for this step to test anything: macOS
only clears `NSWindowOcclusionStateVisible` when an opaque window covers the
Onionskin window completely, and while any sliver of it shows, gpui keeps
drawing frames and the old frame-driven code would pass this step too. Put the
other application full screen on the same display as Onionskin, or size and
position it so that no part of the Onionskin window is visible.

Remember that the document opened fitted to the page, so it is not at 100 per
cent.

Move VoiceOver into Onionskin without bringing it forward: press
Ctrl-Option-F1 twice for the application chooser and pick Onionskin, or use
Ctrl-Option-F2 twice for the window chooser. Navigate to "Actual Size" and
press Ctrl-Option-Space.

- **Pass**: VoiceOver announces the control as checked straight away, and the
  document is at 100 per cent when you bring the window forward.
- **Failure**: nothing is announced until you bring the window forward, and
  then the press takes effect all at once. That is the defect this step
  exists for: gpui draws a window only while macOS reports it visible, so a
  press that waited for a frame waited for the user.

This is the step no automated test on this machine can stand in for. The probe
and the shell tests both drive a window the test itself owns.

## What to record

Copy this in and fill it out.

```
## Result

Date:
macOS version:
Onionskin build (git rev):

Step 1 the window is there and not silent: pass / fail
Step 2 chrome reads as named controls:     pass / fail
Step 3 state is state:                     pass / fail
Step 4 disabled controls say why:          pass / fail
Step 5 the document is a document:         pass / fail
Step 6 the page text reads:                pass / fail
Step 7 keyboard only:                      pass / fail
Step 7b cursor and keyboard agree:          pass / fail
Step 8 background window stays quiet:      pass / fail
Step 9 background window can be operated:   pass / fail

What failed, exactly as it sounded:

What surprised you:
```

M2 is not done until every step above passes, or until a failure is recorded
here with an issue against it.
