#!/usr/bin/env python
import ctypes
import sys
import enum
import os

class SHFILEOPSTRUCTA(ctypes.Structure):
  _fields_ = [
    ("hwnd", ctypes.c_void_p),
    ("wFunc", ctypes.c_uint),
    ("pFrom", ctypes.c_char_p),
    ("pTo", ctypes.c_char_p),
    ("fFlags", ctypes.c_ushort),
    ("fAnyOperationsAborted", ctypes.c_int),
    ("hNameMappings", ctypes.c_void_p),
    ("lpszProgressTitle", ctypes.c_char_p),
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

shell32.SHFileOperationA.restype = ctypes.c_int
shell32.SHFileOperationA.argtypes = (ctypes.POINTER(SHFILEOPSTRUCTA), )

def DeleteFileToRecycleBin(fn_list):
  
  sShFileOp = SHFILEOPSTRUCTA()
  sShFileOp.wFunc = FO.DELETE
  
  sShFileOp.pFrom = b"\0".join([fn.encode("cp932") for fn in fn_list]) + b'\0\0'
  sShFileOp.fFlags = FOF.ALLOWUNDO | FOF.NOCONFIRMATION | FOF.NOERRORUI | FOF.SILENT
  return shell32.SHFileOperationA(sShFileOp)

if __name__ == '__main__':

  fn_list = sys.argv[1:]
  retcode = DeleteFileToRecycleBin(fn_list)
  if retcode != 0:
    sys.stderr.write("failed to remove %s\n" % ",".join(fn_list))
    sys.stderr.write("[info] error code=%d 0x%x\n" % (retcode, retcode))

  exit(retcode)
