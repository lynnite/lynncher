set -euo pipefail

cd "$(dirname "$0")"

REPO_ROOT="$(cd .. && pwd)"
LOADER_DIR="$REPO_ROOT/loader/bin_x64/loader"
OUT_DIR="src/bin/Release/net10.0"

echo "[patch-framework] building Lynncher.Patches..."
dotnet build src/Lynncher.Patches.csproj -c Release -v quiet

if [[ ! -f "$OUT_DIR/Lynncher.Patches.dll" ]]; then
    echo "[patch-framework] build produced no Lynncher.Patches.dll" >&2
    exit 1
fi

HARMONY="$(find "$HOME/.nuget/packages/lib.harmony" -path '*/lib/net10.0/0Harmony.dll' | sort | tail -n1)"
if [[ -z "$HARMONY" || ! -f "$HARMONY" ]]; then
    echo "[patch-framework] could not locate 0Harmony.dll (net10.0) from the NuGet cache" >&2
    exit 1
fi

mkdir -p "$LOADER_DIR"
cp "$OUT_DIR/Lynncher.Patches.dll" "$LOADER_DIR/Lynncher.Patches.dll"
cp "$HARMONY" "$LOADER_DIR/0Harmony.dll"

echo "[patch-framework] staged into $LOADER_DIR:"
echo "  - Lynncher.Patches.dll"
echo "  - 0Harmony.dll"

BUNDLE_OUT="$REPO_ROOT/patch-framework/bundles/_out"
mkdir -p "$BUNDLE_OUT"

resolve_client_path() {
    if [[ -n "${CLIENT_PATH:-}" ]]; then
        echo "$CLIENT_PATH"; return
    fi
    if [[ -d "$REPO_ROOT/RMC-14/bin/Content.Client" ]]; then
        echo "$REPO_ROOT/RMC-14"; return
    fi
    for base in \
        "$HOME/.local/share/Space Station 14/launcher-rust/clients" \
        "${XDG_DATA_HOME:-$HOME/.local/share}/Space Station 14/launcher-rust/clients"; do
        if [[ -d "$base" ]]; then
            local hit
            hit="$(find "$base" -maxdepth 3 -type d -name Content.Client 2>/dev/null | head -n1)"
            if [[ -n "$hit" ]]; then
                echo "$(dirname "$(dirname "$hit")")"; return
            fi
        fi
    done
    echo ""
}

CLIENT_PATH_RESOLVED="$(resolve_client_path)"

build_bundle() {
    local name="$1" proj="$2"
    if [[ -z "$CLIENT_PATH_RESOLVED" ]]; then
        echo "[patch-framework] skipping bundle '$name': no game client found." >&2
        echo "                  Set CLIENT_PATH to a built client (dir containing bin/Content.Client)." >&2
        return 0
    fi
    echo "[patch-framework] building bundle: $name (client: $CLIENT_PATH_RESOLVED)"
    dotnet build "$proj" -c Release -v quiet -p:ClientPath="$CLIENT_PATH_RESOLVED"
    local dll
    dll="$(find "$(dirname "$proj")/bin/Release" -maxdepth 2 -name "$name.dll" | head -n1)"
    if [[ -n "$dll" && -f "$dll" ]]; then
        cp "$dll" "$BUNDLE_OUT/$name.dll"
        echo "  -> $BUNDLE_OUT/$name.dll"
    else
        echo "  !! $name.dll not found after build" >&2
    fi
}

build_bundle "HitMarker" "bundles/HitMarker/src/HitMarker.csproj"
build_bundle "HealthBars" "bundles/HealthBars/src/HealthBars.csproj"

LOADER_DLL="$LOADER_DIR/SS14.Loader.dll"
if [[ -f "$LOADER_DLL" ]] && grep -qa "Lynncher.Patches" "$LOADER_DLL"; then
    echo "[patch-framework] loader is patch-capable (embeds Lynncher.Patches)."
else
    echo "[patch-framework] WARNING: $LOADER_DLL does not embed 'Lynncher.Patches'." >&2
    echo "                  Build it from SanabiLauncher/SS14.Loader (hook already wired" >&2
    echo "                  into SS14.Loader.csproj + Program.cs) and deploy it to" >&2
    echo "                  $LOADER_DIR, or the launcher will refuse to patch." >&2
fi

echo "[patch-framework] done."

