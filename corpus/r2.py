#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import re
import errno
import hashlib
import platform
import secrets
import shutil
import stat
import sys
from pathlib import Path, PureWindowsPath

import ctypes

sys.dont_write_bytecode = True

if os.name == "nt":
    import msvcrt
    from ctypes import wintypes
else:
    msvcrt = None
    wintypes = None

STAMP_NAME = ".fetch-stamp"
NO_CHECKSUM_SENTINEL = "--no-checksum"
PAYLOAD_DIR_NAME = "payload"
SAFE_COMPONENT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
CHECKSUM_MANIFEST_LINE_RE = re.compile(r"^([0-9a-f]{64})  (.+)$")
WINDOWS_RESERVED_BASENAMES = {
    "con",
    "prn",
    "aux",
    "nul",
    *(f"com{index}" for index in range(1, 10)),
    *(f"lpt{index}" for index in range(1, 10)),
}
COPY_CHUNK_SIZE = 1024 * 1024
PRIVATE_PUBLISH_PREFIX = "r2publish"
RENAME_NOREPLACE = 1
RENAME_EXCL = 0x4
FILE_ATTRIBUTE_DIRECTORY = 0x00000010
FILE_ATTRIBUTE_REPARSE_POINT = 0x00000400

if os.name == "nt":
    DELETE = 0x00010000
    GENERIC_READ = 0x80000000
    GENERIC_WRITE = 0x40000000
    FILE_LIST_DIRECTORY = 0x00000001
    FILE_READ_ATTRIBUTES = 0x00000080
    FILE_SHARE_READ = 0x00000001
    FILE_SHARE_WRITE = 0x00000002
    CREATE_NEW = 1
    OPEN_EXISTING = 3
    FILE_ATTRIBUTE_NORMAL = 0x00000080
    FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000
    FILE_FLAG_BACKUP_SEMANTICS = 0x02000000
    FILE_FLAG_SEQUENTIAL_SCAN = 0x08000000
    FILE_TYPE_DISK = 0x00000001
    INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
    INVALID_FILE_ATTRIBUTES = 0xFFFFFFFF
    ERROR_FILE_NOT_FOUND = 2
    ERROR_PATH_NOT_FOUND = 3
    ERROR_FILE_EXISTS = 80
    ERROR_ALREADY_EXISTS = 183
    FILE_RENAME_INFO_CLASS = 3
    FILE_DISPOSITION_INFO_CLASS = 4
    FILE_ATTRIBUTE_TAG_INFO_CLASS = 9
    FILE_ID_INFO_CLASS = 18

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class FILE_ATTRIBUTE_TAG_INFO(ctypes.Structure):
        _fields_ = [
            ("FileAttributes", wintypes.DWORD),
            ("ReparseTag", wintypes.DWORD),
        ]

    class FILE_ID_128(ctypes.Structure):
        _fields_ = [("Identifier", ctypes.c_ubyte * 16)]

    class FILE_ID_INFO(ctypes.Structure):
        _fields_ = [
            ("VolumeSerialNumber", ctypes.c_ulonglong),
            ("FileId", FILE_ID_128),
        ]

    class FILE_RENAME_INFO(ctypes.Structure):
        _fields_ = [
            ("ReplaceIfExists", ctypes.c_ubyte),
            ("RootDirectory", wintypes.HANDLE),
            ("FileNameLength", wintypes.DWORD),
            ("FileName", ctypes.c_uint16 * 1),
        ]

    class FILE_DISPOSITION_INFO(ctypes.Structure):
        _fields_ = [("DeleteFile", ctypes.c_ubyte)]

    kernel32.CreateFileW.argtypes = [
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.LPVOID,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.HANDLE,
    ]
    kernel32.CreateFileW.restype = wintypes.HANDLE
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL
    kernel32.CreateDirectoryW.argtypes = [wintypes.LPCWSTR, wintypes.LPVOID]
    kernel32.CreateDirectoryW.restype = wintypes.BOOL
    kernel32.GetFileAttributesW.argtypes = [wintypes.LPCWSTR]
    kernel32.GetFileAttributesW.restype = wintypes.DWORD
    kernel32.GetFileType.argtypes = [wintypes.HANDLE]
    kernel32.GetFileType.restype = wintypes.DWORD
    try:
        get_file_information_by_handle_ex = kernel32.GetFileInformationByHandleEx
    except AttributeError:
        get_file_information_by_handle_ex = None
    else:
        get_file_information_by_handle_ex.argtypes = [
            wintypes.HANDLE,
            ctypes.c_int,
            wintypes.LPVOID,
            wintypes.DWORD,
        ]
        get_file_information_by_handle_ex.restype = wintypes.BOOL
    kernel32.SetFileInformationByHandle.argtypes = [
        wintypes.HANDLE,
        ctypes.c_int,
        wintypes.LPVOID,
        wintypes.DWORD,
    ]
    kernel32.SetFileInformationByHandle.restype = wintypes.BOOL
else:
    kernel32 = None
    get_file_information_by_handle_ex = None


class R2Error(Exception):
    pass


class WindowsHandle:
    def __init__(self, handle: int):
        self.handle = handle

    def close(self) -> None:
        if self.handle is not None:
            kernel32.CloseHandle(self.handle)
            self.handle = None

    def detach(self) -> int:
        if self.handle is None:
            raise R2Error("Windows handle already transferred")
        handle = self.handle
        self.handle = None
        return handle

    def __enter__(self) -> "WindowsHandle":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()


WindowsIdentity = tuple[str, int, bytes]


def has_windows_handles() -> bool:
    return (
        os.name == "nt"
        and kernel32 is not None
        and msvcrt is not None
        and get_file_information_by_handle_ex is not None
    )


def unsupported_backend_message(action: str) -> str:
    return (
        f"{action}: unsupported platform: requires Unix dir-fd or Windows handle backend"
    )


def windows_message(error: int) -> str:
    message = ctypes.FormatError(error).strip()
    if message:
        return message
    return f"Win32 error {error}"


def windows_last_error(action: str) -> str:
    return f"{action}: {windows_message(ctypes.get_last_error())}"


def windows_path_attributes(path: Path) -> tuple[int | None, str | None]:
    attributes = kernel32.GetFileAttributesW(str(path))
    if attributes == INVALID_FILE_ATTRIBUTES:
        error = ctypes.get_last_error()
        if error in {ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND}:
            return None, "missing file"
        return None, windows_message(error)
    return int(attributes), None


def windows_create_file_handle(
    path: Path,
    desired_access: int,
    share_mode: int,
    creation_disposition: int,
    flags: int,
    action: str,
) -> WindowsHandle:
    handle = kernel32.CreateFileW(
        str(path),
        desired_access,
        share_mode,
        None,
        creation_disposition,
        flags,
        None,
    )
    if handle is None or handle == INVALID_HANDLE_VALUE:
        error = ctypes.get_last_error()
        if creation_disposition == OPEN_EXISTING and error in {
            ERROR_FILE_NOT_FOUND,
            ERROR_PATH_NOT_FOUND,
        }:
            raise R2Error("missing file")
        raise R2Error(f"{action}: {windows_message(error)}")
    return WindowsHandle(handle)


def windows_handle_to_descriptor(handle: WindowsHandle, flags: int, path: Path) -> int:
    raw_handle = handle.detach()
    try:
        return msvcrt.open_osfhandle(
            raw_handle, flags | getattr(os, "O_NOINHERIT", 0)
        )
    except OSError as error:
        kernel32.CloseHandle(raw_handle)
        raise R2Error(f"{path}: cannot wrap file handle: {error}") from error


