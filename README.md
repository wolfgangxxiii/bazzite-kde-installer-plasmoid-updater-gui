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
- **Samoczynnie aktualizuje również `plasmoid-updater`:** w każdym cyklu sprawdza nowe wydanie na GitHub, porównuje wersję i pobiera binarkę tylko wtedy, gdy jest nowsza.
- Konfiguruje automatyczne aktualizacje dodatków i narzędzia co **6, 12 lub 24 godziny** przez timer `systemd --user` — **bez konieczności uruchamiania GUI w tle**.
- Instaluje lekki, niezależny agent aktualizacji uruchamiany przez systemd; działa nawet po zamknięciu AppImage.
- Umożliwia oddzielne wyłączenie automatycznej aktualizacji samego `plasmoid-updater`.
- Gdy brakuje `bsdtar`, próbuje zainstalować `libarchive` przez Homebrew (`brew install libarchive`).
- Pozwala wykluczyć konkretne dodatki z aktualizacji.
- Pozwala zdecydować, czy po automatycznej aktualizacji restartować Plasma (domyślnie **nie**).
- Zapisuje dziennik działań i wyświetla błędy w interfejsie.

Program **nie** aktualizuje samego Bazzite, Flatpaków, sterowników ani pakietów systemowych. Do tych celów służą oficjalne mechanizmy Bazzite. Nie używa też opcji `plasmoid-updater --system`, która nie jest odpowiednia dla niezmiennego obrazu systemu.

## Instalacja AppImage

1. **[Pobierz gotowy AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases/latest/download/Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage)** z zakładki [Releases](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases) (nie trzeba pobierać archiwum z kodem źródłowym).
2. W katalogu z pobranym plikiem uruchom:

   ```bash
   chmod +x Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage
   ./Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage
   ```

3. W aplikacji kliknij **Zainstaluj / uaktualnij** — instalator sprawdzi zależności i w razie potrzeby użyje Homebrew.
4. Kliknij **Symuluj**, by zobaczyć potencjalne aktualizacje bez instalowania.
5. Włącz **Automatyczne aktualizacje w tle** i **Aktualizuj również plasmoid-updater** (oba przełączniki są domyślnie włączone).
6. Wybierz harmonogram i kliknij **Zapisz i zastosuj**. Od tego momentu możesz zamknąć GUI.

Wymagania środowiska: **Bazzite KDE Plasma 6, x86_64**, dostęp do Internetu i polecenia `curl`, `systemctl`, `kpackagetool6` oraz `bsdtar` przy aktualizacji dodatków. Jeżeli brakuje `bsdtar`, program spróbuje zainstalować `libarchive` przez działający Homebrew i podlinkować binarkę do `~/.local/bin/bsdtar`. KDE `kpackagetool6` musi pochodzić z systemu.

## Zasada działania

| Element | Ścieżka / działanie |
|---|---|
| Binarka | `~/.local/bin/plasmoid-updater` |
| Agent automatyzacji | `~/.local/bin/bazzite-plasmoid-updater-agent` |
| Ustawienia upstream | `~/.config/plasmoid-updater.toml` |
| Ustawienia GUI | `~/.config/bazzite-plasmoid-updater-gui/settings.toml` |
| Usługa | `~/.config/systemd/user/bazzite-plasmoid-updater.service` |
| Timer | `~/.config/systemd/user/bazzite-plasmoid-updater.timer` |

Po zmianie ustawień program przeładowuje konfigurację `systemd --user` oraz odpowiednio włącza lub wyłącza timer. Każdy zaplanowany cykl uruchamia **samodzielnego agenta**. Najpierw sprawdza on wydanie `plasmoid-updater` na GitHub i ewentualnie weryfikuje SHA-256 oraz wymienia binarkę. Następnie wykonuje `plasmoid-updater --skip-plasma-detection update --yes --no-restart-plasma` dla dodatków użytkownika. Gdy GitHub jest chwilowo niedostępny, agent i tak próbuje zaktualizować dodatki z już zainstalowaną binarką.

`Persistent=true` pozwala nadrobić pominięte uruchomienie po kolejnym zalogowaniu, a `RandomizedDelaySec=20min` rozprasza czas aktualizacji. **Nie jest to proces działający bez przerwy**: systemd uruchamia agenta na czas aktualizacji. Jeśli użytkownik nie ma włączonego `linger`, menedżer `systemd --user` nie będzie pracował przez całą noc po jego wylogowaniu; zaległy cykl zostanie wykonany po ponownym logowaniu.

Automatyzacja pracuje w sesji użytkownika, **nie uruchamia się stale jako usługa systemowa root**. Przy domyślnych ustawieniach nie restartuje Plasma; zaktualizowane efekty i motywy mogą wymagać ponownego zalogowania, aby zobaczyć zmiany.

Sprawdzenie stanu:

```bash
systemctl --user status bazzite-plasmoid-updater.timer
systemctl --user list-timers --all
journalctl --user -u bazzite-plasmoid-updater.service -n 50 --no-pager
systemctl --user start bazzite-plasmoid-updater.service   # uruchom i przetestuj cały cykl teraz
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
cargo build --release --bins
cargo test
```

Aby utworzyć AppImage na systemie z wymaganymi bibliotekami i narzędziami do pakowania:

```bash
bash scripts/build-appimage.sh
```

Wynik: `dist/Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage`.

## Rozwój

Wersja `0.2.0` dodaje automatyczną aktualizację samego programu `plasmoid-updater` i osobnego agenta systemd. Zgłoszenia błędów i propozycje funkcji są mile widziane przez [Issues](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues).

Projekt nie jest oficjalnie powiązany z Bazzite, KDE ani autorem `plasmoid-updater`.

**Licencja:** [GPL-3.0-or-later](LICENSE).
