#define WIN32_LEAN_AND_MEAN

#include <windows.h>

#include <cstdlib>
#include <cstdio>
#include <malloc.h>
#include <memory.h>
#include <tchar.h>

#include <shellapi.h>

int DeleteFileToRecycleBin(LPCTSTR pszFile)
{
  int ret;
  size_t nLen;
  TCHAR* pszFrom;
  SHFILEOPSTRUCT sShFileOp;

  if(pszFile == NULL)
    return false;

  nLen = _tcslen(pszFile);
  if(nLen == 0)
    return false;

  pszFrom = new TCHAR[nLen + 10];
  if(pszFrom == NULL)
    return false;
  _tcscpy_s(pszFrom,nLen + 10,pszFile);

  pszFrom[nLen + 1] = NULL;

  ::ZeroMemory(&sShFileOp,sizeof(SHFILEOPSTRUCT));
  sShFileOp.wFunc = FO_DELETE;
  sShFileOp.pFrom = pszFrom;
  sShFileOp.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT;
  ret = ::SHFileOperation(&sShFileOp);

  delete pszFrom;

  return ret;
}

int  main(int argc, char** argv)
{
  int retcode = 0;

  for(int i=1; i<argc; i++) {
    char const* const fn = argv[i];
    int ret_i = DeleteFileToRecycleBin(fn);

    if(ret_i!=0) {
      fprintf(stderr, "failed to remove %s\n", fn);
      retcode |= ret_i;
    }

  }
  return retcode;
}