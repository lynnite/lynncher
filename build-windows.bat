@echo off
setlocal
cd /d "%~dp0"

echo [build-windows] Building release executable...
cargo build --release
if %errorlevel% neq 0 (
    echo [build-windows] Build failed.
    exit /b 1
)

echo [build-windows] Copying to asset name...
copy /y "target\release\ss14-launcher-rust.exe" "lynncher-windows-x86_64.exe" >nul
if %errorlevel% neq 0 (
    echo [build-windows] Failed to copy executable.
    exit /b 1
)

echo [build-windows] Built lynncher-windows-x86_64.exe
echo [build-windows] Done.
