@echo off
echo Compilando async_vpk...
cargo build --release
if %errorlevel% == 0 (
    echo.
    echo BUILD OK! Executavel em: target\release\async_vpk.exe
    copy target\release\async_vpk.exe async_vpk.exe
    echo Copiado para async_vpk.exe na pasta atual.
) else (
    echo ERRO na compilacao.
)
pause
