#!/usr/bin/env python

import sys
from win32comext.shell import shell, shellcon
import glob

def glob_path_list(path_list):
    path_list_glob = []
    for path in path_list:
        path_list_glob += glob.glob(path)
    return path_list_glob

def delete_file_to_recycle_bin(path_list):

    path_list = glob_path_list(path_list)

    return shell.SHFileOperation(
        (
          0,
          shellcon.FO_DELETE,
          "\0".join(path_list) + "\0\0",
          None,
          shellcon.FOF_ALLOWUNDO | shellcon.FOF_NOCONFIRMATION
        )
        # shellcon.FOF_SILENT
    )

if __name__ == '__main__':

    path_list = sys.argv[1:]
    retcode, aborted = delete_file_to_recycle_bin(path_list)
    if retcode != 0:
        sys.stderr.write("failed to remove %s\n" % ",".join(path_list))
        sys.stderr.write("[info] error code=%d 0x%x\n" % (retcode, retcode))

    exit(retcode)
