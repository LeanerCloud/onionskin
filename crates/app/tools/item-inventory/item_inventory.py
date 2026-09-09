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

  * `use` and `extern crate` items, and `mod name;` declarations. Every new
    file needs its own imports and the parent module has to declare it.
  * The file an item lives in. That is the whole point.

Visibility is carried in its own column rather than in the signature, because
moving an item to a sibling module forces a private item to widen to
`pub(super)` to keep the callers it already had. `compare` reports those
separately from everything else, so a reviewer reads the list and judges it
instead of the widening being either invisible or drowned in noise.

Usage:
    item_inventory.py emit FILE...            write the listing to stdout
    item_inventory.py compare BEFORE AFTER    diff two listings as multisets
"""

from __future__ import annotations

import hashlib
import sys
from collections import Counter

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
    out, i, n = [], 0, len(text)
    while i < n:
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


def canonical_signature(text):
    """A signature spelled the same however rustfmt chose to wrap it.

    Adding a visibility qualifier lengthens the first line, which can push
    rustfmt from a one-line signature to a wrapped one with a trailing comma.
    That is a layout difference, not a signature difference, so it is
    normalized away here rather than reported as a changed item.
    """
    out = []
    for c in text:
        if c == " ":
            if not out or out[-1] in "([<,":
                continue
        elif c in ")]>":
            while out and out[-1] in " ,":
                out.pop()
        elif c == ",":
            while out and out[-1] == " ":
                out.pop()
        out.append(c)
    return "".join(out)


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
        # its semicolon rather than through the generic header scanner.
        if peek_keyword(src, cls, i, end) == "use":
            i = scan_to_semicolon(src, cls, i, end) + 1
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
            body = (
                normalize(src, cls, terminator_at + 1, body_end, True) if terminator == "{" else ""
            )
            out.append(Item(container, kind, name, vis, signature, body))
            if kind in {"struct", "enum", "union"} and terminator == "{":
                member_kind = "variant" if kind == "enum" else "field"
                nested = "%s > %s" % (container, signature) if container else signature
                members = normalize(src, cls, terminator_at + 1, body_end, False)
                for member in split_top_level(members):
                    member_vis = visibility_of(member)
                    bare = strip_visibility(member)
                    member_name = bare.split(":")[0].split("(")[0].split("=")[0].strip()
                    out.append(Item(nested, member_kind, member_name, member_vis, bare, ""))
        i = next_i


def emit(paths):
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
            if item.kind in CONTAINER_KINDS:
                containers.add(item.line())
            else:
                lines.append(item.line())
    for line in sorted(lines + sorted(containers)):
        print(line)
    return 0


def compare(before_path, after_path):
    with open(before_path, encoding="utf-8") as handle:
        before = Counter(handle.read().splitlines())
    with open(after_path, encoding="utf-8") as handle:
        after = Counter(handle.read().splitlines())
    only_before = before - after
    only_after = after - before

    def key(line):
        parts = line.split("\t")
        return tuple(parts[:3] + parts[4:])

    before_by_key = Counter(key(line) for line in only_before.elements())
    after_by_key = Counter(key(line) for line in only_after.elements())
    visibility_only = before_by_key & after_by_key

    def vis_change(k):
        old = next(l for l in only_before.elements() if key(l) == k).split("\t")[3]
        new = next(l for l in only_after.elements() if key(l) == k).split("\t")[3]
        return "%s :: %s %s: %s -> %s" % (k[0], k[1], k[2], old, new)

    real_before = [l for l in sorted(only_before.elements()) if key(l) not in visibility_only]
    real_after = [l for l in sorted(only_after.elements()) if key(l) not in visibility_only]

    print("items before: %d" % sum(before.values()))
    print("items after:  %d" % sum(after.values()))
    print("visibility-only changes: %d" % sum(visibility_only.values()))
    for k in sorted(visibility_only):
        print("  VIS  %s" % vis_change(k))
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
    if len(argv) >= 3 and argv[1] == "emit":
        return emit(argv[2:])
    if len(argv) == 4 and argv[1] == "compare":
        return compare(argv[2], argv[3])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
