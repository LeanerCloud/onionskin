#!/usr/bin/env python3
"""Emit and compare an item inventory of Rust source files.

The inventory exists to prove that a file split relocated code without
changing it. It emits one line per item: the lexical container (which impl
block, which inline module), the kind, the name, the declared visibility, the
normalized signature, and a hash of the normalized body. Whitespace and
comments are normalized away, so a body that moved between files and was
re-indented hashes the same, while a changed body, a lost item or an invented
one shows up as a set difference.

The container carries the impl target and the trait path, so a method that
moves between `impl ShellFrame` and `impl Render for ShellFrame` is a
difference rather than a match.

Two things are deliberately excluded, because a split cannot preserve them:

  * plain `use` and `extern crate` items, and `mod name;` declarations. Every
    new file needs its own imports and the parent module has to declare it. A
    `pub use` is kept: a re-export grants reach rather than importing, so it is
    an item like any other.
  * The file an item lives in. That is the whole point.

Visibility is carried in its own column rather than in the signature, because
moving an item to a sibling module forces a private item to widen to
`pub(super)` to keep the callers it already had. `compare` reports those
separately from everything else, so a reviewer reads the list and judges it
instead of the widening being either invisible or drowned in noise. What
`pub(super)` reaches depends on the module that declares it, which the listing
deliberately does not record, so `audit` reports that separately.

A relocation leaves every body byte-identical, so the plain listing proves it.
Gathering loose fields into a sub-struct does not: every method that read
`self.foo` now reads `self.group.foo`, so every body hash moves and the
comparison degenerates into noise that says nothing. `--elide` is the rule that
keeps the listing exact across that restructuring:

    Given a group name `g`, and in the code of a body only: the whitespace
    beside every `.` is dropped, and then `.g.` is replaced by `.`, to a fixed
    point. `x.g.field` therefore hashes exactly as `x.field` did, however
    rustfmt chose to wrap the chain the hop lengthened.

It elides the hop and never the leaf, which is what stops it degenerating into
a normalization tuned to make a diff empty:

  * `self.g`, `&mut self.g` and `g: G { .. }` are left alone. The rule fires
    only between two dots, so passing the group whole, building it, or naming
    it as a field is a difference like any other.
  * the leaf keeps its own name in the hash, so a field renamed on the way into
    its group is a difference. A restructuring that renames nothing is what the
    rule is for; one that renames is reported.
  * string literals are left alone, so a changed literal that happens to spell
    a group hop cannot be normalized back into the original.
  * the names are given on the command line and recorded in the listing header,
    and `compare` refuses two listings that were elided differently. A diff
    cannot be emptied by eliding one side of it.

What the rule cannot absorb, by construction, is the declaration of the state
itself: the fields change container, the sub-structs are new items, and the
constructor that spells the literal out really is a changed body. Those are
reported, and are meant to be read rather than hashed away.

Usage:
    item_inventory.py emit [--at REV] [--elide A,B] FILE...
                                                write the listing to stdout
    item_inventory.py audit [--at REV] FILE...  non-private items and their modules
    item_inventory.py compare BEFORE AFTER      diff two listings as multisets

`--at` records the revision the files were read at, in a header `compare`
skips. Pass it: a listing without one cannot be placed later.
"""

from __future__ import annotations

import hashlib
import re
import sys
from collections import Counter
from pathlib import Path

CODE, COMMENT, STRING = 0, 1, 2

# Item kinds that own a body to recurse into rather than to hash.
CONTAINER_KINDS = {"impl", "trait", "mod"}
# Item kinds a split cannot preserve, so they are not part of the inventory.
SKIPPED_KINDS = {"use", "extern"}

KEYWORD_MODIFIERS = {"pub", "unsafe", "async", "default", "extern", "auto"}


