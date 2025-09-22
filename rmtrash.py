#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import glob
import ctypes
from ctypes import wintypes

# 定数定義 (ShellAPI)
FO_DELETE = 3
FOF_ALLOWUNDO = 0x0040
FOF_NOCONFIRMATION = 0x0010
# 必要に応じて FOF_SILENT = 0x0004 なども追加可能

# SHFILEOPSTRUCT 構造体
class SHFILEOPSTRUCTW(ctypes.Structure):
    _fields_ = [
        ("hwnd", wintypes.HWND),
        ("wFunc", wintypes.UINT),
        ("pFrom", wintypes.LPCWSTR),
        ("pTo", wintypes.LPCWSTR),
        ("fFlags", ctypes.c_uint16),
        ("fAnyOperationsAborted", wintypes.BOOL),
        ("hNameMappings", wintypes.LPVOID),
        ("lpszProgressTitle", wintypes.LPCWSTR),
    ]

# API 関数のロード
shell32 = ctypes.WinDLL("shell32", use_last_error=True)
SHFileOperationW = shell32.SHFileOperationW
SHFileOperationW.argtypes = [ctypes.POINTER(SHFILEOPSTRUCTW)]
SHFileOperationW.restype = ctypes.c_int

def glob_path_list(path_list):
    expanded = []
    for path in path_list:
        expanded += glob.glob(path)
    return expanded

def delete_file_to_recycle_bin(path_list):
    path_list = glob_path_list(path_list)
    if not path_list:
        return (0, False)

    # SHFileOperation に渡す文字列は二重の NULL 終端が必要
    from_buf = "\0".join(path_list) + "\0\0"

    file_op = SHFILEOPSTRUCTW()
    file_op.hwnd = 0
    file_op.wFunc = FO_DELETE
    file_op.pFrom = from_buf
    file_op.pTo = None
    file_op.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION
    file_op.fAnyOperationsAborted = False
    file_op.hNameMappings = None
    file_op.lpszProgressTitle = None

    ret = SHFileOperationW(ctypes.byref(file_op))
    return (ret, bool(file_op.fAnyOperationsAborted))

if __name__ == "__main__":
    path_list = sys.argv[1:]
    retcode, aborted = delete_file_to_recycle_bin(path_list)
    if retcode != 0:
        sys.stderr.write("failed to remove %s\n" % ",".join(path_list))
        sys.stderr.write("[info] error code=%d 0x%x\n" % (retcode, retcode))
    sys.exit(retcode)
