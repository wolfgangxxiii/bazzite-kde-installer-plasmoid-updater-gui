#!/usr/bin/env bash
set -euo pipefail

APP_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
APPIMAGE="${1:-dist/Bazzite-Plasmoid-Updater-GUI-v${APP_VERSION}-x86_64.AppImage}"
mkdir -p ui-smoke
app_pid=""
trap 'if [[ -n "$app_pid" ]]; then kill -TERM -- "-$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi' EXIT

capture_page() {
  local page="$1" width="$2" height="$3" label="$4"
  local window_id=""
  local accent_config=""
  local -a appearance_env=()
  local expand_tests=0 expand_advanced=0 expand_dashboard=0
  if [[ "$label" == narrow-automation-tests || "$label" == desktop-automation-tests ]]; then
    expand_tests=1
  elif [[ "$label" == narrow-automation-advanced ]]; then
    expand_advanced=1
  elif [[ "$label" == narrow-dashboard-expanders || "$label" == desktop-dashboard-checklist || "$label" == desktop-dashboard-accent ]]; then
    expand_dashboard=1
  fi

  if [[ "$label" == desktop-dashboard-accent || "$label" == desktop-about-accent ]]; then
    accent_config="$(mktemp -d)"
    printf '[General]\nAccentColor=213,95,180\n' > "$accent_config/kdeglobals"
    appearance_env=(XDG_CONFIG_HOME="$accent_config" XDG_CURRENT_DESKTOP=KDE)
  fi

  # An explicit initial page is deterministic across font themes and scaling.
  # AppImage's AppRun spawns a wrapped GUI process. A dedicated process group
  # lets us terminate the entire tree; otherwise the next GTK instance exits
  # after forwarding activation to the still-running first window.
  setsid env "${appearance_env[@]}" BAZZITE_GUI_SMOKE_PAGE="$page" \
    BAZZITE_GUI_SMOKE_EXPAND_TESTS="$expand_tests" \
    BAZZITE_GUI_SMOKE_EXPAND_ADVANCED="$expand_advanced" \
    BAZZITE_GUI_SMOKE_EXPAND_DASHBOARD="$expand_dashboard" \
    APPIMAGE_EXTRACT_AND_RUN=1 \
    GDK_BACKEND=x11 G_DEBUG=fatal-criticals \
    "$APPIMAGE" >"ui-smoke/${label}.log" 2>&1 &
  app_pid=$!

  for attempt in $(seq 1 50); do
    window_id="$(xdotool search --onlyvisible --name 'Bazzite' | head -n 1 || true)"
    if [[ -n "$window_id" ]]; then
      break
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      echo "AppImage exited before opening the ${label} window" >&2
      sed -n '1,100p' "ui-smoke/${label}.log" >&2
      exit 1
    fi
    sleep 0.2
  done

  if [[ -z "$window_id" ]]; then
    echo "AppImage did not open the ${label} window" >&2
    sed -n '1,100p' "ui-smoke/${label}.log" >&2
    exit 1
  fi

  xdotool windowsize "$window_id" "$width" "$height"
  xdotool windowmove "$window_id" 0 0
  sleep 1.5
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "AppImage crashed in the ${label} layout" >&2
    sed -n '1,100p' "ui-smoke/${label}.log" >&2
    exit 1
  fi
  if [[ "$expand_tests" == 1 || "$expand_advanced" == 1 || "$expand_dashboard" == 1 ]]; then
    # Scroll through opened panels and verify their contents are accessible.
    xdotool mousemove --window "$window_id" 330 410 click --repeat 20 --delay 50 5
    sleep 0.5
  fi
  scrot "ui-smoke/${label}.png"

  if [[ "$page" == automation && "$label" == desktop-automation ]]; then
    # Reproduce the user's scrolled settings view. Save stays visible below.
    xdotool mousemove --window "$window_id" 650 450 click --repeat 12 --delay 50 5
    sleep 0.5
    scrot 'ui-smoke/desktop-automation-scrolled.png'
  fi

  # A screenshot can succeed despite a card being drawn over its neighbour.
  # Treat libadwaita's allocation warning as a layout test failure.
  if grep -q 'exceeds AdwBreakpointBin height' "ui-smoke/${label}.log"; then
    echo "A widget overlaps its allocated card in the ${label} layout" >&2
    grep 'exceeds AdwBreakpointBin height' "ui-smoke/${label}.log" >&2
    exit 1
  fi

  kill -TERM -- "-$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  if [[ -n "$accent_config" ]]; then
    rm -r -- "$accent_config"
  fi
  sleep 0.3
}

capture_page dashboard 540 720 narrow-dashboard
capture_page dashboard 820 720 breakpoint-dashboard
for page in dashboard installation addons automation journal about; do
  capture_page "$page" 1020 740 "desktop-${page}"
done
capture_page automation 540 720 narrow-automation
capture_page automation 540 720 narrow-automation-tests
capture_page automation 540 720 narrow-automation-advanced
capture_page dashboard 540 720 narrow-dashboard-expanders
capture_page dashboard 1020 740 desktop-dashboard-checklist
capture_page dashboard 1020 740 desktop-dashboard-accent
capture_page about 1020 740 desktop-about-accent
capture_page automation 1020 740 desktop-automation-tests
capture_page installation 540 720 narrow-installation
capture_page about 540 720 narrow-about
