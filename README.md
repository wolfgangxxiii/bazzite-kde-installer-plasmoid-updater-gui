<div align="center">

<img src="assets/io.github.wolfgangxxiii.BazzitePlasmoidUpdater.svg" width="128" alt="Bazzite Plasmoid Updater GUI icon" />

# Bazzite Plasmoid Updater GUI

**Natywny instalator i menedżer aktualizacji dodatków KDE Plasma dla Bazzite**

Rust · GTK4 · libadwaita · AppImage · systemd --user

[![Build GTK4 AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml/badge.svg)](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

</div>

---

## Co potrafi?

- Pobiera oficjalne wydanie **[uwuclxdy/plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater)** dla x86_64.
- Weryfikuje pobrany plik za pomocą **SHA-256** przed instalacją.
- Instaluje narzędzie w `~/.local/bin` — bez `sudo` i bez zmiany obrazu Bazzite.
- Sprawdza aktualizacje, pokazuje symulację (`--dry-run`) i wykonuje aktualizacje dodatków użytkownika.
- Konfiguruje automatyczne aktualizacje co **6, 12 lub 24 godziny** przez timer `systemd --user`.
- Pozwala wykluczyć konkretne dodatki z aktualizacji.
- Pozwala zdecydować, czy po automatycznej aktualizacji restartować Plasma (domyślnie **nie**).
- Zapisuje dziennik działań i wyświetla błędy w interfejsie.

Program **nie** aktualizuje samego Bazzite, Flatpaków, sterowników ani pakietów systemowych. Do tych celów służą oficjalne mechanizmy Bazzite. Nie używa też opcji `plasmoid-updater --system`, która nie jest odpowiednia dla niezmiennego obrazu systemu.

## Instalacja AppImage

1. Otwórz [GitHub Actions → Build GTK4 AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml) i pobierz artefakt ostatniej pomyślnej kompilacji. Wydania oznaczone tagiem są także publikowane w [Releases](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases).
2. Rozpakuj artefakt ZIP, a następnie uruchom:

   ```bash
   chmod +x Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage
   ./Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage
   ```

3. W aplikacji kliknij **Zainstaluj / uaktualnij**.
4. Kliknij **Symuluj**, by zobaczyć potencjalne aktualizacje bez instalowania.
5. Wybierz harmonogram i **Zapisz i zastosuj**.

Wymagania środowiska: **Bazzite KDE Plasma 6, x86_64**, dostęp do Internetu i polecenia `curl`, `systemctl`, `kpackagetool6` oraz `bsdtar` przy aktualizacji dodatków. Jeżeli brakuje `bsdtar`, na Bazzite można użyć `brew install libarchive` (polecenie można udostępnić przez link do `~/.local/bin/bsdtar`).

## Zasada działania

| Element | Ścieżka / działanie |
|---|---|
| Binarka | `~/.local/bin/plasmoid-updater` |
| Ustawienia upstream | `~/.config/plasmoid-updater.toml` |
| Ustawienia GUI | `~/.config/bazzite-plasmoid-updater-gui/settings.toml` |
| Usługa | `~/.config/systemd/user/bazzite-plasmoid-updater.service` |
| Timer | `~/.config/systemd/user/bazzite-plasmoid-updater.timer` |

Po zmianie ustawień program przeładowuje konfigurację `systemd --user` oraz odpowiednio włącza lub wyłącza timer. `Persistent=true` pozwala nadrobić pominięte uruchomienie po kolejnym zalogowaniu, a `RandomizedDelaySec=20min` rozprasza czas aktualizacji.

Automatyzacja pracuje w sesji użytkownika, **nie uruchamia się stale jako usługa systemowa root**. Przy domyślnych ustawieniach nie restartuje Plasma; zaktualizowane efekty i motywy mogą wymagać ponownego zalogowania, aby zobaczyć zmiany.

Sprawdzenie stanu:

```bash
systemctl --user status bazzite-plasmoid-updater.timer
systemctl --user list-timers --all
journalctl --user -u bazzite-plasmoid-updater.service -n 50 --no-pager
```

Zatrzymanie automatyzacji:

```bash
systemctl --user disable --now bazzite-plasmoid-updater.timer
```

**Bezpieczeństwo:** GUI nie wykonuje poleceń przez `sh -c`, weryfikuje sumę SHA-256 przed podmianą programu i nie nadpisuje usług systemd, których samo nie utworzyło. Aktualizacje KDE Store są kodem innych autorów — włączenie automatycznego instalowania dodatków wiąże się z zaufaniem do ich dostawców.

## Kompilacja ze źródeł

Wymagane: Rust (stable), nagłówki GTK4/libadwaita, `pkg-config`.

Na Fedora/Bazzite środowisko programistyczne najlepiej zainstalować w Distrobox/Toolbx; kompilacja AppImage jest zautomatyzowana na Ubuntu 24.04 w GitHub Actions.

```bash
cargo build --release
cargo test
```

Aby utworzyć AppImage na systemie z wymaganymi bibliotekami i narzędziami do pakowania:

```bash
bash scripts/build-appimage.sh
```

Wynik: `dist/Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage`.

## Rozwój

To pierwsza wersja (`0.1.0`). Zgłoszenia błędów i propozycje funkcji są mile widziane przez [Issues](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues).

Projekt nie jest oficjalnie powiązany z Bazzite, KDE ani autorem `plasmoid-updater`.

**Licencja:** [GPL-3.0-or-later](LICENSE).
