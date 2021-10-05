#!/usr/bin/env python

import sys
from win32com.shell import shell, shellcon

def DeleteFileToRecycleBin(path_list):

  return shell.SHFileOperation(
    (0, shellcon.FO_DELETE, "\0".join(path_list) + "\0\0", None, shellcon.FOF_ALLOWUNDO|shellcon.FOF_NOCONFIRMATION|shellcon.FOF_SILENT)
    )

if __name__ == '__main__':

  path_list = sys.argv[1:]
  retcode, aborted = DeleteFileToRecycleBin(path_list)
  if retcode != 0:
    sys.stderr.write("failed to remove %s\n" % ",".join(path_list))
    sys.stderr.write("[info] error code=%d 0x%x\n" % (retcode, retcode))

  exit(retcode)
