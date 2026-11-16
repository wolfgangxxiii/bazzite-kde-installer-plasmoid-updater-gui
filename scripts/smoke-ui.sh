#!/usr/bin/env bash
set -euo pipefail

APPIMAGE="${1:-dist/Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage}"
mkdir -p ui-smoke

APPIMAGE_EXTRACT_AND_RUN=1 GDK_BACKEND=x11 G_DEBUG=fatal-criticals \
  "$APPIMAGE" >ui-smoke/app.log 2>&1 &
app_pid=$!
trap 'kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true' EXIT

window_id=""
for attempt in $(seq 1 40); do
  window_id="$(xdotool search --onlyvisible --name 'Plasmoid Updater' | head -n 1 || true)"
  if [[ -n "$window_id" ]]; then
    break
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo 'AppImage exited before opening a window' >&2
    sed -n '1,100p' ui-smoke/app.log >&2
    exit 1
  fi
  sleep 0.25
done

if [[ -z "$window_id" ]]; then
  echo 'AppImage did not open a visible window' >&2
  sed -n '1,100p' ui-smoke/app.log >&2
  exit 1
fi

for spec in '540 720 narrow' '820 720 breakpoint' '1020 740 desktop'; do
  read -r width height label <<<"$spec"
  xdotool windowsize "$window_id" "$width" "$height"
  xdotool windowmove "$window_id" 0 0
  sleep 2
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "AppImage crashed in the $label layout" >&2
    sed -n '1,100p' ui-smoke/app.log >&2
    exit 1
  fi
  scrot "ui-smoke/${label}.png"
done