def windows_require_disk(handle: WindowsHandle, path: Path) -> None:
    if kernel32.GetFileType(handle.handle) != FILE_TYPE_DISK:
        raise R2Error(f"{path}: not a disk file")


def windows_attribute_tag(handle: WindowsHandle, path: Path) -> tuple[int, int]:
    if get_file_information_by_handle_ex is None:
        raise R2Error(f"{path}: Windows FileAttributeTagInfo is unavailable")
    info = FILE_ATTRIBUTE_TAG_INFO()
    if not get_file_information_by_handle_ex(
        handle.handle,
        FILE_ATTRIBUTE_TAG_INFO_CLASS,
        ctypes.byref(info),
        ctypes.sizeof(info),
    ):
        raise R2Error(f"{path}: {windows_last_error('cannot inspect file attributes')}")
    return int(info.FileAttributes), int(info.ReparseTag)


def windows_identity(handle: WindowsHandle, path: Path) -> WindowsIdentity:
    if get_file_information_by_handle_ex is None:
        raise R2Error(f"{path}: Windows FileIdInfo is unavailable")
    info = FILE_ID_INFO()
    if not get_file_information_by_handle_ex(
        handle.handle,
        FILE_ID_INFO_CLASS,
        ctypes.byref(info),
        ctypes.sizeof(info),
    ):
        raise R2Error(f"{path}: {windows_last_error('cannot read file identity')}")
    return ("windows", int(info.VolumeSerialNumber), bytes(info.FileId.Identifier))


def ensure_windows_regular_file(handle: WindowsHandle, path: Path) -> WindowsIdentity:
    windows_require_disk(handle, path)
    attributes, reparse_tag = windows_attribute_tag(handle, path)
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT or reparse_tag != 0:
        raise R2Error(f"{path}: reparse point not allowed")
    if attributes & FILE_ATTRIBUTE_DIRECTORY:
        raise R2Error(f"{path}: not a regular file")
    return windows_identity(handle, path)


def ensure_windows_directory(handle: WindowsHandle, path: Path) -> WindowsIdentity:
    windows_require_disk(handle, path)
    attributes, reparse_tag = windows_attribute_tag(handle, path)
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT or reparse_tag != 0:
        raise R2Error(f"{path}: reparse point not allowed")
    if not attributes & FILE_ATTRIBUTE_DIRECTORY:
        raise R2Error(f"{path}: not a directory")
    return windows_identity(handle, path)


def open_windows_regular_file(path: Path):
    attributes, reason = windows_path_attributes(path)
    if reason:
        return None, reason
    assert attributes is not None
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT:
        return None, "reparse point not allowed"
    if attributes & FILE_ATTRIBUTE_DIRECTORY:
        return None, "not a regular file"

    try:
        handle = windows_create_file_handle(
            path,
            GENERIC_READ,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN,
            "cannot open file",
        )
    except R2Error as error:
        return None, str(error)

    try:
        ensure_windows_regular_file(handle, path)
        descriptor = windows_handle_to_descriptor(
            handle, os.O_RDONLY | getattr(os, "O_BINARY", 0), path
        )
    except R2Error as error:
        handle.close()
        return None, str(error).removeprefix(f"{path}: ")

    try:
        return os.fdopen(descriptor, "rb"), None
    except OSError as error:
        os.close(descriptor)
        return None, f"cannot wrap file descriptor: {error}"


def open_windows_directory(path: Path) -> tuple[WindowsHandle, WindowsIdentity]:
    try:
        handle = windows_create_file_handle(
            path,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            "cannot open directory",
        )
        identity = ensure_windows_directory(handle, path)
        return handle, identity
    except R2Error as error:
        try:
            handle.close()
        except UnboundLocalError:
            pass
        message = str(error)
        if message.startswith(f"{path}: "):
            raise
        raise R2Error(f"{path}: {message}") from error


def create_windows_regular_file(path: Path):
    try:
        handle = windows_create_file_handle(
            path,
            GENERIC_WRITE | FILE_READ_ATTRIBUTES,
            0,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            "cannot create destination file",
        )
        ensure_windows_regular_file(handle, path)
        descriptor = windows_handle_to_descriptor(
            handle, os.O_WRONLY | getattr(os, "O_BINARY", 0), path
        )
    except R2Error:
        try:
            handle.close()
        except UnboundLocalError:
            pass
        raise

    try:
        return os.fdopen(descriptor, "wb")
    except OSError as error:
        os.close(descriptor)
        raise R2Error(f"{path}: cannot wrap file descriptor: {error}") from error


def is_reparse_point(info: os.stat_result) -> bool:
    attributes = getattr(info, "st_file_attributes", 0)
    return bool(attributes & FILE_ATTRIBUTE_REPARSE_POINT)


def is_windows_reserved_component(name: str) -> bool:
    stem = name.rstrip(" .").split(".", 1)[0].casefold()
    return stem in WINDOWS_RESERVED_BASENAMES


def safe_component(name: str) -> str | None:
    if name != name.strip():
        return "path has leading or trailing whitespace"
    if not name:
        return "empty path not allowed"
    if any(ord(character) < 32 or ord(character) == 127 for character in name):
        return "control character not allowed"
    if name in {".", ".."}:
        return "path traversal not allowed"

    windows = PureWindowsPath(name)
    if Path(name).is_absolute() or windows.is_absolute():
        return "absolute path not allowed"
    if windows.drive:
        return "drive-qualified path not allowed"
    if "/" in name or "\\" in name:
        if any(part in {".", ".."} for part in re.split(r"[\\/]", name)):
            return "path traversal not allowed"
        return "path separators are not allowed"
    if ":" in name:
        return "colon in path not allowed"
    if name.endswith("."):
        return "trailing dot not allowed"
    if not SAFE_COMPONENT_RE.fullmatch(name):
        return "path is not a safe single segment"
    if is_windows_reserved_component(name):
        return "Windows reserved device name not allowed"
    return None


def safe_id_segment(identifier: str) -> str | None:
    return safe_component(identifier)


def safe_pdf_name(name: str) -> str | None:
    if reason := safe_component(name):
        return reason
    if Path(name).suffix.lower() != ".pdf":
        return "manifest entry is not a PDF"
    return None


def open_regular_file(path: Path):
    if has_windows_handles():
        return open_windows_regular_file(path)
    if os.name != "posix":
        return None, unsupported_backend_message("regular-file open")
    return open_unix_regular_file(path)


def open_unix_regular_file(path: Path):
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


def directory_identity(path: Path) -> tuple[os.stat_result | None, str | None]:
    try:
        info = path.lstat()
    except FileNotFoundError:
        return None, "missing root directory"
    except OSError as error:
        return None, f"cannot stat root directory: {error}"

    if stat.S_ISLNK(info.st_mode):
        return None, "symlink not allowed"
    if is_reparse_point(info):
        return None, "reparse point not allowed"
    if not stat.S_ISDIR(info.st_mode):
        return None, "not a directory"
    return info, None


def is_windows_identity(value) -> bool:
    return (
        isinstance(value, tuple)
        and len(value) == 3
        and value[0] == "windows"
        and isinstance(value[2], bytes)
    )


