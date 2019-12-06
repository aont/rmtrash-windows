#!/usr/bin/env python
import ctypes
import sys

class SHFILEOPSTRUCTW(ctypes.Structure):
  _fields_ = [
    ("hwnd", ctypes.c_void_p),
    ("wFunc", ctypes.c_uint),
    ("pFrom", ctypes.c_wchar_p),
    ("pTo", ctypes.c_wchar_p),
    ("fFlags", ctypes.c_ushort),
    ("fAnyOperationsAborted", ctypes.c_int),
    ("hNameMappings", ctypes.c_void_p),
    ("lpszProgressTitle", ctypes.c_wchar_p),
  ]

# FO_MOVE = 0x1
# FO_COPY = 0x2
FO_DELETE = 0x3
# FO_RENAME = 0x4

# FOF_MULTIDESTFILES = 0x1
# FOF_CONFIRMMOUSE = 0x2
FOF_SILENT = 0x4
# FOF_RENAMEONCOLLISION = 0x8
FOF_NOCONFIRMATION = 0x10
# FOF_WANTMAPPINGHANDLE = 0x20
FOF_ALLOWUNDO = 0x40
# FOF_FILESONLY = 0x80
# FOF_SIMPLEPROGRESS = 0x100
# FOF_NOCONFIRMMKDIR = 0x200
FOF_NOERRORUI = 0x400
# FOF_NOCOPYSECURITYATTRIBS = 0x800
# FOF_NORECURSION = 0x1000
# FOF_NO_CONNECTED_ELEMENTS = 0x2000
# FOF_WANTNUKEWARNING = 0x4000
# FOF_NORECURSEREPARSE = 0x8000

shell32 = ctypes.WinDLL("shell32")
shell32.SHFileOperationW.restype = ctypes.c_int
shell32.SHFileOperationW.argtypes = (ctypes.POINTER(SHFILEOPSTRUCTW), )

def DeleteFileToRecycleBin(fn):

  sShFileOp = SHFILEOPSTRUCTW()

  sShFileOp.hWnd = 0
  sShFileOp.wFunc = FO_DELETE
  sShFileOp.pFrom = fn
  sShFileOp.pTo = None
  sShFileOp.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT
  # sys.stderr.write("[info] flags=0x%x\n"%sShFileOp.fFlags)

  sShFileOp.fAnyOperationsAborted = 0
  sShFileOp.hNameMappings = None
  sShFileOp.lpszProgressTitle = None

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
