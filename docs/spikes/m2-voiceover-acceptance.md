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

Then open the left navigation panes (the icon strip on the left) and select
Layers on a document that has them.

- **Pass**: each layer reads "…, checkbox, checked" or "unchecked".
- **Failure**: the tick is read as a character ("ballot box with check"), or
  there is no state.

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

- Tab and Shift-Tab move through the chrome in reading order and wrap at both
  ends.
- Enter or Space runs the focused control.
- Escape closes whatever is on top, in the order the window stacks them: a
  dialog, then the global search panel, then a context menu, then the main
  menu or the recents flyout, then the find bar. It closes one thing per
  press, and with none of them open it does nothing rather than swallowing
  the key.
- Typing in the find field still gets its own keys: Enter finds the next match
  rather than re-running the focused button.

- **Failure**: focus that jumps somewhere unrelated, a Tab that does nothing, a
  control that focuses but does not run, or an Escape that closes the wrong
  thing.

### Step 8: the window that is not in front

Open a second application over Onionskin and interact with it.

- **Pass**: VoiceOver reads the other application, and Onionskin does not
  announce anything.
- **Failure**: Onionskin keeps claiming focus. That would mean the adapter is
  being told the view is focused when the window is not key, which is the
  specific thing M1's spike could not test.

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
Step 8 background window stays quiet:      pass / fail

What failed, exactly as it sounded:

What surprised you:
```

M2 is not done until every step above passes, or until a failure is recorded
here with an issue against it.
