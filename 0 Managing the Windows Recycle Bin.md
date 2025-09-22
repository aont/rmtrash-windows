# Managing the Windows Recycle Bin

Managing the Windows Recycle Bin usually requires manual steps through the desktop interface. However, with Python, it is possible to automate this process and create a command-line tool. The script `recycle_cli_no_expand.py` demonstrates how to interact with the Recycle Bin directly using the Windows API through the `ctypes` library.

## Key Features

1. **Delete files to the Recycle Bin**
   Instead of permanently removing files, this tool sends them to the Recycle Bin using the Windows API (`SHFileOperationW`). It ensures that files can be recovered later if needed.

2. **Check Recycle Bin status**
   By calling `SHQueryRecycleBinW`, the script can display the total number of items and their combined size currently in the Recycle Bin.

3. **Empty the Recycle Bin**
   The tool allows users to clear the Recycle Bin completely with `SHEmptyRecycleBinW`. Options are available to suppress confirmation dialogs, progress windows, and sounds.

## Design Details

* The script avoids expanding environment variables (`os.path.expandvars`) and user directories (`os.path.expanduser`). It only processes the literal paths given by the user.
* Paths are normalized and duplicates removed before they are sent to the API.
* Error handling is built-in: if an API call fails, the tool fetches the corresponding Windows error message for clarity.
* The script uses command-line arguments to control its behavior. For example:

  * `--status` shows the Recycle Bin size and item count.
  * `--empty` clears it.
  * Providing file or folder paths moves them to the Recycle Bin.

## Usage Example

* To check the current Recycle Bin status:

  ```
  python recycle_cli_no_expand.py --status
  ```

* To empty the Recycle Bin without confirmation or sounds:

  ```
  python recycle_cli_no_expand.py --empty --no-sound
  ```

* To move files into the Recycle Bin:

  ```
  python recycle_cli_no_expand.py file1.txt folder2
  ```

## Conclusion

This script is a practical example of how Python can work closely with the Windows API. It provides a fast, command-line way to manage the Recycle Bin, making it useful for developers, system administrators, or anyone who prefers automation over manual cleanup.