def classify(src):
    """Tag every byte as code, comment or string literal."""
    n = len(src)
    cls = [CODE] * n
    i = 0
    while i < n:
        c = src[i]
        starts_token = i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j < 0 else j
            cls[i:j] = [COMMENT] * (j - i)
            i = j
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src[j] == "/" and j + 1 < n and src[j + 1] == "*":
                    depth += 1
                    j += 2
                elif src[j] == "*" and j + 1 < n and src[j + 1] == "/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            cls[i:j] = [COMMENT] * (j - i)
            i = j
        elif starts_token and c in "rb" and prefixed_literal_end(src, i) is not None:
            j = prefixed_literal_end(src, i)
            cls[i:j] = [STRING] * (j - i)
            i = j
        elif c == '"':
            j = plain_string_end(src, i)
            cls[i:j] = [STRING] * (j - i)
            i = j
        elif c == "'" and char_literal_end(src, i) is not None:
            j = char_literal_end(src, i)
            cls[i:j] = [STRING] * (j - i)
            i = j
        else:
            i += 1
    return cls


def prefixed_literal_end(src, i):
    """End of a raw, byte or byte-raw literal at `i`, or None if it is not one."""
    n, j = len(src), i
    if src[j] == "b":
        j += 1
        if j < n and src[j] == '"':
            return plain_string_end(src, j)
        if j < n and src[j] == "'":
            return char_literal_end(src, j)
    if j >= n or src[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < n and src[j] == "#":
        hashes += 1
        j += 1
    if j >= n or src[j] != '"':
        return None
    terminator = '"' + "#" * hashes
    end = src.find(terminator, j + 1)
    if end < 0:
        raise SyntaxError("unterminated raw string at offset %d" % i)
    return end + len(terminator)


def plain_string_end(src, i):
    n, j = len(src), i + 1
    while j < n:
        if src[j] == "\\":
            j += 2
            continue
        if src[j] == '"':
            return j + 1
        j += 1
    raise SyntaxError("unterminated string at offset %d" % i)


def char_literal_end(src, i):
    """End of a char literal at `i`, or None when the quote opens a lifetime."""
    n = len(src)
    if i + 1 < n and src[i + 1] == "\\":
        j = i + 2
        while j < n and src[j] != "'":
            j += 1
        return j + 1
    if i + 2 < n and src[i + 2] == "'":
        return i + 3
    return None


def skip_trivia(src, cls, i, end):
    while i < end and (cls[i] != CODE or src[i].isspace()):
        i += 1
    return i


def match_brace(src, cls, i):
    depth = 0
    j = i
    while j < len(src):
        if cls[j] == CODE:
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    return j
        j += 1
    raise SyntaxError("unbalanced brace at offset %d" % i)


def consume_attribute(src, cls, i):
    """End of the `#[...]` or `#![...]` attribute starting at `i`."""
    j = i + 1
    if j < len(src) and src[j] == "!":
        j += 1
    if j >= len(src) or src[j] != "[":
        raise SyntaxError("malformed attribute at offset %d" % i)
    depth = 0
    while j < len(src):
        if cls[j] == CODE:
            if src[j] == "[":
                depth += 1
            elif src[j] == "]":
                depth -= 1
                if depth == 0:
                    return j + 1
        j += 1
    raise SyntaxError("unterminated attribute at offset %d" % i)


def scan_header(src, cls, i, end):
    """Index and identity of the `{` or `;` that ends the item header at `i`."""
    depth = 0
    j = i
    while j < end:
        if cls[j] == CODE:
            c = src[j]
            if c in "([":
                depth += 1
            elif c in ")]":
                depth -= 1
            elif depth == 0 and c in "{;":
                return j, c
        j += 1
    raise SyntaxError("unterminated item header at offset %d" % i)


def scan_to_semicolon(src, cls, i, end):
    """Index of the `;` ending a `use` or `extern crate`, braces and all."""
    depth = 0
    j = i
    while j < end:
        if cls[j] == CODE:
            c = src[j]
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif depth == 0 and c == ";":
                return j
        j += 1
    raise SyntaxError("unterminated import at offset %d" % i)


def peek_keyword(src, cls, i, end):
    """The item keyword of the header at `i`, past any visibility qualifier."""
    for _ in range(2):
        j = i
        while j < end and cls[j] == CODE and is_ident_char(src[j]):
            j += 1
        word = src[i:j]
        if word != "pub":
            return word
        if j < end and src[j] == "(":
            depth = 0
            while j < end:
                if src[j] == "(":
                    depth += 1
                elif src[j] == ")":
                    depth -= 1
                    if depth == 0:
                        j += 1
                        break
                j += 1
        i = skip_trivia(src, cls, j, end)
    return ""


def normalize(src, cls, start, end, drop_vis):
    """Text with comments dropped and every whitespace run collapsed to a space."""
    kept = [src[k] if cls[k] != COMMENT else " " for k in range(start, end)]
    text = " ".join("".join(kept).split())
    return strip_visibility(text) if drop_vis else text


def strip_visibility(text):
    """Remove `pub` and `pub(...)` qualifiers, leaving string literals alone.

    Visibility is reported in its own column, so it is taken out of signatures
    and out of the bodies of the items that declare fields. A `pub` inside a
    string literal is data, not a qualifier, and removing it would let a
    changed literal hash the same as the original.
    """
    out, i, n = [], 0, len(text)
    in_string = False
    while i < n:
        if in_string:
            if text[i] == "\\":
                out.append(text[i])
                i += 1
                if i < n:
                    out.append(text[i])
                    i += 1
                continue
            if text[i] == '"':
                in_string = False
            out.append(text[i])
            i += 1
            continue
        if text[i] == '"':
            in_string = True
            out.append(text[i])
            i += 1
            continue
        if text.startswith("pub", i) and (i == 0 or not is_ident_char(text[i - 1])):
            j = i + 3
            if j < n and text[j] == "(":
                depth = 0
                while j < n:
                    if text[j] == "(":
                        depth += 1
                    elif text[j] == ")":
                        depth -= 1
                        if depth == 0:
                            j += 1
                            break
                    j += 1
            if j >= n or not is_ident_char(text[j]):
                i = j
                while i < n and text[i] == " ":
                    i += 1
                continue
        out.append(text[i])
        i += 1
    return "".join(out)


def is_ident_char(c):
    return c.isalnum() or c == "_"


def code_spans(text):
    """Split normalized text into (is_code, span) runs, strings being the gaps.

    A string literal is data. Rewriting inside one would let a changed literal
    hash the same as the original, which is the failure the body hash exists to
    prevent, so every code rewrite is applied to the code runs only. Raw and
    byte literals open on the same `"` and close on the first unescaped one,
    which is what `strip_visibility` already assumes.
    """
    spans, start, i, n = [], 0, 0, len(text)
    in_string = False
    while i < n:
        if in_string:
            if text[i] == "\\":
                i += 2
                continue
            if text[i] == '"':
                spans.append((False, text[start : i + 1]))
                start, in_string = i + 1, False
            i += 1
            continue
        if text[i] == '"':
            spans.append((True, text[start:i]))
            start, in_string = i, True
        i += 1
    spans.append((not in_string, text[start:]))
    return spans


DOT = re.compile(r"\s*\.\s*")


def elide_groups(text, names):
    """Delete a group hop from every field-access chain, in code only.

    Two steps, both confined to code:

      * the whitespace beside every `.` goes. Inserting a hop lengthens the
        chain, rustfmt rewraps it, and `.field` that sat inline ends up on a
        line of its own. That is layout, not a change. Whitespace is already
        not part of a body here; this extends the existing collapse-to-one-
        space to collapse-to-nothing beside a dot, and only under `--elide`,
        so a listing emitted without it keeps exactly the meaning it had.
      * `.g.` becomes `.`, so `x.g.field` hashes as `x.field` did. Applied to
        a fixed point, so a chain that gained two hops loses both.

    The hop's own dot is the one that goes; the leaf keeps its name and its
    dot, which is what makes a renamed leaf a difference rather than a match.
    """
    if not names:
        return text
    pattern = re.compile(r"\.(?:%s)\." % "|".join(re.escape(n) for n in sorted(names)))
    out = []
    for is_code, span in code_spans(text):
        if is_code:
            span = DOT.sub(".", span)
            previous = None
            while previous != span:
                previous = span
                span = pattern.sub(".", span)
        out.append(span)
    return "".join(out)


def canonical_signature(text):
    """A signature spelled the same however rustfmt chose to wrap it.

    Adding a visibility qualifier lengthens the first line, which can push
    rustfmt from a one-line signature to a wrapped one with a trailing comma.
    That is a layout difference, not a signature difference, so it is
    normalized away here rather than reported as a changed item.

    The trailing comma is only dropped inside a parameter list, never inside a
    type. `(T,)` is a one-tuple and `(T)` is a `T`, and on an item with no body
    (a trait method declaration) the signature is the only thing distinguishing
    them, so collapsing the two would hide a real change.
    """
    out = []
    # One entry per open bracket: whether it opened a parameter list. A `(`
    # opens one when it follows the name it belongs to, `fn foo(` or
    # `fn foo<T>(`. A `(` that follows `:`, `->`, `,` or another `(` opens a
    # type, where a trailing comma is the one-tuple and has to survive.
    stack = []
    for c in text:
        if c == " ":
            if not out or out[-1] in "([<,":
                continue
        elif c == "(":
            stack.append(opens_parameter_list(out))
        elif c in "[<":
            stack.append(False)
        elif c in ")]>":
            while out and out[-1] == " ":
                out.pop()
            parameter_list = stack.pop() if stack else False
            if parameter_list and out and out[-1] == ",":
                out.pop()
        elif c == ",":
            while out and out[-1] == " ":
                out.pop()
        out.append(c)
    return "".join(out)


def opens_parameter_list(out):
    if not out:
        return False
    if is_ident_char(out[-1]):
        return True
    # `fn foo<T>(` ends in `>`, but `-> (T,)` ends in the `>` of an arrow.
    return out[-1] == ">" and len(out) >= 2 and out[-2] != "-"


def leading_identifier(word):
    end = 0
    while end < len(word) and is_ident_char(word[end]):
        end += 1
    return word[:end]


def visibility_of(header):
    header = header.lstrip()
    if not header.startswith("pub"):
        return "private"
    rest = header[3:]
    if rest.startswith("("):
        depth, j = 0, 0
        while j < len(rest):
            if rest[j] == "(":
                depth += 1
            elif rest[j] == ")":
                depth -= 1
                if depth == 0:
                    return " ".join(("pub" + rest[: j + 1]).split())
            j += 1
    return "pub"


def kind_and_name(header):
    """The item's keyword and the identifier it declares."""
    words = strip_visibility(header).replace("!", " ! ").split()
    idx = 0
    while idx < len(words) and words[idx] in KEYWORD_MODIFIERS:
        if words[idx] == "extern" and idx + 1 < len(words) and words[idx + 1].startswith('"'):
            idx += 1
        idx += 1
    if idx >= len(words):
        return "unknown", ""
    kind = leading_identifier(words[idx])
    if kind == "const" and idx + 1 < len(words) and words[idx + 1] in {"fn", "unsafe"}:
        idx += 1
        kind = words[idx]
    if kind in CONTAINER_KINDS or kind in SKIPPED_KINDS:
        return kind, ""
    name = words[idx + 1] if idx + 1 < len(words) else ""
    for separator in "<(:=":
        name = name.split(separator)[0]
    return kind, name


def reexported_name(header):
    """The tail of a `pub use` path, which is the name it binds."""
    path = strip_visibility(header)[len("use") :].strip().rstrip(";").strip()
    return path.rsplit("::", 1)[-1].strip() or path


def without_leading_visibility(header):
    """The header past its own `pub(...)`, keeping any inside it.

    `pub(super) struct ExportPhase(pub AtomicU8)` has two: the struct's, which
    belongs in the visibility column, and the field's, which has to survive to
    be reported as the field's own.
    """
    stripped = header.lstrip()
    if not stripped.startswith("pub"):
        return header
    rest = stripped[3:]
    if not rest.startswith("("):
        return rest.lstrip()
    depth = 0
    for index, c in enumerate(rest):
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return rest[index + 1 :].lstrip()
    return rest


def tuple_fields(header):
    """The positional fields of a tuple struct header, or an empty list."""
    header = without_leading_visibility(header)
    depth, start = 0, None
    for index, c in enumerate(header):
        if c == "(":
            depth += 1
            if depth == 1:
                start = index + 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return split_top_level(header[start:index])
    return []


def split_top_level(text):
    """Split a struct or enum body on its top-level commas."""
    parts, depth, current = [], 0, []
    for c in text:
        if c in "([{<":
            depth += 1
        elif c in ")]}>":
            depth -= 1
        if c == "," and depth == 0:
            parts.append("".join(current))
            current = []
            continue
        current.append(c)
    parts.append("".join(current))
    return [p.strip() for p in parts if p.strip()]


class Item:
    __slots__ = ("container", "kind", "name", "vis", "signature", "body")

    def __init__(self, container, kind, name, vis, signature, body):
        self.container = container
        self.kind = kind
        self.name = name
        self.vis = vis
        self.signature = signature
        self.body = body

    def line(self):
        digest = hashlib.sha256(self.body.encode()).hexdigest()[:16]
        return "\t".join(
            [self.container or "-", self.kind, self.name or "-", self.vis, self.signature, digest]
        )


def parse(src, cls, start, end, container, out):
    i = start
    while True:
        i = skip_trivia(src, cls, i, end)
        if i >= end:
            return
        attrs = []
        while src[i] == "#":
            j = consume_attribute(src, cls, i)
            attrs.append(normalize(src, cls, i, j, False))
            i = skip_trivia(src, cls, j, end)
            if i >= end:
                return
        # `use gpui::{...};` puts a brace in the header, so it is scanned to
        # its semicolon rather than through the generic header scanner. A plain
        # `use` is dropped, because a split cannot preserve imports. A `pub use`
        # is kept: a re-export is an item that grants reach, not an import, and
        # a split that added one would otherwise go unreported.
        if peek_keyword(src, cls, i, end) == "use":
            semicolon = scan_to_semicolon(src, cls, i, end)
            header = normalize(src, cls, i, semicolon, False)
            if visibility_of(header) != "private":
                out.append(
                    Item(
                        container,
                        "use",
                        reexported_name(header),
                        visibility_of(header),
                        canonical_signature(strip_visibility(header)),
                        "",
                    )
                )
            i = semicolon + 1
            continue
        header_start = i
        terminator_at, terminator = scan_header(src, cls, i, end)
        header = normalize(src, cls, header_start, terminator_at, False)
        kind, name = kind_and_name(header)
        vis = visibility_of(header)
        signature = canonical_signature(" ".join(attrs + [strip_visibility(header)]).strip())

        if terminator == "{":
            body_end = match_brace(src, cls, terminator_at)
            next_i = body_end + 1
        else:
            body_end = terminator_at
            next_i = terminator_at + 1

        if kind in SKIPPED_KINDS or (kind == "mod" and terminator == ";"):
            i = next_i
            continue

        if kind in CONTAINER_KINDS and terminator == "{":
            out.append(Item(container, kind, name, vis, signature, ""))
            nested = "%s > %s" % (container, signature) if container else signature
            parse(src, cls, terminator_at + 1, body_end, nested, out)
        else:
            declares_members = kind in {"struct", "enum", "union"}
            # Visibility is stripped only from the bodies that declare members,
            # where it is reported per member instead. A function body has no
            # visibility to strip, and stripping it there would only risk
            # rewriting a string literal that happens to contain the word.
            body = (
                normalize(src, cls, terminator_at + 1, body_end, declares_members)
                if terminator == "{"
                else ""
            )
            out.append(Item(container, kind, name, vis, signature, body))
            nested = "%s > %s" % (container, signature) if container else signature
            if declares_members and terminator != "{":
                # `struct ExportPhase(AtomicU8);`: the fields are positional and
                # live in the header, so the brace walk below never sees them.
                for position, member in enumerate(tuple_fields(header)):
                    out.append(
                        Item(
                            nested,
                            "field",
                            str(position),
                            visibility_of(member),
                            canonical_signature(strip_visibility(member)),
                            "",
                        )
                    )
            if declares_members and terminator == "{":
                member_kind = "variant" if kind == "enum" else "field"
                members = normalize(src, cls, terminator_at + 1, body_end, False)
                for member in split_top_level(members):
                    member_vis = visibility_of(member)
                    bare = strip_visibility(member)
                    member_name = bare.split(":")[0].split("(")[0].split("=")[0].strip()
                    out.append(Item(nested, member_kind, member_name, member_vis, bare, ""))
        i = next_i


def provenance(at, paths, lines=None, elide=()):
    """The header that records what a listing was emitted from, and of what.

    A listing is 400-odd anonymous lines. Six months later nobody can tell
    which revision it describes, and the commit message that said so is not
    where anyone looks. `compare` tells the header from an item by the field
    count, so the header costs nothing.

    The census is there because the item count is the denominator any claim
    about the listing is read against, and a reader who counts by hand will
    count something else. Note in particular that a trait's default methods are
    items under that trait, not under any `impl`, and that `impl` and `mod`
    headers are deduplicated across files.
    """
    header = [
        "# emitted at %s" % (at or "an unrecorded revision"),
        "# from %s" % " ".join(str(path) for path in paths),
    ]
    # Always emitted, empty set included. A listing that is silent about how it
    # was normalized cannot be compared against one that is not, and `compare`
    # needs the two to say the same thing to know they are comparable.
    header.append("# elided %s" % (" ".join(sorted(elide)) or "nothing"))
    if lines is not None:
        census = Counter(line.split("\t")[1] for line in lines)
        header.append(
            "# items: %d = %s"
            % (
                len(lines),
                ", ".join("%d %s" % (count, kind) for kind, count in sorted(census.items())),
            )
        )
    return header


def emit(paths, at=None, elide=()):
    lines = []
    # A single `impl` block legitimately becomes several when its methods move
    # to different files, so container lines are a set: what has to survive is
    # that the block still exists and still holds the same methods, not how
    # many `impl ShellFrame {` headers spell it.
    containers = set()
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            src = handle.read()
        cls = classify(src)
        items = []
        parse(src, cls, 0, len(src), "", items)
        for item in items:
            item.body = elide_groups(item.body, elide)
            if item.kind in CONTAINER_KINDS:
                containers.add(item.line())
            else:
                lines.append(item.line())
    listing_lines = sorted(lines + sorted(containers))
    for line in provenance(at, paths, listing_lines, elide):
        print(line)
    for line in listing_lines:
        print(line)
    return 0


def module_path(path):
    """The Rust module path a source file declares, from its location."""
    parts = list(Path(path).with_suffix("").parts)
    if "src" in parts:
        parts = parts[parts.index("src") + 1 :]
    if parts and parts[-1] in {"mod", "lib", "main"}:
        parts.pop()
    return "::".join(["crate"] + parts)


def audit(paths, at=None):
    """Every item that is not private, with the module that declares it.

    `pub(super)` names a different scope in every module, so the listing's
    visibility column cannot be judged without knowing where the item sits. The
    module is deliberately not part of the inventory, because a split changes
    it by design, so it is reported here instead.
    """
    for line in provenance(at, paths):
        print(line)
    print("\t".join(["module", "container", "kind", "name", "visibility"]))
    rows = []
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            src = handle.read()
        items = []
        parse(src, classify(src), 0, len(src), "", items)
        module = module_path(path)
        rows.extend(
            "\t".join([module, item.container or "-", item.kind, item.name or "-", item.vis])
            for item in items
            if item.vis != "private"
        )
    for row in sorted(rows):
        print(row)
    return 0


FIELDS = 6


def listing(path):
    """A listing's item lines and its elision set, dropping the rest of the header.

    Item lines are told apart by the field count, not by a `#` prefix: an
    item's container is a signature and a signature carries its attributes, so
    a perfectly ordinary item line can start with `#[derive(...)]`. Anything
    that is neither an item nor a header is an error rather than something to
    skip quietly. A listing written before `--elide` existed has no such header
    and elided nothing.
    """
    items = Counter()
    elided = ()
    with open(path, encoding="utf-8") as handle:
        for line in handle.read().splitlines():
            if line.count("\t") == FIELDS - 1:
                items[line] += 1
            elif line.startswith("# elided "):
                names = line[len("# elided ") :].split()
                elided = () if names == ["nothing"] else tuple(sorted(names))
            elif not line.startswith("#"):
                raise SystemExit("%s: neither an item nor a header: %r" % (path, line))
    return items, elided


def compare(before_path, after_path):
    before, before_elided = listing(before_path)
    after, after_elided = listing(after_path)
    # Eliding one side and not the other would rewrite half the bodies and
    # report the rewrite as agreement. The two listings have to have been
    # normalized the same way for their difference to mean anything.
    if before_elided != after_elided:
        raise SystemExit(
            "listings were elided differently, so they are not comparable: %s vs %s"
            % (" ".join(before_elided) or "nothing", " ".join(after_elided) or "nothing")
        )
    print("elided: %s" % (" ".join(before_elided) or "nothing"))
    only_before = before - after
    only_after = after - before

    def key(line):
        parts = line.split("\t")
        return tuple(parts[:3] + parts[4:])

    before_by_key = Counter(key(line) for line in only_before.elements())
    after_by_key = Counter(key(line) for line in only_after.elements())
    # Paired off by count, not by membership. A key present twice on one side
    # and once on the other pairs once and leaves one line reported, where a
    # membership test would have discarded all three and called the sets equal.
    visibility_only = before_by_key & after_by_key

    remaining_before = Counter(only_before)
    remaining_after = Counter(only_after)
    changes = []
    for k, count in sorted(visibility_only.items()):
        old_lines = sorted(l for l in remaining_before.elements() if key(l) == k)
        new_lines = sorted(l for l in remaining_after.elements() if key(l) == k)
        for old, new in list(zip(old_lines, new_lines))[:count]:
            remaining_before[old] -= 1
            remaining_after[new] -= 1
            changes.append(
                "%s :: %s %s: %s -> %s"
                % (k[0], k[1], k[2], old.split("\t")[3], new.split("\t")[3])
            )

    real_before = sorted((+remaining_before).elements())
    real_after = sorted((+remaining_after).elements())

    print("items before: %d" % sum(before.values()))
    print("items after:  %d" % sum(after.values()))
    for name, counter in (("before", before), ("after", after)):
        duplicates = {k: n for k, n in Counter(key(l) for l in counter.elements()).items() if n > 1}
        if duplicates:
            print("duplicate keys in %s: %d" % (name, sum(duplicates.values())))
            for k in sorted(duplicates):
                print("  DUP  %s :: %s %s x%d" % (k[0], k[1], k[2], duplicates[k]))
    print("visibility-only changes: %d" % len(changes))
    for change in changes:
        print("  VIS  %s" % change)
    print("only in before: %d" % len(real_before))
    for line in real_before:
        print("  -    %s" % line)
    print("only in after:  %d" % len(real_after))
    for line in real_after:
        print("  +    %s" % line)
    equal = not real_before and not real_after
    print("RESULT: sets are equal" if equal else "RESULT: sets differ")
    return 0 if equal else 1


def main(argv):
    at = None
    elide = ()
    while len(argv) >= 4 and argv[2] in {"--at", "--elide"}:
        if argv[2] == "--at":
            at = argv[3]
        else:
            elide = tuple(sorted(name for name in argv[3].split(",") if name))
        argv = argv[:2] + argv[4:]
    if len(argv) >= 3 and argv[1] == "emit":
        return emit(argv[2:], at, elide)
    if len(argv) >= 3 and argv[1] == "audit":
        if elide:
            raise SystemExit("audit reports visibility, which --elide does not touch")
        return audit(argv[2:], at)
    if len(argv) == 4 and argv[1] == "compare":
        return compare(argv[2], argv[3])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
