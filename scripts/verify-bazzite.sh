#!/usr/bin/env bash
set -u

# Read-only, real-host verification after configuring the GUI on Bazzite KDE.
# Run as the desktop user, never through sudo.
problems=0

check_command() {
  if command -v "$1" >/dev/null 2>&1; then
    printf '✓ %s: %s\n' "$1" "$(command -v "$1")"
  else
    printf '✗ Brak: %s\n' "$1"
    problems=$((problems + 1))
  fi
}

if ! grep -qi bazzite /etc/os-release; then
  echo '✗ Ten test należy uruchomić na Bazzite.'
  problems=$((problems + 1))
fi
check_command curl
check_command kpackagetool6
check_command systemctl
if [[ -x "$HOME/.local/bin/bsdtar" || -x /usr/bin/bsdtar || -x /usr/local/bin/bsdtar || -x /home/linuxbrew/.linuxbrew/opt/libarchive/bin/bsdtar ]] || command -v bsdtar >/dev/null 2>&1; then
  echo '✓ bsdtar: dostępny'
else
  echo '✗ Brak: bsdtar'
  problems=$((problems + 1))
fi

if ! systemctl --user show-environment >/dev/null 2>&1; then
  echo '✗ Nie działa menedżer systemd --user.'
  problems=$((problems + 1))
fi

found=''
for path in "$HOME/.local/bin/plasmoid-updater" "$HOME/.cargo/bin/plasmoid-updater" /home/linuxbrew/.linuxbrew/bin/plasmoid-updater /usr/local/bin/plasmoid-updater /usr/bin/plasmoid-updater; do
  if [[ -x "$path" ]]; then
    found="$path"
    break
  fi
done
if [[ -z "$found" ]]; then
  found="$(command -v plasmoid-updater 2>/dev/null || true)"
fi

if [[ -n "$found" ]]; then
  "$found" --version
  echo 'Symulacja bez instalowania dodatków:'
  "$found" --skip-plasma-detection update --dry-run --no-restart-plasma || {
    echo '✗ Symulacja aktualizacji zgłosiła błąd.'
    problems=$((problems + 1))
  }
else
  echo '✗ Nie znaleziono plasmoid-updater.'
  problems=$((problems + 1))
fi

echo 'Stan timera:'
systemctl --user status bazzite-plasmoid-updater.timer --no-pager || {
  problems=$((problems + 1))
}
echo 'Następne zaplanowane uruchomienie:'
systemctl --user list-timers --all --no-pager bazzite-plasmoid-updater.timer || true

if [[ -x "$HOME/.local/bin/bazzite-plasmoid-updater-agent" ]]; then
  "$HOME/.local/bin/bazzite-plasmoid-updater-agent" --version
else
  echo '✗ Brak agenta systemd.'
  problems=$((problems + 1))
fi

if (( problems > 0 )); then
  echo "Test Bazzite: wykryto $problems problem(ów)."
  exit 1
fi
echo 'Test Bazzite: wszystkie podstawowe kontrole przeszły pomyślnie.'
