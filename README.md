# async_vpk

A small tool that turns a folder into VPK files for Source Engine and TF2.

It reads the files in a folder, builds the VPK structure, and writes the output in the format the game expects.

If the input folder is small, it creates one `.vpk` file.
If the folder is larger than 200 MB, it switches to multi-chunk mode automatically.

## Install

### 1. Download a release

If you just want to use the tool, grab the compiled build from the GitHub Releases page.

After downloading, run:

```bat
async_vpk.exe
```

### 2. Build from source

Only do this if you want to compile it yourself.

1. Install Rust: https://rustup.rs
2. Open this folder in a terminal.
3. Build the project:

```bat
cargo build --release
```

The executable will be in:

```text
target\release\async_vpk.exe
```

If you prefer the Windows script, run:

```bat
build.bat
```

## Usage

### GUI mode

Run the app without arguments:

```bat
async_vpk.exe
```

Then:
- select the input folder
- select the output folder
- choose the mode
- click Create VPK

The log box shows progress and the files that were created.

### CLI mode

```bat
async_vpk.exe "C:\path\to\your\folder"
```

Useful options:

```bat
async_vpk.exe --single "C:\path\to\your\folder"
async_vpk.exe --multi "C:\path\to\your\folder"
async_vpk.exe --output "C:\output" "C:\path\to\your\folder"
async_vpk.exe --threads 4 "C:\path\to\your\folder"
```

## How it works

The tool runs in three steps:

1. scan the folder and collect files
2. read files in parallel and calculate CRC32
3. write the final VPK files

For folders above 200 MB, multi-chunk mode is enforced so the tool stays within the practical limits of the format.

## Notes

- The tool targets VPK v1.
- It keeps the interface simple on purpose.
- If a file cannot be read, it is skipped and the error is shown in the log.

## Quick example

```bat
async_vpk.exe "C:\Program Files (x86)\Steam\steamapps\common\Team Fortress 2\tf\custom\my_mod"
```

That will generate the VPK files in the output folder you selected.
