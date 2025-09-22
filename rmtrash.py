#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
recycle_cli_no_expand.py

- os.path.expandvars / os.path.expanduser を使わない版
- ワイルドカードのチェックも削除（そのまま渡された文字列を扱います）
- --status : ごみ箱の合計サイズ / アイテム数を表示 (SHQueryRecycleBinW)
- --empty  : ごみ箱を空にする (SHEmptyRecycleBinW)
"""

import argparse
import os
import sys

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
        buf = ctypes.create_unicode_buffer(1024)
        n = FormatMessageW(FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                           None, code, 0, buf, len(buf), None)
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

def human_size(n: int) -> str:
    for unit in ("B","KB","MB","GB","TB"):
        if n < 1024 or unit == "TB":
            return f"{n:.1f}{unit}"
        n /= 1024.0
    return f"{n:.1f}PB"

# ---------- パス正規化（ただし expandvars/expanduser は行わない） ----------
def normalize_paths_no_expand(path_list):
    """
    - 環境変数展開や ~ 展開は行わない
    - 引数として渡された文字列を絶対化・正規化し、重複を排除して返す
    """
    normalized = []
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
        info = SHQUERYRBINFO()
        info.cbSize = ctypes.sizeof(SHQUERYRBINFO)
        hr = SHQueryRecycleBinW(None, ctypes.byref(info))
        return (hr, info.i64Size, info.i64NumItems)

    # ---------- ごみ箱を空にする ----------
    def empty_recycle_bin(no_confirmation=True, no_progress=True, no_sound=True):
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
        raise OSError("rmtrash is only available on Windows.")

    def query_recycle_bin():
        raise OSError("rmtrash is only available on Windows.")

    def empty_recycle_bin(no_confirmation=True, no_progress=True, no_sound=True):
        raise OSError("rmtrash is only available on Windows.")

# ---------- CLI ----------
def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    if not IS_WINDOWS:
        sys.stderr.write("This script runs on Windows only.\n")
        return 1
    p = argparse.ArgumentParser(description="Recycle bin helper (no expandvars/expanduser, no wildcard checks).")
    p.add_argument("paths", nargs="*", help="Literal files/directories (no expansion is performed).")
    p.add_argument("--status", action="store_true", help="Show recycle bin total size and item count (SHQueryRecycleBinW)")
    p.add_argument("--empty", action="store_true", help="Empty the recycle bin (SHEmptyRecycleBinW). WARNING: uses NOCONFIRMATION|NOPROGRESSUI by default.")
    p.add_argument("--no-sound", action="store_true", help="When emptying, suppress sound (maps to SHERB_NOSOUND).")
    p.add_argument("--confirm-empty", action="store_true", help="When used with --empty, prompt before emptying (overrides NOCONFIRMATION).")
    p.add_argument("-v", "--verbose", action="count", default=0)
    args = p.parse_args(argv)

    # --status
    if args.status:
        hr, size, nitems = query_recycle_bin()
        if hr != 0:
            sys.stderr.write(f"Failed to query recycle bin: hr=0x{hr:x} {explain_win_error(ctypes.get_last_error())}\n")
            return 1
        print("Recycle Bin status:")
        print(f"  items : {nitems}")
        print(f"  size  : {size} bytes ({human_size(size)})")

    # --empty
    if args.empty:
        no_confirmation = not args.confirm_empty
        no_sound = args.no_sound

        if not no_confirmation:
            resp = input("Empty the Recycle Bin now? This cannot be undone. [y/N]: ").strip().lower()
            if resp not in ("y", "yes"):
                print("Aborted by user.")
                return 0

        hr = empty_recycle_bin(no_confirmation=no_confirmation, no_progress=True, no_sound=no_sound)
        if hr != 0:
            err_msg = explain_win_error(ctypes.get_last_error())
            sys.stderr.write(f"Failed to empty recycle bin: hr=0x{hr:x} {err_msg}\n")
            return 1
        else:
            print("Recycle Bin emptied.")

    # delete paths -> recycle bin
    if args.paths:
        try:
            retcode, aborted = delete_file_to_recycle_bin(args.paths, flags=FOF_ALLOWUNDO | FOF_NOCONFIRMATION)
        except Exception as e:
            sys.stderr.write(f"Argument error: {e}\n")
            return 2
        if retcode != 0:
            sys.stderr.write("failed to remove %s\n" % ",".join(args.paths))
            sys.stderr.write("[info] error code=%d 0x%x\n" % (retcode, retcode))
            err_msg = explain_win_error(ctypes.get_last_error())
            sys.stderr.write(f"[info] last error: {err_msg}\n")
            return retcode
        else:
            if args.verbose:
                print("Moved to recycle bin:", ", ".join(args.paths))
            return 0

    # 何も指定が無ければヘルプ表示
    if not (args.status or args.empty or args.paths):
        p.print_help()
        return 0

    return 0

if __name__ == "__main__":
    sys.exit(main())
