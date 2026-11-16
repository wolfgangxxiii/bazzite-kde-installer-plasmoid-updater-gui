#!/usr/bin/env bash
set -euo pipefail

# Build on Ubuntu 24.04 for an older glibc than Bazzite's.
# Unlike linuxdeploy-plugin-gtk (GTK2/3), use linuxdeploy's ELF dependency
# bundling and add the GTK4 runtime data manually.
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

ARCH="$(uname -m)"
if [[ "$ARCH" != "x86_64" ]]; then
  echo "AppImage currently supports x86_64 only" >&2
  exit 1
fi

for tool in curl rsvg-convert glib-compile-schemas; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing build dependency: $tool" >&2
    exit 1
  fi
done

cargo build --locked --release --bins

APPDIR="$PROJECT_ROOT/AppDir"
DISTDIR="$PROJECT_ROOT/dist"
mkdir -p "$DISTDIR" "$PROJECT_ROOT/.build-tools"
if [[ -e "$APPDIR" ]]; then
  echo "Please move or remove existing $APPDIR before packaging." >&2
  exit 1
fi

ICON_NAME="io.github.wolfgangxxiii.BazzitePlasmoidUpdater"
ICON_PNG="$PROJECT_ROOT/.build-tools/$ICON_NAME.png"
rsvg-convert --width 256 --height 256 \
  --output "$ICON_PNG" "assets/$ICON_NAME.svg"

LINUXDEPLOY="$PROJECT_ROOT/.build-tools/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_SHA256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
if [[ ! -f "$LINUXDEPLOY" ]]; then
  curl --fail --location --silent --show-error --retry 3 \
    "$LINUXDEPLOY_URL" \
    --output "$LINUXDEPLOY"
  chmod +x "$LINUXDEPLOY"
fi
printf '%s  %s\n' "$LINUXDEPLOY_SHA256" "$LINUXDEPLOY" | sha256sum --check --status || {
  echo "linuxdeploy SHA-256 mismatch; packaging aborted." >&2
  exit 1
}

export APPIMAGE_EXTRACT_AND_RUN=1
"$LINUXDEPLOY" --appdir "$APPDIR" \
  --executable "target/release/bazzite-plasmoid-updater-gui" \
  --executable "target/release/plasmoid-updater-agent" \
  --desktop-file "packaging/$ICON_NAME.desktop" \
  --icon-file "$ICON_PNG"

test -x "$APPDIR/usr/bin/plasmoid-updater-agent"

mkdir -p "$APPDIR/usr/share/glib-2.0/schemas" "$APPDIR/apprun-hooks"
install -Dm644 "packaging/$ICON_NAME.metainfo.xml" \
  "$APPDIR/usr/share/metainfo/$ICON_NAME.metainfo.xml"
cp /usr/share/glib-2.0/schemas/*.gschema.xml \
  "$APPDIR/usr/share/glib-2.0/schemas/"
glib-compile-schemas "$APPDIR/usr/share/glib-2.0/schemas"
cp "packaging/gtk4-env.sh" "$APPDIR/apprun-hooks/gtk4-env.sh"
chmod +x "$APPDIR/apprun-hooks/gtk4-env.sh"

if [[ -d /usr/share/icons/Adwaita ]]; then
  mkdir -p "$APPDIR/usr/share/icons"
  cp -a /usr/share/icons/Adwaita "$APPDIR/usr/share/icons/Adwaita"
fi

export OUTPUT="$DISTDIR/Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage"
"$LINUXDEPLOY" --appdir "$APPDIR" --output appimage
echo "AppImage available at $OUTPUT"
