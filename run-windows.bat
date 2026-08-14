@echo off
setlocal
cd /d "%~dp0"

echo [run.bat] SS14 Lynncher Windows launcher

where cargo >nul 2>nul
if %errorlevel%==0 (
    where rustc >nul 2>nul
    if %errorlevel%==0 goto build
)

echo [run.bat] Rust toolchain not found. Installing via rustup...
where winget >nul 2>nul
if %errorlevel%==0 (
    echo [run.bat] Installing rustup using winget...
    winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
    if %errorlevel% neq 0 (
        echo [run.bat] winget install failed. Please install Rust manually from https://rustup.rs
        exit /b 1
    )
) else (
    echo [run.bat] winget not found. Downloading rustup-init.exe...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile '%TEMP%\rustup-init.exe'"
    if %errorlevel% neq 0 (
        echo [run.bat] Failed to download rustup-init.exe. Please install Rust from https://rustup.rs
        exit /b 1
    )
    "%TEMP%\rustup-init.exe" -y
    if %errorlevel% neq 0 (
        echo [run.bat] rustup-init failed.
        exit /b 1
    )
)

if exist "%USERPROFILE%\.cargo\env" call "%USERPROFILE%\.cargo\env"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

:build
echo [run.bat] Building the launcher (cargo build --release)...
cargo build --release
if %errorlevel% neq 0 (
    echo [run.bat] Build failed.
    exit /b 1
)

echo [run.bat] Starting SS14 Lynncher...
".\target\release\ss14-launcher-rust.exe" %*
