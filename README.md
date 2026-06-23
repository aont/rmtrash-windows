# rmtrash-windows

`rmtrash-windows` is a Windows Recycle Bin helper. The repository now contains a
native C command-line implementation for MSYS2 UCRT64 and MSVC and keeps the
original Python module for script reuse.

The native CLI uses Windows Shell APIs directly, so files are sent to the
Recycle Bin instead of being permanently removed.

> **Note:** The functionality is only available on Windows.

## Features

- Move files or directories to the Recycle Bin from the terminal.
- Inspect the total size and number of items currently stored in the Recycle Bin.
- Empty the Recycle Bin with optional terminal confirmation and sound control.
- Parse native CLI options with `getopt_long`.
- Reuse the original Python helpers from scripts when needed.

All operations forward the literal paths you provide. `rmtrash` normalizes paths
to absolute Windows paths, but it does not expand environment variables, `~`, or
wildcards by itself.

## MSYS2 UCRT64 build

Install the UCRT64 toolchain from an MSYS2 UCRT64 shell:

```bash
pacman -S --needed mingw-w64-ucrt-x86_64-gcc make
```

Build the native command:

```bash
git clone https://github.com/aont/rmtrash-windows.git
cd rmtrash-windows
make -f Makefile.mingw
```

This produces `rmtrash.exe` in the repository root.

Install it into `/ucrt64/bin`:

```bash
make -f Makefile.mingw install
```

Override the install prefix if needed:

```bash
make -f Makefile.mingw PREFIX=/usr/local install
```


## MSVC NMAKE build

Install Microsoft Visual Studio or Build Tools for Visual Studio, then open a
Developer Command Prompt so `cl` and `nmake` are available.

`rmtrash` uses `getopt_long`, which is not provided by MSVC. Before building,
clone and prepare [`getopt-msvc-helper`](https://github.com/aont/getopt-msvc-helper),
then add its `include` and `lib` directories to the MSVC environment.

```cmd
git clone https://github.com/aont/getopt-msvc-helper.git C:\src\getopt-msvc-helper
set INCLUDE=%INCLUDE%;C:\src\getopt-msvc-helper\include
set LIB=%LIB%;C:\src\getopt-msvc-helper\lib
```

Build the native command with NMAKE:

```cmd
git clone https://github.com/aont/rmtrash-windows.git
cd rmtrash-windows
nmake /f Makefile.msvc
```

This produces `rmtrash.exe` in the repository root. Remove generated files with:

```cmd
nmake /f Makefile.msvc clean
```

## Command-line usage

```bash
rmtrash [PATH ...]
rmtrash --status
rmtrash --empty [--confirm-empty] [--no-sound]
rmtrash --help
```

- Supplying one or more `PATH` values moves them to the Recycle Bin. The command
  exits successfully even if the user aborts the shell operation.
- `--status` prints the total item count and size currently stored in the
  Recycle Bin.
- `--empty` clears the Recycle Bin.
- `--confirm-empty` prompts in the terminal before emptying the Recycle Bin.
- `--no-sound` disables the Recycle Bin empty sound.
- `-v`, `--verbose` prints additional messages when moving files.
- `-h`, `--help` prints usage information.

Examples:

```bash
# Show statistics about the Recycle Bin.
rmtrash --status

# Move two files to the Recycle Bin.
rmtrash C:\\temp\\report.txt D:\\archive\\old.log

# Empty the Recycle Bin after confirming in the terminal.
rmtrash --empty --confirm-empty

# Empty the Recycle Bin without the sound effect.
rmtrash --empty --no-sound
```

## Native implementation notes

The C CLI calls these Windows APIs:

- `SHFileOperationW` with `FO_DELETE | FOF_ALLOWUNDO` for Recycle Bin moves.
- `SHQueryRecycleBinW` for status queries.
- `SHEmptyRecycleBinW` for emptying the Recycle Bin.

MSYS2 UCRT64 normally passes command-line arguments as UTF-8. The implementation
converts arguments to UTF-16 before calling the wide-character Windows APIs.