def same_identity(
    left: os.stat_result | WindowsIdentity, right: os.stat_result | WindowsIdentity
) -> bool:
    if is_windows_identity(left) or is_windows_identity(right):
        return left == right
    try:
        return os.path.samestat(left, right)
    except OSError:
        return False


def ensure_same_directory_identity(
    path: Path, expected: os.stat_result, changed_reason: str
) -> str | None:
    current, reason = directory_identity(path)
    if reason:
        return reason
    if current is None or not same_identity(expected, current):
        return changed_reason
    return None


def ensure_same_directory(path: Path, expected: os.stat_result) -> str | None:
    return ensure_same_directory_identity(
        path, expected, "root directory changed during verification"
    )


def validate_destination_name(dest: Path) -> None:
    if reason := safe_component(dest.name):
        raise R2Error(f"{dest}: destination name is not safe: {reason}")


def destination_parent_identity(
    dest: Path, allow_missing: bool = False
) -> os.stat_result | None:
    validate_destination_name(dest)
    parent = dest.parent
    try:
        info = parent.lstat()
    except FileNotFoundError as error:
        if allow_missing:
            return None
        raise R2Error(f"{parent}: missing parent directory") from error
    except OSError as error:
        raise R2Error(f"{parent}: cannot stat parent directory: {error}") from error

    if stat.S_ISLNK(info.st_mode):
        raise R2Error(f"{parent}: symlink not allowed")
    if is_reparse_point(info):
        raise R2Error(f"{parent}: reparse point not allowed")
    if not stat.S_ISDIR(info.st_mode):
        raise R2Error(f"{parent}: not a directory")
    return info


def destination_state(dest: Path) -> str:
    if destination_parent_identity(dest, allow_missing=True) is None:
        return "absent"

    try:
        info = dest.lstat()
    except FileNotFoundError:
        return "absent"
    except OSError as error:
        raise R2Error(f"{dest}: cannot stat destination: {error}") from error

    if stat.S_ISLNK(info.st_mode):
        raise R2Error(f"{dest}: symlink not allowed")
    if is_reparse_point(info):
        raise R2Error(f"{dest}: reparse point not allowed")
    if not stat.S_ISDIR(info.st_mode):
        raise R2Error(f"{dest}: not a directory")

    stamp = dest / STAMP_NAME
    try:
        stamp_info = stamp.lstat()
    except FileNotFoundError as error:
        raise R2Error(
            f"{dest} exists but has no {STAMP_NAME} (interrupted fetch?)"
        ) from error
    except OSError as error:
        raise R2Error(f"{stamp}: cannot stat stamp: {error}") from error

    if stat.S_ISLNK(stamp_info.st_mode):
        raise R2Error(f"{stamp}: symlink not allowed")
    if is_reparse_point(stamp_info):
        raise R2Error(f"{stamp}: reparse point not allowed")
    if not stat.S_ISREG(stamp_info.st_mode):
        raise R2Error(f"{stamp}: not a regular file")
    return "stamped"


def fail(path: Path | str, reason: str) -> str:
    return f"{path}: {reason}"


def path_error(manifest: Path, line_number: int, reason: str) -> str:
    return f"{manifest}:{line_number}: {reason}"


def read_regular_utf8(path: Path, description: str) -> str:
    handle, reason = open_regular_file(path)
    if reason:
        raise R2Error(f"{path}: {reason}")
    assert handle is not None

    try:
        with handle:
            return handle.read().decode("utf-8")
    except UnicodeDecodeError as error:
        raise R2Error(f"{path}: {description} is not UTF-8: {error}") from error
    except OSError as error:
        raise R2Error(f"{path}: cannot read {description}: {error}") from error


def checksum_manifest(path: Path) -> tuple[dict[str, str], list[str]]:
    expected: dict[str, str] = {}
    canonical_names: dict[str, str] = {}
    errors: list[str] = []
    names: list[str] = []

    try:
        contents = read_regular_utf8(path, "manifest")
    except R2Error as error:
        return {}, [str(error)]

    for line_number, line in enumerate(contents.splitlines(), start=1):
        match = CHECKSUM_MANIFEST_LINE_RE.fullmatch(line)
        if match is None:
            errors.append(path_error(path, line_number, "malformed line"))
            continue

        digest, name = match.groups()

        if reason := safe_pdf_name(name):
            errors.append(path_error(path, line_number, f"{name}: {reason}"))
            continue

        canonical = name.casefold()
        if canonical in canonical_names:
            previous = canonical_names[canonical]
            errors.append(
                path_error(
                    path,
                    line_number,
                    f"{name}: duplicate name (aliases {previous})",
                )
            )
            continue

        canonical_names[canonical] = name
        expected[name] = digest
        names.append(name)

    if names != sorted(names):
        errors.append(fail(path, "manifest entries are not sorted by name"))
    if not names and not errors:
        errors.append(fail(path, "manifest has no entries"))

    return expected, errors


def checksum_manifest_or_error(path: Path) -> dict[str, str]:
    expected, errors = checksum_manifest(path)
    if errors:
        raise R2Error("; ".join(errors))
    return expected


def manifest_ids(path: Path) -> list[str]:
    contents = read_regular_utf8(path, "manifest")

    try:
        entries = json.loads(contents)
    except json.JSONDecodeError as error:
        raise R2Error(f"{path}: invalid JSON: {error}") from error

    if not isinstance(entries, list):
        raise R2Error(f"{path}: manifest must be a top-level JSON array")

    ids: list[str] = []
    canonical_ids: dict[str, str] = {}
    for index, entry in enumerate(entries, start=1):
        if isinstance(entry, str):
            identifier = entry
        elif isinstance(entry, dict):
            identifier = entry.get("id")
            if not isinstance(identifier, str):
                raise R2Error(f"{path}:{index}: manifest entry id is not a string")
        else:
            raise R2Error(
                f"{path}:{index}: manifest entry must be a string or object with an id"
            )

        if reason := safe_id_segment(identifier):
            raise R2Error(f"{path}:{index}: {identifier!r}: {reason}")
        canonical = identifier.casefold()
        if previous := canonical_ids.get(canonical):
            raise R2Error(
                f"{path}:{index}: {identifier}: duplicate id (aliases {previous})"
            )
        canonical_ids[canonical] = identifier
        ids.append(identifier)

    if not ids:
        raise R2Error(f"{path}: manifest lists no ids")
    return ids


def can_use_dir_fd() -> bool:
    supports_dir_fd = getattr(os, "supports_dir_fd", set())
    supports_fd = getattr(os, "supports_fd", set())
    return (
        os.name == "posix"
        and hasattr(os, "O_DIRECTORY")
        and os.open in supports_dir_fd
        and os.stat in supports_dir_fd
        and os.listdir in supports_fd
    )


def can_publish_with_dir_fd() -> bool:
    supports_dir_fd = getattr(os, "supports_dir_fd", set())
    return (
        can_use_dir_fd()
        and os.mkdir in supports_dir_fd
        and os.rmdir in supports_dir_fd
        and os.unlink in supports_dir_fd
    )


def open_directory_fd(path: Path) -> tuple[int, os.stat_result]:
    before, reason = directory_identity(path)
    if reason:
        raise R2Error(f"{path}: {reason}")
    assert before is not None

    flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise R2Error(f"{path}: cannot open directory: {error}") from error

    try:
        opened = os.fstat(descriptor)
        if not stat.S_ISDIR(opened.st_mode):
            os.close(descriptor)
            raise R2Error(f"{path}: not a directory")
        if is_reparse_point(opened):
            os.close(descriptor)
            raise R2Error(f"{path}: reparse point not allowed")
        if not same_identity(before, opened):
            os.close(descriptor)
            raise R2Error(f"{path}: root directory changed during verification")
        return descriptor, opened
    except OSError as error:
        os.close(descriptor)
        raise R2Error(f"{path}: cannot verify directory identity: {error}") from error


