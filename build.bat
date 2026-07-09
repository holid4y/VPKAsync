@echo off
setlocal EnableDelayedExpansion

echo ------------------------------------------------
echo   Building VPKAsync for Windows...
echo ------------------------------------------------

where cargo >nul 2>&1
if errorlevel 1 (
    echo ERROR: 'cargo' was not found in PATH.
    echo Install Rust/Cargo from: https://rustup.rs/
    echo After installing, reopen the terminal and run this script again.
    if not defined CI pause
    exit /b 1
)

:: Extract version from Cargo.toml
set "VERSION="
for /f "tokens=2 delims==" %%A in ('findstr /b /c:"version =" Cargo.toml') do (
    set "VERSION=%%A"
)
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%

if "%VERSION%"=="" (
    echo ERROR: Could not read version from Cargo.toml.
    if not defined CI pause
    exit /b 1
)

echo Detected Version: %VERSION%

set "TARGET=x86_64-pc-windows-msvc"

where rustup >nul 2>&1
if not errorlevel 1 (
    rustup target add %TARGET% >nul
    if errorlevel 1 (
        echo.
        echo ERROR: Failed to install Rust target %TARGET%.
        if not defined CI pause
        exit /b 1
    )
)

cargo build --release --target %TARGET%
if errorlevel 1 (
    echo.
    echo ERROR: Cargo build failed.
    if not defined CI pause
    exit /b 1
)

set "EXE=target\%TARGET%\release\async_vpk.exe"
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\check_windows_imports.ps1" -Path "!EXE!" -StrictSystemOnly
if errorlevel 1 (
    echo.
    echo ERROR: Windows dependency check failed.
    if not defined CI pause
    exit /b 1
)

if not exist dist mkdir dist
set "OUTFILE=VPKAsync_v%VERSION%-Windows.exe"

echo.
echo Build OK. Executable at: !EXE!
copy /Y "!EXE!" "dist\!OUTFILE!" >nul
if errorlevel 1 (
    echo ERROR: Failed to copy file into dist.
    if not defined CI pause
    exit /b 1
)

echo Created: dist\!OUTFILE!
echo ------------------------------------------------
if not defined CI pause
