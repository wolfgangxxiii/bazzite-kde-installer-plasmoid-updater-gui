#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_root="$(mktemp -d)"
trap 'rm -r -- "$fixture_root"' EXIT
mkdir -p "$fixture_root/home/.config/gtk-4.0"

printf '[Settings]\ngtk-cursor-theme-name=Breeze_Snow\n' \
  >"$fixture_root/home/.config/gtk-4.0/settings.ini"
actual="$(env -u XDG_CONFIG_HOME -u XCURSOR_THEME -u XCURSOR_PATH HOME="$fixture_root/home" \
  sh -c '. "$1"; printf "%s|%s" "$XCURSOR_THEME" "$XCURSOR_PATH"' \
  _ "$project_root/packaging/gtk4-env.sh")"
[[ "$actual" == Breeze_Snow\|*"$fixture_root/home/.local/share/icons"* ]]

actual="$(env -u XDG_CONFIG_HOME HOME="$fixture_root/home" XCURSOR_THEME=UserChoice \
  XCURSOR_PATH=/custom/cursors sh -c \
  '. "$1"; printf "%s|%s" "$XCURSOR_THEME" "$XCURSOR_PATH"' \
  _ "$project_root/packaging/gtk4-env.sh")"
[[ "$actual" == 'UserChoice|/custom/cursors' ]]

rm "$fixture_root/home/.config/gtk-4.0/settings.ini"
printf '[Mouse]\ncursorTheme=Breeze\n' \
  >"$fixture_root/home/.config/kcminputrc"
actual="$(env -u XDG_CONFIG_HOME -u XCURSOR_THEME HOME="$fixture_root/home" \
  XDG_CURRENT_DESKTOP=KDE sh -c \
  '. "$1"; printf "%s" "$XCURSOR_THEME"' \
  _ "$project_root/packaging/gtk4-env.sh")"
[[ "$actual" == Breeze ]]

printf '[Settings]\ngtk-cursor-theme-name=Adwaita\n' \
  >"$fixture_root/home/.config/gtk-4.0/settings.ini"
actual="$(env -u XDG_CONFIG_HOME -u XCURSOR_THEME HOME="$fixture_root/home" \
  XDG_CURRENT_DESKTOP=KDE sh -c \
  '. "$1"; printf "%s" "$XCURSOR_THEME"' \
  _ "$project_root/packaging/gtk4-env.sh")"
[[ "$actual" == Breeze ]]

echo 'Cursor theme checks passed (GTK, override, KDE priority and fallback).'