def open_regular_file_from_fd(root_fd: int, root: Path, name: str):
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    path = root / name
    try:
        before = os.stat(name, dir_fd=root_fd, follow_symlinks=False)
    except OSError as error:
        return None, f"cannot stat file: {error}"

    if stat.S_ISLNK(before.st_mode):
        return None, "symlink not allowed"
    if is_reparse_point(before):
        return None, "reparse point not allowed"
    if not stat.S_ISREG(before.st_mode):
        return None, "not a regular file"

    try:
        descriptor = os.open(name, flags, dir_fd=root_fd)
    except OSError as error:
        return None, f"cannot open file: {error}"

    try:
        opened = os.fstat(descriptor)
        after = os.stat(name, dir_fd=root_fd, follow_symlinks=False)
        if is_reparse_point(opened) or is_reparse_point(after):
            os.close(descriptor)
            return None, "reparse point not allowed"
        if not stat.S_ISREG(opened.st_mode) or not stat.S_ISREG(after.st_mode):
            os.close(descriptor)
            return None, "not a regular file"
        if not same_identity(before, opened) or not same_identity(opened, after):
            os.close(descriptor)
            return None, "file changed during verification"
        try:
            return os.fdopen(descriptor, "rb"), None
        except OSError as error:
            os.close(descriptor)
            return None, f"cannot wrap file descriptor: {error}"
    except OSError as error:
        os.close(descriptor)
        return None, f"cannot verify file identity: {error}"


def sha256_stream(handle) -> str:
    digest = hashlib.sha256()
    for chunk in iter(lambda: handle.read(COPY_CHUNK_SIZE), b""):
        digest.update(chunk)
    return digest.hexdigest()


def sha256_file(path: Path) -> tuple[str | None, str | None]:
    handle, reason = open_regular_file(path)
    if reason:
        return None, reason
    assert handle is not None

    try:
        with handle:
            return sha256_stream(handle), None
    except OSError as error:
        return None, f"cannot read file: {error}"


def sha256_file_from_fd(root_fd: int, root: Path, name: str) -> tuple[str | None, str | None]:
    handle, reason = open_regular_file_from_fd(root_fd, root, name)
    if reason:
        return None, reason
    assert handle is not None

    try:
        with handle:
            return sha256_stream(handle), None
    except OSError as error:
        return None, f"cannot read file: {error}"


def staged_pdf_names_from_fd(staging: Path, staging_fd: int) -> list[str]:
    names: list[str] = []
    canonical_names: dict[str, str] = {}
    try:
        entries = sorted(os.listdir(staging_fd))
    except OSError as error:
        raise R2Error(f"{staging}: cannot list staging directory: {error}") from error

    for name in entries:
        path = staging / name
        if reason := safe_pdf_name(name):
            raise R2Error(f"{path}: {reason}")
        handle, reason = open_regular_file_from_fd(staging_fd, staging, name)
        if reason:
            raise R2Error(f"{path}: {reason}")
        assert handle is not None
        handle.close()

        canonical = name.casefold()
        if previous := canonical_names.get(canonical):
            raise R2Error(f"{path}: duplicate name (aliases {previous})")
        canonical_names[canonical] = name
        names.append(name)

    if not names:
        raise R2Error(f"{staging}: staging directory has no PDF entries")
    return names


def staged_pdf_names_windows(staging: Path) -> list[str]:
    names: list[str] = []
    canonical_names: dict[str, str] = {}
    try:
        entries = sorted(staging.iterdir(), key=lambda path: path.name)
    except OSError as error:
        raise R2Error(f"{staging}: cannot list staging directory: {error}") from error

    for path in entries:
        name = path.name
        if reason := safe_pdf_name(name):
            raise R2Error(f"{path}: {reason}")
        handle, reason = open_regular_file(path)
        if reason:
            raise R2Error(f"{path}: {reason}")
        assert handle is not None
        handle.close()

        canonical = name.casefold()
        if previous := canonical_names.get(canonical):
            raise R2Error(f"{path}: duplicate name (aliases {previous})")
        canonical_names[canonical] = name
        names.append(name)

    if not names:
        raise R2Error(f"{staging}: staging directory has no PDF entries")
    return names


def staged_pdf_names(staging: Path) -> list[str]:
    if can_use_dir_fd():
        descriptor = -1
        try:
            descriptor, _ = open_directory_fd(staging)
            return staged_pdf_names_from_fd(staging, descriptor)
        finally:
            if descriptor >= 0:
                os.close(descriptor)
    if has_windows_handles():
        handle = None
        try:
            handle, _ = open_windows_directory(staging)
            return staged_pdf_names_windows(staging)
        finally:
            if handle is not None:
                handle.close()
    raise R2Error(unsupported_backend_message("staging validation"))


def open_destination_parent_fd(
    dest: Path, expected: os.stat_result
) -> tuple[int, os.stat_result]:
    descriptor, opened = open_directory_fd(dest.parent)
    if not same_identity(expected, opened):
        os.close(descriptor)
        raise R2Error(f"{dest.parent}: parent directory changed during publication")
    return descriptor, opened


def child_directory_identity_by_name(
    parent_fd: int, parent: Path, name: str
) -> os.stat_result:
    path = parent / name
    try:
        info = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    except FileNotFoundError as error:
        raise R2Error(f"{path}: missing destination directory") from error
    except OSError as error:
        raise R2Error(f"{path}: cannot stat destination directory: {error}") from error

    if stat.S_ISLNK(info.st_mode):
        raise R2Error(f"{path}: symlink not allowed")
    if is_reparse_point(info):
        raise R2Error(f"{path}: reparse point not allowed")
    if not stat.S_ISDIR(info.st_mode):
        raise R2Error(f"{path}: not a directory")
    return info


def child_directory_identity(parent_fd: int, dest: Path) -> os.stat_result:
    return child_directory_identity_by_name(parent_fd, dest.parent, dest.name)


def private_publish_name(dest: Path) -> str:
    for _ in range(128):
        candidate = f"{PRIVATE_PUBLISH_PREFIX}-{dest.name}-{os.getpid()}-{secrets.token_hex(12)}"
        if safe_component(candidate) is None:
            return candidate
    raise R2Error(f"{dest}: cannot generate a safe private publication name")


def create_private_directory_from_parent(parent_fd: int, dest: Path) -> str:
    for _ in range(128):
        name = private_publish_name(dest)
        try:
            os.mkdir(name, 0o700, dir_fd=parent_fd)
            return name
        except FileExistsError:
            continue
        except OSError as error:
            raise R2Error(
                f"{dest.parent / name}: cannot create private publication directory: {error}"
            ) from error
    raise R2Error(f"{dest}: cannot allocate a private publication directory")


