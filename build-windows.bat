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

echo [build-windows] Bundling SS14.Loader binaries next to the executable...
rem The loader is bundled under loader\bin_x64 (co-located with the source
rem executable at the project root). The launcher finds it relative to the
rem running executable, so no extra copy step is needed beyond ensuring it is
rem present here next to the compiled exe at build time.
if exist "loader\bin_x64\loader" (
    if not exist "loader\bin_x64\signing_key" (
        echo [build-windows] warning: loader signing_key not found; launcher will download the loader at runtime.
    ) else (
        echo [build-windows] loader detected at loader\bin_x64
    )
) else (
    echo [build-windows] warning: no loader\bin_x64\loader directory found; launcher will download the loader at runtime.
)

echo [build-windows] Built lynncher-windows-x86_64.exe
echo [build-windows] Done.

