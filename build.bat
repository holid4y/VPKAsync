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
    pause
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
    pause
    exit /b 1
)

echo Detected Version: %VERSION%

cargo build --release
if %errorlevel% neq 0 (
    echo.
    echo ERROR: Cargo build failed.
    pause
    exit /b 1
)

if not exist dist mkdir dist
set "OUTFILE=VPKAsync_v%VERSION%.exe"

echo.
echo BUILD OK! Executable at: target\release\async_vpk.exe
copy /Y "target\release\async_vpk.exe" "dist\!OUTFILE!" >nul
if %errorlevel% neq 0 (
    echo ERROR: Failed to copy file into dist.
    pause
    exit /b 1
)

echo Created: dist\!OUTFILE!
echo ------------------------------------------------
pause