def open_child_directory_fd_by_name(
    parent_fd: int, parent: Path, name: str
) -> tuple[int, os.stat_result]:
    before = child_directory_identity_by_name(parent_fd, parent, name)
    flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(name, flags, dir_fd=parent_fd)
    except OSError as error:
        raise R2Error(f"{parent / name}: cannot open destination directory: {error}") from error

    try:
        opened = os.fstat(descriptor)
        after = child_directory_identity_by_name(parent_fd, parent, name)
        if not stat.S_ISDIR(opened.st_mode):
            os.close(descriptor)
            raise R2Error(f"{parent / name}: not a directory")
        if is_reparse_point(opened):
            os.close(descriptor)
            raise R2Error(f"{parent / name}: reparse point not allowed")
        if not same_identity(before, opened) or not same_identity(opened, after):
            os.close(descriptor)
            raise R2Error(f"{parent / name}: destination directory changed during publication")
        return descriptor, opened
    except OSError as error:
        os.close(descriptor)
        raise R2Error(f"{parent / name}: cannot verify destination identity: {error}") from error


def open_child_directory_fd(parent_fd: int, dest: Path) -> tuple[int, os.stat_result]:
    return open_child_directory_fd_by_name(parent_fd, dest.parent, dest.name)


def copy_file_from_fd_to_fd(source_fd: int, source_root: Path, name: str, dest_fd: int) -> None:
    handle, reason = open_regular_file_from_fd(source_fd, source_root, name)
    if reason:
        raise R2Error(f"{source_root / name}: {reason}")
    assert handle is not None

    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_BINARY", 0)
    try:
        descriptor = os.open(name, flags, 0o644, dir_fd=dest_fd)
    except OSError as error:
        handle.close()
        raise R2Error(f"{name}: cannot create destination file: {error}") from error

    try:
        output = os.fdopen(descriptor, "wb")
        descriptor = -1
        with handle, output:
            shutil.copyfileobj(handle, output, COPY_CHUNK_SIZE)
    except OSError as error:
        if descriptor >= 0:
            os.close(descriptor)
        handle.close()
        raise R2Error(f"{name}: cannot copy staged file: {error}") from error


def write_stamp_to_fd(dest_fd: int, source: str, revision: str) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_BINARY", 0)
    try:
        descriptor = os.open(STAMP_NAME, flags, 0o644, dir_fd=dest_fd)
    except OSError as error:
        raise R2Error(f"{STAMP_NAME}: cannot create stamp: {error}") from error

    try:
        handle = os.fdopen(descriptor, "w", encoding="utf-8")
        descriptor = -1
        with handle:
            handle.write(f"source={source}\n")
            handle.write(f"revision={revision}\n")
    except OSError as error:
        if descriptor >= 0:
            os.close(descriptor)
        raise R2Error(f"{STAMP_NAME}: cannot write stamp: {error}") from error


def validate_expected_names(root: Path, names: list[str], expected: dict[str, str]) -> None:
    expected_names = set(expected)
    actual_names = set(names)
    if actual_names == expected_names:
        return

    details: list[str] = []
    missing = sorted(expected_names - actual_names)
    unexpected = sorted(actual_names - expected_names)
    if missing:
        details.append(f"missing {', '.join(missing)}")
    if unexpected:
        details.append(f"unexpected {', '.join(unexpected)}")
    raise R2Error(f"{root}: PDF set does not match checksum manifest ({'; '.join(details)})")


def validate_private_contents_from_fd(
    root: Path,
    root_fd: int,
    expected_names: list[str],
    expected_checksums: dict[str, str] | None,
) -> None:
    names = staged_pdf_names_from_fd(root, root_fd)
    if expected_checksums is not None:
        validate_expected_names(root, names, expected_checksums)
        for name, expected_digest in expected_checksums.items():
            actual_digest, reason = sha256_file_from_fd(root_fd, root, name)
            if reason:
                raise R2Error(f"{root / name}: {reason}")
            if actual_digest != expected_digest:
                raise R2Error(
                    f"{root / name}: sha256 mismatch: expected {expected_digest}, got {actual_digest}"
                )
        return

    validate_expected_names(root, names, {name: "" for name in expected_names})


def create_payload_directory_from_private(
    private_fd: int, private_path: Path
) -> tuple[int, os.stat_result]:
    try:
        os.mkdir(PAYLOAD_DIR_NAME, 0o755, dir_fd=private_fd)
    except FileExistsError as error:
        raise R2Error(f"{private_path / PAYLOAD_DIR_NAME}: payload directory already exists") from error
    except OSError as error:
        raise R2Error(
            f"{private_path / PAYLOAD_DIR_NAME}: cannot create payload directory: {error}"
        ) from error
    return open_child_directory_fd_by_name(private_fd, private_path, PAYLOAD_DIR_NAME)


def linux_renameat2_syscall_number() -> int | None:
    machine = platform.machine().casefold()
    if machine in {"x86_64", "amd64"}:
        return 316
    if machine in {"aarch64", "arm64"}:
        return 276
    if machine in {"i386", "i686", "x86"}:
        return 353
    return None


def exclusive_finalize_error(dest: Path, error_number: int) -> R2Error:
    if error_number in {errno.EEXIST, getattr(errno, "ENOTEMPTY", errno.EEXIST)}:
        return R2Error(f"{dest}: destination appeared before publication")
    if error_number in {
        errno.EINVAL,
        getattr(errno, "ENOTSUP", errno.EINVAL),
        getattr(errno, "ENOSYS", errno.EINVAL),
        errno.EXDEV,
    }:
        return R2Error(
            f"{dest}: exclusive finalization unsupported on this platform: {os.strerror(error_number)}"
        )
    return R2Error(f"{dest}: cannot finalize publication: {os.strerror(error_number)}")


def finalize_payload_directory_unix(
    private_fd: int, parent_fd: int, dest: Path
) -> None:
    old_name = os.fsencode(PAYLOAD_DIR_NAME)
    new_name = os.fsencode(dest.name)

    if sys.platform.startswith("linux"):
        number = linux_renameat2_syscall_number()
        if number is None:
            raise R2Error(
                f"{dest}: exclusive finalization unsupported on Linux architecture {platform.machine()}"
            )
        libc = ctypes.CDLL(None, use_errno=True)
        renameat2 = libc.syscall
        renameat2.argtypes = [
            ctypes.c_long,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_uint,
        ]
        renameat2.restype = ctypes.c_int
        result = renameat2(
            ctypes.c_long(number),
            ctypes.c_int(private_fd),
            ctypes.c_char_p(old_name),
            ctypes.c_int(parent_fd),
            ctypes.c_char_p(new_name),
            ctypes.c_uint(RENAME_NOREPLACE),
        )
        if result != 0:
            raise exclusive_finalize_error(dest, ctypes.get_errno())
        return

    if sys.platform == "darwin":
        libc = ctypes.CDLL(None, use_errno=True)
        renameatx_np = getattr(libc, "renameatx_np", None)
        if renameatx_np is None:
            raise R2Error(f"{dest}: exclusive finalization unsupported on this macOS runtime")
        renameatx_np.argtypes = [
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_uint,
        ]
        renameatx_np.restype = ctypes.c_int
        result = renameatx_np(
            ctypes.c_int(private_fd),
            ctypes.c_char_p(old_name),
            ctypes.c_int(parent_fd),
            ctypes.c_char_p(new_name),
            ctypes.c_uint(RENAME_EXCL),
        )
        if result != 0:
            raise exclusive_finalize_error(dest, ctypes.get_errno())
        return

    raise R2Error(f"{dest}: exclusive finalization unsupported on this platform")


