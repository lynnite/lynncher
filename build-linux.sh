#!/bin/sh
set -e

cd "$(dirname "$0")"

echo "[build-linux] Building release binary..."
cargo build --release

VERSION=$(grep -E '^version' Cargo.toml | head -n1 | sed -E 's/.*= *"([^"]+)".*/\1/')
BIN="target/release/ss14-launcher-rust"
STAGE="target/deb-root"
DEB="target/lynncher-${VERSION}_amd64.deb"
ASSET_NAME="lynncher-linux-x86_64.deb"

echo "[build-linux] Packaging .deb (version $VERSION)..."

rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/share/applications" \
         "$STAGE/usr/share/pixmaps"

sed "s/__VERSION__/$VERSION/" packaging/control.template > "$STAGE/DEBIAN/control"
cp "$BIN" "$STAGE/usr/bin/lynncher"
cp packaging/usr/share/applications/lynncher.desktop "$STAGE/usr/share/applications/"
cp logo.png "$STAGE/usr/share/pixmaps/lynncher.png"

# Bundle the SS14.Loader binaries so the launcher can run standalone.
if [ -d "loader/bin_x64" ]; then
    mkdir -p "$STAGE/usr/bin/loader/bin_x64"
    cp -r loader/bin_x64/loader "$STAGE/usr/bin/loader/bin_x64/"
    cp loader/bin_x64/signing_key "$STAGE/usr/bin/loader/bin_x64/"
else
    echo "[build-linux] warning: no loader/bin_x64 directory found; launcher will download the loader at runtime." >&2
fi

if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --build --root-owner-group "$STAGE" "$DEB"
else
    echo "[build-linux] error: dpkg-deb not found. Install dpkg-dev." >&2
    exit 1
fi

cp "$DEB" "$ASSET_NAME"
echo "[build-linux] Built $DEB (also copied to $ASSET_NAME)"
echo "[build-linux] Done."

