#!/bin/sh
set -e

cd "$(dirname "$0")"

echo "[build-rpm] Building release binary..."
cargo build --release

VERSION=$(grep -E '^version' Cargo.toml | head -n1 | sed -E 's/.*= *"([^"]+)".*/\1/')
BIN="target/release/ss14-launcher-rust"
ASSET_NAME="lynncher-linux-x86_64.rpm"

echo "[build-rpm] Packaging .rpm (version $VERSION)..."

if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "[build-rpm] error: rpmbuild not found. Install rpm-build." >&2
    exit 1
fi

RPMROOT="$HOME/rpmbuild"
mkdir -p "$RPMROOT/BUILD" "$RPMROOT/BUILDROOT" "$RPMROOT/RPMS" "$RPMROOT/SOURCES" "$RPMROOT/SPECS" "$RPMROOT/SRPMS"

SPEC="$RPMROOT/SPECS/lynncher.spec"
sed "s/@VERSION@/$VERSION/g" packaging/lynncher.spec.template > "$SPEC"

if ! grep -q "^Name:" "$SPEC"; then
    echo "[build-rpm] error: failed to generate spec file." >&2
    exit 1
fi

rm -f "$RPMROOT"/RPMS/*/lynncher-*.rpm

rpmbuild -bb "$SPEC"

find "$RPMROOT/RPMS" -name "lynncher-*.rpm" -type f | while read -r RPM; do
    cp "$RPM" "$ASSET_NAME"
    echo "[build-rpm] Built $RPM (also copied to $ASSET_NAME)"
done

echo "[build-rpm] Done."
