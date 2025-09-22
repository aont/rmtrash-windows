# rmtrash-windows

`rmtrash-windows` is a command-line tool **and** importable Python module that
lets you manage the Windows Recycle Bin without leaving the terminal. It uses
the Windows shell APIs underneath, so files are sent to the Recycle Bin instead
of being permanently removed.

> **Note:** The functionality is only available on Windows. Importing or running
> the CLI on other platforms raises an error.

## Features

- Move files or directories to the Recycle Bin from the terminal or your own
  Python scripts.
- Inspect the total size and number of items currently stored in the Recycle
  Bin.
- Empty the Recycle Bin with optional confirmation prompts and sound control.

All operations forward the literal paths you provide; environment variables and
``~`` are not expanded automatically.

## Installation

```bash
pip install git+https://github.com/aont/rmtrash-windows.git
```

After installation the ``rmtrash`` command becomes available.

## Command-line usage

```bash
rmtrash [PATH ...]
rmtrash --status
rmtrash --empty [--confirm-empty] [--no-sound]
```

- Supplying one or more ``PATH`` values moves them to the Recycle Bin. The
  command exits successfully even if the user aborts the shell operation.
- ``--status`` prints the total item count and size currently stored in the
  Recycle Bin.
- ``--empty`` clears the Recycle Bin. ``--confirm-empty`` adds an interactive
  confirmation step, while ``--no-sound`` disables the sound effect.

Example:

```bash
# Show statistics about the Recycle Bin.
rmtrash --status

# Move two files to the Recycle Bin.
rmtrash C:\\temp\\report.txt D:\\archive\\old.log

# Empty the Recycle Bin after confirming in the terminal.
rmtrash --empty --confirm-empty
```

## Python API

The same functionality can be reused from Python code:

```python
from rmtrash import (
    RecycleBinError,
    empty_recycle_bin_with_options,
    get_recycle_bin_status,
    send_to_recycle_bin,
)

status = get_recycle_bin_status()
print(f"Items: {status.items}, Size: {status.human_readable_size()}")

try:
    send_to_recycle_bin([r"C:\\temp\\report.txt"])
    empty_recycle_bin_with_options(play_sound=False)
except RecycleBinError as exc:
    print(f"Recycle Bin operation failed: {exc}")
```

- :func:`send_to_recycle_bin` moves files and directories to the Recycle Bin.
- :func:`get_recycle_bin_status` returns a ``RecycleBinStatus`` dataclass with
  the item count and total size. Use ``human_readable_size()`` for a friendly
  representation.
- :func:`empty_recycle_bin_with_options` empties the Recycle Bin while letting
  you control confirmation prompts, progress UI and sounds.
- Errors raise :class:`RecycleBinError`; the ``code`` attribute contains the
  underlying Windows return value when available.

These helpers all forward the literal paths you pass in and raise
``RecycleBinError`` if the current platform is not Windows.
