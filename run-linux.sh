set -e

if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo "[run.sh] Rust toolchain not found. Installing via rustup..."
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- https://sh.rustup.rs | sh -s -- -y
    else
        echo "[run.sh] Error: need curl or wget to install Rust." >&2
        exit 1
    fi
    . "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
    echo "[run.sh] Rust installed."
fi

MISSING=""

require_lib() {
    suffix="$1"
    found=""
    for dir in /usr/lib /usr/lib64 /lib /lib64 /usr/lib/x86_64-linux-gnu /usr/lib/aarch64-linux-gnu; do
        if ls "$dir"/"$suffix"* >/dev/null 2>&1; then
            found=1
            break
        fi
    done
    [ -n "$found" ] || MISSING="$MISSING $suffix"
}

require_lib libxkbcommon.so
require_lib libX11.so
require_lib libXcursor.so
require_lib libXrandr.so
require_lib libXi.so
require_lib libGL.so
require_lib libxcb.so
require_lib libwayland-client.so

if [ -n "$MISSING" ]; then
    echo "[run.sh] Detected possibly missing runtime libraries:$MISSING"
    if command -v apt-get >/dev/null 2>&1; then
        echo "[run.sh] Installing dependencies via apt (may prompt for sudo)..."
        sudo apt-get update
        sudo apt-get install -y \
            libxkbcommon0 libxkbcommon-x11-0 libx11-6 libxcb1 \
            libxcursor1 libxrandr2 libxi6 libgl1 libxext6 \
            libwayland-client0 libgles2 || true
    elif command -v dnf >/dev/null 2>&1; then
        echo "[run.sh] Installing dependencies via dnf (may prompt for sudo)..."
        sudo dnf install -y \
            libxkbcommon libX11 libxcb libXcursor \
            libXrandr libXi mesa-libGL libXext \
            wayland-libs-client || true
    elif command -v pacman >/dev/null 2>&1; then
        echo "[run.sh] Installing dependencies via pacman (may prompt for sudo)..."
        sudo pacman -Sy --noconfirm \
            libxkbcommon libx11 libxcb libxcursor \
            libxrandr libxi mesa wayland || true
    elif command -v zypper >/dev/null 2>&1; then
        echo "[run.sh] Installing dependencies via zypper (may prompt for sudo)..."
        sudo zypper --non-interactive install \
            libxkbcommon0 libxkbcommon-x11-0 libX11-6 libxcb1 \
            libXcursor1 libXrandr2 libXi6 Mesa-libGL1 \
            libXext6 libwayland-client0 || true
    else
        echo "[run.sh] Warning: could not detect a supported package manager."
        echo "[run.sh] Please install the X11/Wayland/OpenGL dev + runtime libraries manually."
    fi
fi
cd "$(dirname "$0")"

echo "[run.sh] Building the launcher (cargo build)..."
cargo build --release

echo "[run.sh] Starting SS14 Lynncher..."
exec ./target/release/ss14-launcher-rust "$@"