def remove_payload_contents_from_fd(payload: Path, payload_fd: int) -> None:
    try:
        entries = sorted(os.listdir(payload_fd))
    except OSError as error:
        raise R2Error(f"{payload}: cannot list private payload for cleanup: {error}") from error

    for name in entries:
        path = payload / name
        try:
            info = os.stat(name, dir_fd=payload_fd, follow_symlinks=False)
        except OSError as error:
            raise R2Error(f"{path}: cannot stat private payload entry for cleanup: {error}") from error
        if stat.S_ISLNK(info.st_mode):
            raise R2Error(f"{path}: symlink not allowed in private payload")
        if stat.S_ISDIR(info.st_mode):
            raise R2Error(f"{path}: unexpected directory in private payload")
        if is_reparse_point(info):
            raise R2Error(f"{path}: reparse point not allowed in private payload")
        try:
            os.unlink(name, dir_fd=payload_fd)
        except OSError as error:
            raise R2Error(f"{path}: cannot remove private payload entry: {error}") from error


def remove_empty_private_container_unix(
    parent_fd: int,
    parent: Path,
    private_name: str,
    expected_private_identity: os.stat_result,
) -> None:
    private_path = parent / private_name
    current = child_directory_identity_by_name(parent_fd, parent, private_name)
    if not same_identity(expected_private_identity, current):
        raise R2Error(f"{private_path}: private publication directory changed; leaving it in place")

    private_fd = -1
    try:
        private_fd, opened = open_child_directory_fd_by_name(parent_fd, parent, private_name)
        if not same_identity(expected_private_identity, opened):
            raise R2Error(f"{private_path}: private publication directory changed; leaving it in place")
        try:
            entries = os.listdir(private_fd)
        except OSError as error:
            raise R2Error(f"{private_path}: cannot list private publication directory: {error}") from error
        if entries:
            raise R2Error(f"{private_path}: private publication directory is not empty")
    finally:
        if private_fd >= 0:
            os.close(private_fd)

    try:
        os.rmdir(private_name, dir_fd=parent_fd)
    except OSError as error:
        raise R2Error(f"{private_path}: cannot remove private publication directory: {error}") from error


def cleanup_private_publication_unix(
    parent_fd: int,
    parent: Path,
    private_name: str,
    expected_private_identity: os.stat_result,
    expected_payload_identity: os.stat_result | None,
) -> str | None:
    private_path = parent / private_name
    try:
        current_private = child_directory_identity_by_name(parent_fd, parent, private_name)
        if not same_identity(expected_private_identity, current_private):
            return f"{private_path}: private publication directory changed; leaving it in place"

        private_fd = -1
        payload_fd = -1
        try:
            private_fd, opened_private = open_child_directory_fd_by_name(
                parent_fd, parent, private_name
            )
            if not same_identity(expected_private_identity, opened_private):
                return f"{private_path}: private publication directory changed; leaving it in place"

            if expected_payload_identity is not None:
                payload_path = private_path / PAYLOAD_DIR_NAME
                current_payload = child_directory_identity_by_name(
                    private_fd, private_path, PAYLOAD_DIR_NAME
                )
                if not same_identity(expected_payload_identity, current_payload):
                    return f"{payload_path}: private payload directory changed; leaving it in place"
                payload_fd, opened_payload = open_child_directory_fd_by_name(
                    private_fd, private_path, PAYLOAD_DIR_NAME
                )
                if not same_identity(expected_payload_identity, opened_payload):
                    return f"{payload_path}: private payload directory changed; leaving it in place"
                remove_payload_contents_from_fd(payload_path, payload_fd)
                os.close(payload_fd)
                payload_fd = -1
                os.rmdir(PAYLOAD_DIR_NAME, dir_fd=private_fd)

            entries = os.listdir(private_fd)
            if entries:
                return f"{private_path}: private publication directory is not empty; leaving it in place"
        finally:
            if payload_fd >= 0:
                os.close(payload_fd)
            if private_fd >= 0:
                os.close(private_fd)

        os.rmdir(private_name, dir_fd=parent_fd)
    except R2Error as error:
        return f"{error}; leaving private publication in place"
    except OSError as error:
        return f"{private_path}: cleanup failed: {error}; leaving private publication in place"
    return None


def copy_file_to_windows_path(source: Path, dest: Path) -> None:
    handle, reason = open_regular_file(source)
    if reason:
        raise R2Error(f"{source}: {reason}")
    assert handle is not None

    try:
        output = create_windows_regular_file(dest)
    except R2Error:
        handle.close()
        raise

    try:
        with handle, output:
            shutil.copyfileobj(handle, output, COPY_CHUNK_SIZE)
    except OSError as error:
        raise R2Error(f"{dest}: cannot copy staged file: {error}") from error


def write_stamp_to_windows_path(dest: Path, source: str, revision: str) -> None:
    try:
        with create_windows_regular_file(dest / STAMP_NAME) as handle:
            handle.write(f"source={source}\nrevision={revision}\n".encode("utf-8"))
    except OSError as error:
        raise R2Error(f"{dest / STAMP_NAME}: cannot write stamp: {error}") from error


def validate_windows_rename_info_abi() -> None:
    if ctypes.sizeof(ctypes.c_void_p) != 8:
        raise R2Error("Windows FileRenameInfo ABI is only supported on 64-bit Python")
    if FILE_RENAME_INFO.RootDirectory.offset != 8:
        raise R2Error("Windows FileRenameInfo RootDirectory offset is unsupported")
    if FILE_RENAME_INFO.FileNameLength.offset != 16:
        raise R2Error("Windows FileRenameInfo FileNameLength offset is unsupported")
    if FILE_RENAME_INFO.FileName.offset != 20:
        raise R2Error("Windows FileRenameInfo FileName offset is unsupported")
    if ctypes.sizeof(FILE_RENAME_INFO) != 24:
        raise R2Error("Windows FileRenameInfo size is unsupported")


def validate_windows_disposition_info_abi() -> None:
    if FILE_DISPOSITION_INFO.DeleteFile.offset != 0:
        raise R2Error("Windows FileDispositionInfo DeleteFile offset is unsupported")
    if ctypes.sizeof(FILE_DISPOSITION_INFO) != 1:
        raise R2Error("Windows FileDispositionInfo size is unsupported")


def open_windows_private_directory(path: Path) -> tuple[WindowsHandle, WindowsIdentity]:
    try:
        handle = windows_create_file_handle(
            path,
            DELETE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            "cannot open private directory",
        )
        identity = ensure_windows_directory(handle, path)
        return handle, identity
    except R2Error as error:
        try:
            handle.close()
        except UnboundLocalError:
            pass
        message = str(error)
        if message.startswith(f"{path}: "):
            raise
        raise R2Error(f"{path}: {message}") from error


def create_windows_private_directory(parent: Path, dest: Path) -> str:
    for _ in range(128):
        name = private_publish_name(dest)
        path = parent / name
        if kernel32.CreateDirectoryW(str(path), None):
            return name
        error = ctypes.get_last_error()
        if error == ERROR_ALREADY_EXISTS:
            continue
        raise R2Error(
            f"{path}: cannot create private publication directory: {windows_message(error)}"
        )
    raise R2Error(f"{dest}: cannot allocate a private publication directory")


def create_windows_payload_directory(private: Path) -> tuple[WindowsHandle, WindowsIdentity]:
    payload = private / PAYLOAD_DIR_NAME
    if not kernel32.CreateDirectoryW(str(payload), None):
        error = ctypes.get_last_error()
        if error == ERROR_ALREADY_EXISTS:
            raise R2Error(f"{payload}: payload directory already exists")
        raise R2Error(f"{payload}: cannot create payload directory: {windows_message(error)}")
    return open_windows_private_directory(payload)


