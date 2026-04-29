@echo off
setlocal

echo Building async_vpk...
where cargo >nul 2>&1
if errorlevel 1 (
    echo.
    echo ERROR: 'cargo' was not found in PATH.
    echo Install Rust/Cargo from: https://rustup.rs/
    echo After installing, reopen the terminal and run this script again.
    pause
    exit /b 1
)

cargo build --release
if %errorlevel% == 0 (
    echo.
    echo BUILD OK! Executable at: target\release\async_vpk.exe
    copy /Y target\release\async_vpk.exe async_vpk.exe
    echo Copied to async_vpk.exe in the current folder.
) else (
    echo Build error.
)
pause
