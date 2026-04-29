# VPKAsync - Async VPK Compiler

A small tool that transforms a folder into VPK files for the Source Engine, created with Team Fortress 2 in mind.
It reads the files in a folder, builds the VPK structure, and writes the output in the format expected by the engine, the same as Valve's vpk.exe already does, but 4x faster.

If the input folder exceeds 600 MB, the tool automatically enables its multi-chunk mode.
In this mode, the output is split into multiple VPK files (`_000.vpk`, `_001.vpk`, etc.) along with a `_dir.vpk` index. It uses a **First-Fit Decreasing (FFD) algorithm** to perfectly pack files, generating significantly fewer chunks than the standard compiler.

## 1. Installation
Download a prebuilt binary from the [Releases](https://github.com/UnusualHatter/VPKAsync/releases) page.

After downloading, just run the executable:

`VPKAsync_v1.1.1.exe`

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
.\build.bat
```

## Usage

### GUI mode

Run the app without arguments:

```bat
VPKAsync_v1.1.1.exe
```

Then:
- Select the input folder
- Select the output folder (the app will remember your choice for next time)
- Choose the mode
- Click Create VPK

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

## Notes

- The tool targets VPK v1.
- It keeps the interface simple on purpose.
- It automatically prevents corrupted VPKs by halting if a single file (or a single VPK output mode) exceeds the 4.29 GB format limit.
- If a file cannot be read due to permission errors, it logs a clear warning in the interface.

## Quick example

```bat
async_vpk.exe "C:\Program Files (x86)\Steam\steamapps\common\Team Fortress 2\tf\custom\my_mod"
```

That will generate the VPK files in the output folder you selected.