def windows_file_rename_info(dest_name: str, parent_handle: WindowsHandle):
    validate_windows_rename_info_abi()
    encoded_name = dest_name.encode("utf-16-le")
    total_size = FILE_RENAME_INFO.FileName.offset + len(encoded_name)
    buffer = ctypes.create_string_buffer(total_size)
    info = ctypes.cast(buffer, ctypes.POINTER(FILE_RENAME_INFO)).contents
    info.ReplaceIfExists = 0
    info.RootDirectory = parent_handle.handle
    info.FileNameLength = len(encoded_name)
    ctypes.memmove(
        ctypes.addressof(buffer) + FILE_RENAME_INFO.FileName.offset,
        encoded_name,
        len(encoded_name),
    )
    return buffer, total_size


def finalize_payload_directory_windows(
    payload_handle: WindowsHandle,
    parent_handle: WindowsHandle,
    dest: Path,
) -> None:
    buffer, size = windows_file_rename_info(dest.name, parent_handle)
    if kernel32.SetFileInformationByHandle(
        payload_handle.handle,
        FILE_RENAME_INFO_CLASS,
        buffer,
        size,
    ):
        return
    error = ctypes.get_last_error()
    if error in {ERROR_FILE_EXISTS, ERROR_ALREADY_EXISTS}:
        raise R2Error(f"{dest}: destination appeared before publication")
    raise R2Error(f"{dest}: cannot finalize publication: {windows_message(error)}")


def delete_windows_regular_file(path: Path) -> None:
    validate_windows_disposition_info_abi()
    handle = windows_create_file_handle(
        path,
        DELETE | FILE_READ_ATTRIBUTES,
        0,
        OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT,
        "cannot open private payload entry for cleanup",
    )
    try:
        ensure_windows_regular_file(handle, path)
        disposition = FILE_DISPOSITION_INFO(True)
        if not kernel32.SetFileInformationByHandle(
            handle.handle,
            FILE_DISPOSITION_INFO_CLASS,
            ctypes.byref(disposition),
            ctypes.sizeof(disposition),
        ):
            raise R2Error(
                f"{path}: cannot remove private payload entry: {windows_last_error('delete failed')}"
            )
    finally:
        handle.close()


def mark_windows_directory_for_delete(
    path: Path,
    handle: WindowsHandle,
    identity: WindowsIdentity,
    expected: WindowsIdentity,
) -> None:
    validate_windows_disposition_info_abi()
    if not same_identity(expected, identity):
        raise R2Error(f"{path}: private publication directory changed; leaving it in place")
    disposition = FILE_DISPOSITION_INFO(True)
    if not kernel32.SetFileInformationByHandle(
        handle.handle,
        FILE_DISPOSITION_INFO_CLASS,
        ctypes.byref(disposition),
        ctypes.sizeof(disposition),
    ):
        raise R2Error(f"{path}: cannot remove private directory: {windows_last_error('delete failed')}")


def cleanup_private_publication_windows(
    parent: Path,
    private_name: str,
    expected_private_identity: WindowsIdentity,
    expected_payload_identity: WindowsIdentity | None,
) -> str | None:
    private = parent / private_name
    payload = private / PAYLOAD_DIR_NAME
    private_handle = None
    payload_handle = None
    try:
        private_handle, private_identity = open_windows_private_directory(private)
        if not same_identity(expected_private_identity, private_identity):
            return f"{private}: private publication directory changed; leaving it in place"

        if expected_payload_identity is not None:
            payload_handle, payload_identity = open_windows_private_directory(payload)
            if not same_identity(expected_payload_identity, payload_identity):
                return f"{payload}: private payload directory changed; leaving it in place"
            for child in sorted(payload.iterdir(), key=lambda path: path.name):
                info = child.lstat()
                if stat.S_ISDIR(info.st_mode):
                    return f"{child}: unexpected directory in private payload; leaving it in place"
                if stat.S_ISLNK(info.st_mode) or is_reparse_point(info):
                    return f"{child}: reparse point in private payload; leaving it in place"
                delete_windows_regular_file(child)
            mark_windows_directory_for_delete(
                payload, payload_handle, payload_identity, expected_payload_identity
            )
            payload_handle.close()
            payload_handle = None

        if any(private.iterdir()):
            return f"{private}: private publication directory is not empty; leaving it in place"
        mark_windows_directory_for_delete(
            private, private_handle, private_identity, expected_private_identity
        )
        private_handle.close()
        private_handle = None
    except (OSError, R2Error) as error:
        return f"{error}; leaving private publication in place"
    finally:
        if payload_handle is not None:
            payload_handle.close()
        if private_handle is not None:
            private_handle.close()
    return None


def ensure_destination_absent_from_parent(parent_fd: int, dest: Path) -> None:
    try:
        os.stat(dest.name, dir_fd=parent_fd, follow_symlinks=False)
    except FileNotFoundError:
        return
    except OSError as error:
        raise R2Error(f"{dest}: cannot stat destination: {error}") from error
    raise R2Error(f"{dest}: destination appeared before publication")


def validate_private_contents_windows(
    root: Path,
    expected_names: list[str],
    expected_checksums: dict[str, str] | None,
) -> None:
    names = staged_pdf_names_windows(root)
    if expected_checksums is not None:
        validate_expected_names(root, names, expected_checksums)
        for name, expected_digest in expected_checksums.items():
            actual_digest, reason = sha256_file(root / name)
            if reason:
                raise R2Error(f"{root / name}: {reason}")
            if actual_digest != expected_digest:
                raise R2Error(
                    f"{root / name}: sha256 mismatch: expected {expected_digest}, got {actual_digest}"
                )
        return

    validate_expected_names(root, names, {name: "" for name in expected_names})


def checked_manifest(path: Path | None) -> dict[str, str] | None:
    if path is None:
        return None
    return checksum_manifest_or_error(path)


