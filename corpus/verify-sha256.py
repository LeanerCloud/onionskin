#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import os
import re
import stat
import sys
from pathlib import Path, PureWindowsPath

MANIFEST_LINE_RE = re.compile(r"^([0-9a-f]{64})  (.+)$")
CHUNK_SIZE = 1024 * 1024


def usage() -> int:
    print("usage: verify-sha256.py MANIFEST ROOT", file=sys.stderr)
    return 2


def fail(path: Path | str, reason: str) -> str:
    return f"{path}: {reason}"


def path_error(manifest: Path, line_number: int, reason: str) -> str:
    return f"{manifest}:{line_number}: {reason}"


def safe_name(name: str) -> str | None:
    if name != name.strip():
        return "path has leading or trailing whitespace"
    if not name:
        return "empty path not allowed"

    windows = PureWindowsPath(name)
    if name.startswith(("/", "\\")) or windows.is_absolute():
        return "absolute path not allowed"
    if windows.drive:
        return "drive-qualified path not allowed"
    if ":" in name:
        return "colon in path not allowed"
    if "\\" in name:
        return "backslash path separators are not allowed"

    parts = name.split("/")
    if any(part in {".", ".."} for part in parts):
        return "path traversal not allowed"
    if any(not part for part in parts):
        return "empty path component is not allowed"
    if len(parts) != 1:
        return "nested path not allowed"
    if Path(name).suffix.lower() != ".pdf":
        return "manifest entry is not a PDF"
    return None


def is_reparse_point(info: os.stat_result) -> bool:
    attributes = getattr(info, "st_file_attributes", 0)
    reparse = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    return bool(reparse and attributes & reparse)


def open_regular_file(path: Path):
    try:
        before = path.lstat()
    except FileNotFoundError:
        return None, "missing file"
    except OSError as error:
        return None, f"cannot stat file: {error}"

    if stat.S_ISLNK(before.st_mode):
        return None, "symlink not allowed"
    if is_reparse_point(before):
        return None, "reparse point not allowed"
    if not stat.S_ISREG(before.st_mode):
        return None, "not a regular file"

    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        return None, f"cannot open file: {error}"

    try:
        opened = os.fstat(descriptor)
        after = path.lstat()
        if is_reparse_point(opened) or is_reparse_point(after):
            os.close(descriptor)
            return None, "reparse point not allowed"
        if not stat.S_ISREG(opened.st_mode) or not stat.S_ISREG(after.st_mode):
            os.close(descriptor)
            return None, "not a regular file"
        if not os.path.samestat(before, opened) or not os.path.samestat(opened, after):
            os.close(descriptor)
            return None, "file changed during verification"
        return os.fdopen(descriptor, "rb"), None
    except OSError as error:
        os.close(descriptor)
        return None, f"cannot verify file identity: {error}"


def read_manifest(manifest: Path) -> tuple[dict[str, str], list[str]]:
    expected: dict[str, str] = {}
    canonical_names: dict[str, str] = {}
    errors: list[str] = []
    names: list[str] = []

    handle, reason = open_regular_file(manifest)
    if reason:
        return {}, [fail(manifest, reason)]
    try:
        with handle:
            contents = handle.read().decode("utf-8")
    except UnicodeDecodeError as error:
        return {}, [fail(manifest, f"manifest is not UTF-8: {error}")]
    except OSError as error:
        return {}, [fail(manifest, f"cannot read manifest: {error}")]

    for line_number, line in enumerate(contents.splitlines(), start=1):
        match = MANIFEST_LINE_RE.fullmatch(line)
        if match is None:
            errors.append(path_error(manifest, line_number, "malformed line"))
            continue

        digest, name = match.groups()

        if reason := safe_name(name):
            errors.append(path_error(manifest, line_number, f"{name}: {reason}"))
            continue

        canonical = name.casefold()
        if canonical in canonical_names:
            previous = canonical_names[canonical]
            errors.append(
                path_error(
                    manifest,
                    line_number,
                    f"{name}: duplicate name (aliases {previous})",
                )
            )
            continue

        canonical_names[canonical] = name
        expected[name] = digest
        names.append(name)

    if names != sorted(names):
        errors.append(fail(manifest, "manifest entries are not sorted by name"))
    if not names and not errors:
        errors.append(fail(manifest, "manifest has no entries"))

    return expected, errors


def root_pdfs(root: Path) -> tuple[set[str], list[str]]:
    found: set[str] = set()
    errors: list[str] = []

    try:
        entries = sorted(root.iterdir(), key=lambda path: path.name)
    except OSError as error:
        return found, [fail(root, f"cannot list root directory: {error}")]

    for path in entries:
        try:
            info = path.lstat()
        except OSError as error:
            errors.append(fail(path, f"cannot stat path: {error}"))
            continue
        if stat.S_ISLNK(info.st_mode):
            errors.append(fail(path, "symlink not allowed"))
        elif is_reparse_point(info):
            errors.append(fail(path, "reparse point not allowed"))
        elif stat.S_ISDIR(info.st_mode):
            errors.append(fail(path, "unexpected directory"))
        elif not stat.S_ISREG(info.st_mode):
            errors.append(fail(path, "not a regular file"))
        elif path.suffix.lower() == ".pdf":
            found.add(path.name)

    return found, errors


def sha256_file(path: Path) -> tuple[str | None, str | None]:
    handle, reason = open_regular_file(path)
    if reason:
        return None, reason

    digest = hashlib.sha256()
    try:
        with handle:
            for chunk in iter(lambda: handle.read(CHUNK_SIZE), b""):
                digest.update(chunk)
    except OSError as error:
        return None, f"cannot read file: {error}"
    return digest.hexdigest(), None


def verify(manifest: Path, root: Path) -> tuple[list[str], int]:
    errors: list[str] = []

    try:
        root_info = root.lstat()
    except FileNotFoundError:
        errors.append(fail(root, "missing root directory"))
    except OSError as error:
        errors.append(fail(root, f"cannot stat root directory: {error}"))
    else:
        if stat.S_ISLNK(root_info.st_mode):
            errors.append(fail(root, "symlink not allowed"))
        elif is_reparse_point(root_info):
            errors.append(fail(root, "reparse point not allowed"))
        elif not stat.S_ISDIR(root_info.st_mode):
            errors.append(fail(root, "not a directory"))

    expected, manifest_errors = read_manifest(manifest)
    errors.extend(manifest_errors)
    if errors:
        return errors, len(expected)

    actual_pdfs, walk_errors = root_pdfs(root)
    errors.extend(walk_errors)
    expected_names = set(expected)
    for name in sorted(actual_pdfs - expected_names):
        errors.append(fail(root / name, "unexpected PDF"))

    for name, expected_digest in expected.items():
        path = root / name
        actual_digest, reason = sha256_file(path)
        if reason:
            errors.append(fail(path, reason))
            continue
        if actual_digest != expected_digest:
            errors.append(
                fail(
                    path,
                    f"sha256 mismatch: expected {expected_digest}, got {actual_digest}",
                )
            )

    return errors, len(expected)


def main() -> int:
    if len(sys.argv) != 3:
        return usage()

    manifest = Path(sys.argv[1])
    root = Path(sys.argv[2])
    errors, count = verify(manifest, root)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print(f"verified {count} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
