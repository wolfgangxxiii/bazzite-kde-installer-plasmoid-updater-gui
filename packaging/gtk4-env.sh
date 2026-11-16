#!/bin/sh
# linuxdeploy's AppRun sources hooks from AppDir/apprun-hooks.
# GTK 4 needs schemas and icons in addition to its shared libraries.
if [ -n "${APPDIR:-}" ]; then
  if [ -d "$APPDIR/usr/share/glib-2.0/schemas" ]; then
    export GSETTINGS_SCHEMA_DIR="$APPDIR/usr/share/glib-2.0/schemas"
  fi
  export XDG_DATA_DIRS="$APPDIR/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
fi
