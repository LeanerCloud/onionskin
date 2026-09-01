#!/usr/bin/env python3
from __future__ import annotations

import os
import stat
import sys
from pathlib import Path

sys.dont_write_bytecode = True

import r2

def usage() -> int:
    print("usage: verify-sha256.py MANIFEST ROOT", file=sys.stderr)
    return 2


def fail(path: Path | str, reason: str) -> str:
    return f"{path}: {reason}"


def read_manifest(manifest: Path) -> tuple[dict[str, str], list[str]]:
    return r2.checksum_manifest(manifest)


def root_pdfs_from_windows(root: Path) -> tuple[set[str], list[str]]:
    found: set[str] = set()
    errors: list[str] = []

    try:
        entries = sorted(root.iterdir(), key=lambda path: path.name)
    except OSError as error:
        return found, [fail(root, f"cannot list root directory: {error}")]

    for path in entries:
        name = path.name
        try:
            info = path.lstat()
        except OSError as error:
            errors.append(fail(path, f"cannot stat path: {error}"))
            continue
        if r2.is_reparse_point(info):
            errors.append(fail(path, "reparse point not allowed"))
        elif stat.S_ISLNK(info.st_mode):
            errors.append(fail(path, "symlink not allowed"))
        elif stat.S_ISDIR(info.st_mode):
            errors.append(fail(path, "unexpected directory"))
        elif not stat.S_ISREG(info.st_mode):
            errors.append(fail(path, "not a regular file"))
        elif path.suffix.lower() == ".pdf":
            if reason := r2.safe_pdf_name(name):
                errors.append(fail(path, reason))
            else:
                found.add(name)

    return found, errors


def root_pdfs_from_fd(root: Path, root_fd: int) -> tuple[set[str], list[str]]:
    found: set[str] = set()
    errors: list[str] = []

    try:
        entries = sorted(os.listdir(root_fd))
    except OSError as error:
        return found, [fail(root, f"cannot list root directory: {error}")]

    for name in entries:
        path = root / name
        try:
            info = os.stat(name, dir_fd=root_fd, follow_symlinks=False)
        except OSError as error:
            errors.append(fail(path, f"cannot stat path: {error}"))
            continue
        if stat.S_ISLNK(info.st_mode):
            errors.append(fail(path, "symlink not allowed"))
        elif r2.is_reparse_point(info):
            errors.append(fail(path, "reparse point not allowed"))
        elif stat.S_ISDIR(info.st_mode):
            errors.append(fail(path, "unexpected directory"))
        elif not stat.S_ISREG(info.st_mode):
            errors.append(fail(path, "not a regular file"))
        elif Path(name).suffix.lower() == ".pdf":
            if reason := r2.safe_pdf_name(name):
                errors.append(fail(path, reason))
            else:
                found.add(name)

    return found, errors


def sha256_file_from_windows(path: Path) -> tuple[str | None, str | None]:
    return r2.sha256_file(path)


def verify_windows_root(
    root: Path,
    expected: dict[str, str],
    root_identity: r2.WindowsIdentity,
) -> list[str]:
    errors: list[str] = []
    try:
        actual_pdfs, walk_errors = root_pdfs_from_windows(root)
        errors.extend(walk_errors)
        expected_names = set(expected)
        for name in sorted(actual_pdfs - expected_names):
            errors.append(fail(root / name, "unexpected PDF"))

        for name, expected_digest in expected.items():
            path = root / name
            actual_digest, reason = sha256_file_from_windows(path)
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

        after_handle, after_identity = r2.open_windows_directory(root)
        after_handle.close()
        if not r2.same_identity(root_identity, after_identity):
            errors.append(fail(root, "root directory changed during verification"))
    except r2.R2Error as error:
        errors.append(str(error))
    return errors


def sha256_file_from_fd(root_fd: int, name: str) -> tuple[str | None, str | None]:
    return r2.sha256_file_from_fd(root_fd, Path("."), name)


def verify(manifest: Path, root: Path) -> tuple[list[str], int]:
    errors: list[str] = []
    expected: dict[str, str] = {}

    if r2.can_use_dir_fd():
        root_info, root_error = r2.directory_identity(root)
        if root_error:
            errors.append(fail(root, root_error))

        expected, manifest_errors = read_manifest(manifest)
        errors.extend(manifest_errors)
        if errors:
            return errors, len(expected)
        assert root_info is not None

        descriptor = -1
        try:
            descriptor, opened = r2.open_directory_fd(root)
            if not r2.same_identity(root_info, opened):
                errors.append(fail(root, "root directory changed during verification"))
                return errors, len(expected)
            actual_pdfs, walk_errors = root_pdfs_from_fd(root, descriptor)
            errors.extend(walk_errors)
            expected_names = set(expected)
            for name in sorted(actual_pdfs - expected_names):
                errors.append(fail(root / name, "unexpected PDF"))

            for name, expected_digest in expected.items():
                path = root / name
                actual_digest, reason = sha256_file_from_fd(descriptor, name)
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

            try:
                after = root.lstat()
            except OSError as error:
                errors.append(fail(root, f"cannot restat root directory: {error}"))
            else:
                if not r2.same_identity(opened, after):
                    errors.append(fail(root, "root directory changed during verification"))
        except r2.R2Error as error:
            errors.append(str(error))
        finally:
            if descriptor >= 0:
                os.close(descriptor)
        return errors, len(expected)

    if r2.has_windows_handles():
        root_handle = None
        try:
            root_handle, root_identity = r2.open_windows_directory(root)
            expected, manifest_errors = read_manifest(manifest)
            errors.extend(manifest_errors)
            if not errors:
                errors.extend(verify_windows_root(root, expected, root_identity))
        except r2.R2Error as error:
            errors.append(str(error))
        finally:
            if root_handle is not None:
                root_handle.close()
        return errors, len(expected)

    expected, manifest_errors = read_manifest(manifest)
    errors.extend(manifest_errors)
    if not manifest_errors:
        errors.append(fail(root, r2.unsupported_backend_message("verification")))

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
