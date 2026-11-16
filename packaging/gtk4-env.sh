#!/bin/sh
# linuxdeploy's AppRun sources hooks from AppDir/apprun-hooks.
# GTK 4 needs schemas and icons in addition to its shared libraries.
if [ -n "${APPDIR:-}" ]; then
  if [ -d "$APPDIR/usr/share/glib-2.0/schemas" ]; then
    export GSETTINGS_SCHEMA_DIR="$APPDIR/usr/share/glib-2.0/schemas"
  fi
  export XDG_DATA_DIRS="$APPDIR/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
fi

# linuxdeploy changes the icon search environment. Cursors must still be
# resolved from the host Plasma theme, including user-installed themes.
# Plasma's kcminputrc is authoritative on KDE; GTK's settings.ini is used
# when KDE settings are unavailable (or on other desktop environments).
read_cursor_theme() {
  awk -v section="$1" -v key="$2" '
    /^\[[^]]+\]/ {
      current = $0
      sub(/^\[/, "", current)
      sub(/\].*$/, "", current)
      next
    }
    current == section && index($0, "=") {
      name = substr($0, 1, index($0, "=") - 1)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", name)
      if (name == key) {
        value = substr($0, index($0, "=") + 1)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
        if (length(value)) print value
        exit
      }
    }
  ' "$3"
}

if [ -z "${XCURSOR_THEME:-}" ]; then
  config_home="${XDG_CONFIG_HOME:-${HOME:-}/.config}"
  gtk_settings="$config_home/gtk-4.0/settings.ini"
  kde_settings="$config_home/kcminputrc"
  cursor_theme=""
  case ":${XDG_CURRENT_DESKTOP:-}:" in
    *:KDE:*)
      if [ -r "$kde_settings" ]; then
        cursor_theme="$(read_cursor_theme Mouse cursorTheme "$kde_settings")"
      fi
      ;;
  esac
  if [ -z "$cursor_theme" ] && [ -r "$gtk_settings" ]; then
    cursor_theme="$(read_cursor_theme Settings gtk-cursor-theme-name "$gtk_settings")"
  fi
  if [ -z "$cursor_theme" ] && [ -r "$kde_settings" ]; then
    cursor_theme="$(read_cursor_theme Mouse cursorTheme "$kde_settings")"
  fi
  if [ -n "$cursor_theme" ]; then
    export XCURSOR_THEME="$cursor_theme"
  fi
fi

if [ -z "${XCURSOR_PATH:-}" ]; then
  # Put host themes ahead of AppImage's bundled fallback Adwaita icons.
  export XCURSOR_PATH="${HOME:-}/.icons:${XDG_DATA_HOME:-${HOME:-}/.local/share}/icons:${HOME:-}/.local/share/icons:/usr/local/share/icons:/usr/share/icons:${APPDIR:-}/usr/share/icons:/usr/share/pixmaps"
fi
