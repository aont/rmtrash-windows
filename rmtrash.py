#!/usr/bin/env python
import ctypes
import sys
import enum

class SHFILEOPSTRUCTW(ctypes.Structure):
  _fields_ = [
    ("hwnd", ctypes.c_ulong),
    ("wFunc", ctypes.c_uint),
    ("pFrom", ctypes.c_wchar_p),
    ("pTo", ctypes.c_wchar_p),
    ("fFlags", ctypes.c_ushort),
    ("fAnyOperationsAborted", ctypes.c_int),
    ("hNameMappings", ctypes.c_void_p),
    ("lpszProgressTitle", ctypes.c_wchar_p),
  ]

class FO(enum.IntEnum):
  MOVE = 0x1
  COPY = 0x2
  DELETE = 0x3
  RENAME = 0x4


class FOF(enum.IntEnum):
  MULTIDESTFILES = 0x1
  CONFIRMMOUSE = 0x2
  SILENT = 0x4
  RENAMEONCOLLISION = 0x8
  NOCONFIRMATION = 0x10
  WANTMAPPINGHANDLE = 0x20
  ALLOWUNDO = 0x40
  FILESONLY = 0x80
  SIMPLEPROGRESS = 0x100
  NOCONFIRMMKDIR = 0x200
  NOERRORUI = 0x400
  NOCOPYSECURITYATTRIBS = 0x800
  NORECURSION = 0x1000
  NO_CONNECTED_ELEMENTS = 0x2000
  WANTNUKEWARNING = 0x4000
  NORECURSEREPARSE = 0x8000

shell32 = ctypes.WinDLL("shell32")
shell32.SHFileOperationW.restype = ctypes.c_int
shell32.SHFileOperationW.argtypes = (ctypes.POINTER(SHFILEOPSTRUCTW), )

def DeleteFileToRecycleBin(fn):
  
  sShFileOp = SHFILEOPSTRUCTW()
  sShFileOp.wFunc = FO.DELETE
  sShFileOp.pFrom = fn
  sShFileOp.fFlags = FOF.ALLOWUNDO | FOF.NOCONFIRMATION | FOF.NOERRORUI | FOF.SILENT
  return shell32.SHFileOperationW(sShFileOp)

if __name__ == '__main__':

  retcode_sum = 0  
  for i in range(1, len(sys.argv)):
    fn = sys.argv[i]
    retcode_i = DeleteFileToRecycleBin(fn)
    if retcode_i != 0:
      sys.stderr.write("failed to remove %s\n" % fn)
    retcode_sum |= retcode_i

  exit(retcode_sum)