def publish_staged_set_unix(
    staging: Path,
    dest: Path,
    source: str,
    revision: str,
    expected_checksums: dict[str, str] | None,
) -> None:
    staging_fd = -1
    parent_fd = -1
    private_fd = -1
    payload_fd = -1
    private_name: str | None = None
    private_identity: os.stat_result | None = None
    payload_identity: os.stat_result | None = None
    payload_under_private = False
    try:
        staging_fd, _ = open_directory_fd(staging)
        names = staged_pdf_names_from_fd(staging, staging_fd)
        if expected_checksums is not None:
            validate_expected_names(staging, names, expected_checksums)

        parent_info = destination_parent_identity(dest)
        assert parent_info is not None
        try:
            state = destination_state(dest)
        except R2Error as error:
            raise R2Error(f"{dest}: destination is not publishable: {error}") from error
        if state != "absent":
            raise R2Error(f"{dest}: destination already exists")

        parent_fd, parent_opened = open_destination_parent_fd(dest, parent_info)
        ensure_destination_absent_from_parent(parent_fd, dest)

        private_name = create_private_directory_from_parent(parent_fd, dest)
        private_path = dest.parent / private_name
        private_fd, private_identity = open_child_directory_fd_by_name(
            parent_fd, dest.parent, private_name
        )
        payload_fd, payload_identity = create_payload_directory_from_private(
            private_fd, private_path
        )
        payload_under_private = True

        for name in names:
            copy_file_from_fd_to_fd(staging_fd, staging, name, payload_fd)
        validate_private_contents_from_fd(
            private_path / PAYLOAD_DIR_NAME,
            payload_fd,
            names,
            expected_checksums,
        )
        write_stamp_to_fd(payload_fd, source, revision)

        after_payload = child_directory_identity_by_name(
            private_fd, private_path, PAYLOAD_DIR_NAME
        )
        if not same_identity(payload_identity, after_payload):
            raise R2Error(f"{private_path / PAYLOAD_DIR_NAME}: payload directory changed during publication")

        finalize_payload_directory_unix(private_fd, parent_fd, dest)
        payload_under_private = False
        after = child_directory_identity(parent_fd, dest)
        if not same_identity(payload_identity, after):
            raise R2Error(f"{dest}: destination directory changed during publication")
        os.close(payload_fd)
        payload_fd = -1
        remove_empty_private_container_unix(
            parent_fd, dest.parent, private_name, private_identity
        )
        private_name = None
        if reason := ensure_same_directory_identity(
            dest.parent, parent_opened, "parent directory changed during publication"
        ):
            raise R2Error(f"{dest.parent}: {reason}")
    except Exception as error:
        if payload_fd >= 0:
            os.close(payload_fd)
            payload_fd = -1
        if private_fd >= 0:
            os.close(private_fd)
            private_fd = -1
        if (
            parent_fd >= 0
            and private_name is not None
            and private_identity is not None
        ):
            cleanup_message = cleanup_private_publication_unix(
                parent_fd,
                dest.parent,
                private_name,
                private_identity,
                payload_identity if payload_under_private else None,
            )
            if cleanup_message:
                raise R2Error(f"{error}; {cleanup_message}") from error
        raise
    finally:
        if payload_fd >= 0:
            os.close(payload_fd)
        if private_fd >= 0:
            os.close(private_fd)
        if parent_fd >= 0:
            os.close(parent_fd)
        if staging_fd >= 0:
            os.close(staging_fd)


def publish_staged_set_windows(
    staging: Path,
    dest: Path,
    source: str,
    revision: str,
    expected_checksums: dict[str, str] | None,
) -> None:
    staging_handle = None
    parent_handle = None
    private_handle = None
    payload_handle = None
    private_name: str | None = None
    private_identity: WindowsIdentity | None = None
    payload_identity: WindowsIdentity | None = None
    payload_under_private = False
    try:
        validate_windows_rename_info_abi()
        staging_handle, _ = open_windows_directory(staging)
        names = staged_pdf_names_windows(staging)
        if expected_checksums is not None:
            validate_expected_names(staging, names, expected_checksums)

        parent_handle, parent_identity = open_windows_directory(dest.parent)
        try:
            state = destination_state(dest)
        except R2Error as error:
            raise R2Error(f"{dest}: destination is not publishable: {error}") from error
        if state != "absent":
            raise R2Error(f"{dest}: destination already exists")

        attributes, reason = windows_path_attributes(dest)
        if reason is None:
            assert attributes is not None
            if attributes & FILE_ATTRIBUTE_REPARSE_POINT:
                raise R2Error(f"{dest}: reparse point not allowed")
            raise R2Error(f"{dest}: destination appeared before publication")
        if reason != "missing file":
            raise R2Error(f"{dest}: cannot stat destination: {reason}")

        private_name = create_windows_private_directory(dest.parent, dest)
        private_path = dest.parent / private_name
        private_handle, private_identity = open_windows_private_directory(private_path)
        payload_handle, payload_identity = create_windows_payload_directory(private_path)
        payload_under_private = True
        payload_path = private_path / PAYLOAD_DIR_NAME
        for name in names:
            copy_file_to_windows_path(staging / name, payload_path / name)
        validate_private_contents_windows(payload_path, names, expected_checksums)
        write_stamp_to_windows_path(payload_path, source, revision)

        after_payload_handle, after_payload_identity = open_windows_private_directory(payload_path)
        after_payload_handle.close()
        if not same_identity(payload_identity, after_payload_identity):
            raise R2Error(f"{payload_path}: payload directory changed during publication")

        finalize_payload_directory_windows(payload_handle, parent_handle, dest)
        payload_under_private = False
        payload_handle.close()
        payload_handle = None
        after_handle, after_identity = open_windows_directory(dest)
        after_handle.close()
        if not same_identity(payload_identity, after_identity):
            raise R2Error(f"{dest}: destination directory changed during publication")
        private_handle.close()
        private_handle = None
        cleanup_message = cleanup_private_publication_windows(
            dest.parent, private_name, private_identity, None
        )
        if cleanup_message:
            raise R2Error(cleanup_message)
        private_name = None

        parent_after_handle, parent_after_identity = open_windows_directory(dest.parent)
        parent_after_handle.close()
        if not same_identity(parent_identity, parent_after_identity):
            raise R2Error(f"{dest.parent}: parent directory changed during publication")
    except Exception as error:
        if payload_handle is not None:
            payload_handle.close()
            payload_handle = None
        if private_handle is not None:
            private_handle.close()
            private_handle = None
        if (
            private_name is not None
            and private_identity is not None
        ):
            cleanup_message = cleanup_private_publication_windows(
                dest.parent,
                private_name,
                private_identity,
                payload_identity if payload_under_private else None,
            )
            if cleanup_message:
                raise R2Error(f"{error}; {cleanup_message}") from error
        raise
    finally:
        if payload_handle is not None:
            payload_handle.close()
        if private_handle is not None:
            private_handle.close()
        if parent_handle is not None:
            parent_handle.close()
        if staging_handle is not None:
            staging_handle.close()


def publish_staged_set(
    staging: Path,
    dest: Path,
    source: str,
    revision: str,
    checksum_manifest_path: Path | None,
) -> None:
    validate_destination_name(dest)
    expected_checksums = checked_manifest(checksum_manifest_path)

    if can_publish_with_dir_fd():
        publish_staged_set_unix(staging, dest, source, revision, expected_checksums)
        return
    if has_windows_handles():
        publish_staged_set_windows(staging, dest, source, revision, expected_checksums)
        return

    raise R2Error(unsupported_backend_message("publication"))


def usage() -> int:
    print(
        f"usage: r2.py ids MANIFEST | check-dest DEST | publish STAGING DEST SOURCE REVISION CHECKSUM_MANIFEST|{NO_CHECKSUM_SENTINEL}",
        file=sys.stderr,
    )
    return 2


def main() -> int:
    if len(sys.argv) < 2:
        return usage()
    command = sys.argv[1]
    try:
        if command == "ids" and len(sys.argv) == 3:
            for identifier in manifest_ids(Path(sys.argv[2])):
                print(identifier)
            return 0
        if command == "check-dest" and len(sys.argv) == 3:
            print(destination_state(Path(sys.argv[2])))
            return 0
        if command == "publish" and len(sys.argv) == 7:
            checksum_manifest_path = (
                None if sys.argv[6] == NO_CHECKSUM_SENTINEL else Path(sys.argv[6])
            )
            publish_staged_set(
                Path(sys.argv[2]),
                Path(sys.argv[3]),
                sys.argv[4],
                sys.argv[5],
                checksum_manifest_path,
            )
            return 0
    except R2Error as error:
        print(f"r2: {error}", file=sys.stderr)
        return 1
    return usage()


if __name__ == "__main__":
    raise SystemExit(main())
