#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""rmtrash - Windows Recycle Bin helper.

The module provides both a command-line interface and callable helpers so the
functionality can be reused from other Python scripts.  Paths are not expanded;
the literal values supplied by the caller are forwarded to the Windows shell.

* ``--status`` shows the total size and number of items stored in the Recycle
  Bin using :func:`SHQueryRecycleBinW`.
* ``--empty`` empties the Recycle Bin using :func:`SHEmptyRecycleBinW`.
* Positional arguments are moved into the Recycle Bin via
  :func:`SHFileOperationW`.
"""

import argparse
import os
import sys
from dataclasses import dataclass
from typing import Iterable, List, Optional, Sequence

__all__ = [
    "IS_WINDOWS",
    "RecycleBinError",
    "RecycleBinStatus",
    "human_size",
    "normalize_paths_no_expand",
    "delete_file_to_recycle_bin",
    "query_recycle_bin",
    "empty_recycle_bin",
    "send_to_recycle_bin",
    "get_recycle_bin_status",
    "empty_recycle_bin_with_options",
    "main",
]


class RecycleBinError(RuntimeError):
    """Raised when an operation on the Windows Recycle Bin fails."""

    def __init__(self, message: str, *, code: Optional[int] = None) -> None:
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class RecycleBinStatus:
    """Container for the total size and item count stored in the Recycle Bin."""

    size_bytes: int
    items: int

    def human_readable_size(self) -> str:
        """Return the size formatted in a human friendly way (e.g. ``1.5GB``)."""

        return human_size(self.size_bytes)

IS_WINDOWS = os.name == "nt"

if IS_WINDOWS:
    import ctypes
    from ctypes import wintypes

    # ---------- WinAPI 定数 ----------
    FO_DELETE = 3
    FOF_ALLOWUNDO = 0x0040
    FOF_NOCONFIRMATION = 0x0010

    SHERB_NOCONFIRMATION = 0x00000001
    SHERB_NOPROGRESSUI   = 0x00000002
    SHERB_NOSOUND        = 0x00000004

    # ---------- ctypes 用意 ----------
    shell32 = ctypes.WinDLL("shell32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class SHFILEOPSTRUCTW(ctypes.Structure):
        _fields_ = [
            ("hwnd", wintypes.HWND),
            ("wFunc", wintypes.UINT),
            ("pFrom", wintypes.LPCWSTR),
            ("pTo", wintypes.LPCWSTR),
            ("fFlags", wintypes.UINT),
            ("fAnyOperationsAborted", wintypes.BOOL),
            ("hNameMappings", wintypes.LPVOID),
            ("lpszProgressTitle", wintypes.LPCWSTR),
        ]

    SHFileOperationW = shell32.SHFileOperationW
    SHFileOperationW.argtypes = [ctypes.POINTER(SHFILEOPSTRUCTW)]
    SHFileOperationW.restype  = ctypes.c_int

    class SHQUERYRBINFO(ctypes.Structure):
        _fields_ = [
            ("cbSize", wintypes.DWORD),
            ("i64Size", ctypes.c_ulonglong),
            ("i64NumItems", ctypes.c_ulonglong),
        ]

    SHQueryRecycleBinW = shell32.SHQueryRecycleBinW
    SHQueryRecycleBinW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(SHQUERYRBINFO)]
    SHQueryRecycleBinW.restype  = ctypes.c_long  # HRESULT

    SHEmptyRecycleBinW = shell32.SHEmptyRecycleBinW
    SHEmptyRecycleBinW.argtypes = [wintypes.HWND, wintypes.LPCWSTR, wintypes.DWORD]
    SHEmptyRecycleBinW.restype  = ctypes.c_long  # HRESULT

    FormatMessageW = kernel32.FormatMessageW
    FormatMessageW.argtypes = [wintypes.DWORD, wintypes.LPCVOID, wintypes.DWORD,
                               wintypes.DWORD, wintypes.LPWSTR, wintypes.DWORD, wintypes.LPVOID]
    FormatMessageW.restype  = wintypes.DWORD
    FORMAT_MESSAGE_FROM_SYSTEM = 0x00001000
    FORMAT_MESSAGE_IGNORE_INSERTS = 0x00000200

    def explain_win_error(code: int) -> str:
        """Return a human readable message for a Windows error code."""

        buf = ctypes.create_unicode_buffer(1024)
        n = FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            None,
            code,
            0,
            buf,
            len(buf),
            None,
        )
        if n:
            return buf.value.strip()
        return f"WinError {code}"
else:
    ctypes = None
    wintypes = None

    FO_DELETE = FOF_ALLOWUNDO = FOF_NOCONFIRMATION = 0
    SHERB_NOCONFIRMATION = SHERB_NOPROGRESSUI = SHERB_NOSOUND = 0

    def explain_win_error(code: int) -> str:
        return "This function is only available on Windows."


def _format_operation_error(operation: str, retcode: int) -> str:
    """Return a diagnostic message for a failed WinAPI call."""

    message = [f"{operation} failed (code=0x{retcode:x})"]
    if ctypes is not None:
        last_error = ctypes.get_last_error()
        if last_error:
            message.append(
                f"last error {last_error}: {explain_win_error(last_error)}"
            )
    return "; ".join(message)

def human_size(n: int) -> str:
    """Return ``n`` formatted as a human readable size string."""

    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024 or unit == "TB":
            return f"{n:.1f}{unit}"
        n /= 1024.0
    return f"{n:.1f}PB"

# ---------- パス正規化（ただし expandvars/expanduser は行わない） ----------
def normalize_paths_no_expand(path_list: Iterable[str]) -> List[str]:
    """Normalise ``path_list`` without expanding environment variables.

    The function returns absolute, normalised paths while keeping the
    original order and removing duplicates.
    """

    normalized: List[str] = []
    seen = set()
    for p in path_list:
        # ここで expandvars/expanduser は行わない（そのまま扱う）
        abs_p = os.path.abspath(p)
        norm_p = os.path.normpath(abs_p)
        if norm_p not in seen:
            normalized.append(norm_p)
            seen.add(norm_p)
    return normalized

if IS_WINDOWS:
    # ---------- 主機能: ファイルをごみ箱へ ----------
    def delete_file_to_recycle_bin(path_list, flags=FOF_ALLOWUNDO | FOF_NOCONFIRMATION):
        """Low level wrapper around :func:`SHFileOperationW`."""

        path_list = normalize_paths_no_expand(path_list)
        if not path_list:
            return (0, False)
        # SHFileOperation に渡す文字列は double-null terminated list
        from_buf = ("\0".join(path_list) + "\0\0")
        file_op = SHFILEOPSTRUCTW()
        file_op.hwnd = 0
        file_op.wFunc = FO_DELETE
        file_op.pFrom = from_buf
        file_op.pTo = None
        file_op.fFlags = flags
        file_op.fAnyOperationsAborted = False
        file_op.hNameMappings = None
        file_op.lpszProgressTitle = None
        ret = SHFileOperationW(ctypes.byref(file_op))
        return (ret, bool(file_op.fAnyOperationsAborted))

    # ---------- ごみ箱情報照会 ----------
    def query_recycle_bin():
        """Low level wrapper around :func:`SHQueryRecycleBinW`."""

        info = SHQUERYRBINFO()
        info.cbSize = ctypes.sizeof(SHQUERYRBINFO)
        hr = SHQueryRecycleBinW(None, ctypes.byref(info))
        return (hr, info.i64Size, info.i64NumItems)

    # ---------- ごみ箱を空にする ----------
    def empty_recycle_bin(no_confirmation=True, no_progress=True, no_sound=True):
        """Low level wrapper around :func:`SHEmptyRecycleBinW`."""

        flags = 0
        if no_confirmation:
            flags |= SHERB_NOCONFIRMATION
        if no_progress:
            flags |= SHERB_NOPROGRESSUI
        if no_sound:
            flags |= SHERB_NOSOUND
        hr = SHEmptyRecycleBinW(0, None, flags)
        return hr
else:
    def delete_file_to_recycle_bin(path_list, flags=0):
        """Placeholder for non-Windows platforms."""

        raise OSError("rmtrash is only available on Windows.")

    def query_recycle_bin():
        """Placeholder for non-Windows platforms."""

        raise OSError("rmtrash is only available on Windows.")

    def empty_recycle_bin(no_confirmation=True, no_progress=True, no_sound=True):
        """Placeholder for non-Windows platforms."""

        raise OSError("rmtrash is only available on Windows.")

# ---------- Public helpers ----------


def send_to_recycle_bin(
    paths: Sequence[str],
    *,
    allow_undo: bool = True,
    suppress_confirmation: bool = True,
) -> bool:
    """Move ``paths`` to the Windows Recycle Bin.

    Parameters
    ----------
    paths:
        Files or directories to move.  The values are not expanded and are
        forwarded to the Windows shell as-is.
    allow_undo:
        When ``True`` (the default) the delete operation allows the standard
        "undo" behaviour provided by the shell.
    suppress_confirmation:
        When ``True`` (the default) confirmation prompts are disabled.

    Returns
    -------
    bool
        ``True`` if the operation completed without being aborted by the user.

    Raises
    ------
    RecycleBinError
        If the underlying Windows API reports an error or the platform is not
        Windows.
    """

    flags = 0
    if allow_undo:
        flags |= FOF_ALLOWUNDO
    if suppress_confirmation:
        flags |= FOF_NOCONFIRMATION

    try:
        retcode, aborted = delete_file_to_recycle_bin(paths, flags=flags)
    except OSError as exc:
        raise RecycleBinError(str(exc)) from exc

    if retcode != 0:
        raise RecycleBinError(
            _format_operation_error("Moving files to the Recycle Bin", retcode),
            code=retcode,
        )

    return not aborted


def get_recycle_bin_status() -> RecycleBinStatus:
    """Return the current status of the Windows Recycle Bin."""

    try:
        hr, size, items = query_recycle_bin()
    except OSError as exc:
        raise RecycleBinError(str(exc)) from exc

    if hr != 0:
        raise RecycleBinError(
            _format_operation_error("Querying the Recycle Bin", hr),
            code=hr,
        )

    return RecycleBinStatus(size_bytes=size, items=items)


def empty_recycle_bin_with_options(
    *,
    require_confirmation: bool = False,
    show_progress: bool = False,
    play_sound: bool = True,
) -> None:
    """Empty the Recycle Bin with additional options.

    Parameters mirror the flags exposed by :func:`SHEmptyRecycleBinW`.  An
    exception is raised when the operation cannot be completed.
    """

    try:
        hr = empty_recycle_bin(
            no_confirmation=not require_confirmation,
            no_progress=not show_progress,
            no_sound=not play_sound,
        )
    except OSError as exc:
        raise RecycleBinError(str(exc)) from exc

    if hr != 0:
        raise RecycleBinError(
            _format_operation_error("Emptying the Recycle Bin", hr),
            code=hr,
        )


# ---------- CLI ----------
#
# The helpers above are exposed for use as a Python module.  The CLI is a thin
# wrapper around them.


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    if not IS_WINDOWS:
        sys.stderr.write("This script runs on Windows only.\n")
        return 1
    p = argparse.ArgumentParser(
        description="Recycle Bin helper for Windows (no expandvars/expanduser, no wildcard checks)."
    )
    p.add_argument("paths", nargs="*", help="Literal files/directories (no expansion is performed).")
    p.add_argument("--status", action="store_true", help="Show recycle bin total size and item count (SHQueryRecycleBinW)")
    p.add_argument("--empty", action="store_true", help="Empty the recycle bin (SHEmptyRecycleBinW). WARNING: uses NOCONFIRMATION|NOPROGRESSUI by default.")
    p.add_argument("--no-sound", action="store_true", help="When emptying, suppress sound (maps to SHERB_NOSOUND).")
    p.add_argument("--confirm-empty", action="store_true", help="When used with --empty, prompt before emptying (overrides NOCONFIRMATION).")
    p.add_argument("-v", "--verbose", action="count", default=0)
    args = p.parse_args(argv)

    # --status
    if args.status:
        try:
            status = get_recycle_bin_status()
        except RecycleBinError as exc:
            sys.stderr.write(f"{exc}\n")
            return exc.code or 1
        print("Recycle Bin status:")
        print(f"  items : {status.items}")
        print(
            f"  size  : {status.size_bytes} bytes ({status.human_readable_size()})"
        )

    # --empty
    if args.empty:
        no_confirmation = not args.confirm_empty
        no_sound = args.no_sound

        if not no_confirmation:
            resp = input("Empty the Recycle Bin now? This cannot be undone. [y/N]: ").strip().lower()
            if resp not in ("y", "yes"):
                print("Aborted by user.")
                return 0

        try:
            empty_recycle_bin_with_options(
                require_confirmation=args.confirm_empty,
                show_progress=False,
                play_sound=not no_sound,
            )
        except RecycleBinError as exc:
            sys.stderr.write(f"{exc}\n")
            return exc.code or 1
        else:
            print("Recycle Bin emptied.")

    # delete paths -> recycle bin
    if args.paths:
        try:
            completed = send_to_recycle_bin(
                args.paths,
                allow_undo=True,
                suppress_confirmation=True,
            )
        except RecycleBinError as exc:
            sys.stderr.write(f"{exc}\n")
            return exc.code or 1
        else:
            if args.verbose:
                if completed:
                    print("Moved to recycle bin:", ", ".join(args.paths))
                else:
                    print("Recycle Bin operation was aborted by the shell.")
            return 0

    # 何も指定が無ければヘルプ表示
    if not (args.status or args.empty or args.paths):
        p.print_help()
        return 0

    return 0

if __name__ == "__main__":
    sys.exit(main())
