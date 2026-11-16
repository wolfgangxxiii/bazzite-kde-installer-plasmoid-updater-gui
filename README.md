<div align="center">

<img src="assets/io.github.wolfgangxxiii.BazzitePlasmoidUpdater.svg" width="128" alt="Bazzite Plasmoid Updater GUI icon" />

# Bazzite Plasmoid Updater GUI

**Natywny instalator i konfigurator plasmoid-updater dla Bazzite KDE**

Rust · GTK4 · libadwaita · AppImage · systemd --user

[![Build GTK4 AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml/badge.svg)](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

</div>

---

## Co potrafi?

- Pobiera oficjalne wydanie **[uwuclxdy/plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater)** dla x86_64.
- Wykrywa istniejące instalacje w `~/.local/bin`, `~/.cargo/bin`, lokalizacjach systemowych i `PATH`; nie pobiera ponownie aktualnej wersji.
- Sprawdza Bazzite, KDE, architekturę, `curl`, Homebrew, `bsdtar`, `kpackagetool6`, `systemctl --user`, istniejącą konfigurację i starsze harmonogramy.
- Weryfikuje pobrany plik za pomocą **SHA-256** przed instalacją.
- Instaluje narzędzie w `~/.local/bin` — bez `sudo` i bez zmiany obrazu Bazzite.
- Odczytuje wykluczenia z istniejącego `plasmoid-updater.toml`, nie nadpisując pozostałych indywidualnych opcji pliku.
- Wykrywa starsze jednostki systemd. Przejęcie niezarządzanych `plasmoid-updater.service/timer` wymaga zgody użytkownika; ich kopie są zachowywane przed zmianą.
- Sprawdza aktualizacje, pokazuje symulację (`--dry-run`) i wykonuje aktualizacje dodatków użytkownika.
- **Samoczynnie aktualizuje również `plasmoid-updater`:** w każdym cyklu sprawdza nowe wydanie na GitHub, porównuje wersję i pobiera binarkę tylko wtedy, gdy jest nowsza.
- Konfiguruje automatyczne aktualizacje dodatków i narzędzia co **6, 12 lub 24 godziny** przez timer `systemd --user` — **bez konieczności uruchamiania GUI w tle**.
- Instaluje lekki, niezależny agent aktualizacji uruchamiany przez systemd; działa nawet po zamknięciu AppImage.
- Umożliwia oddzielne wyłączenie automatycznej aktualizacji samego `plasmoid-updater`.
- Gdy brakuje `bsdtar`, próbuje zainstalować `libarchive` przez Homebrew (`brew install libarchive`).
- Pozwala wykluczyć konkretne dodatki z aktualizacji.
- Pozwala zdecydować, czy po automatycznej aktualizacji restartować Plasma (domyślnie **nie**).
- Zapisuje dziennik działań i wyświetla błędy w interfejsie.
- Pokazuje aktywność timera i następny zaplanowany termin; testuje całą konfigurację przez `--dry-run`.
- Pozwala uruchomić pełny cykl aktualizacji przez usługę systemd z poziomu GUI.

Program **nie** aktualizuje samego Bazzite, Flatpaków, sterowników ani pakietów systemowych. Do tych celów służą oficjalne mechanizmy Bazzite. Nie używa też opcji `plasmoid-updater --system`, która nie jest odpowiednia dla niezmiennego obrazu systemu.

## Instalacja AppImage

1. **[Pobierz gotowy AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases/latest/download/Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage)** z zakładki [Releases](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases) (nie trzeba pobierać archiwum z kodem źródłowym).
2. W katalogu z pobranym plikiem uruchom:

   ```bash
   chmod +x Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage
   ./Bazzite-Plasmoid-Updater-GUI-x86_64.AppImage
   ```

3. Kliknij **Sprawdź składniki**, aby wykryć wymagania oraz istniejące narzędzie i konfigurację.
4. Kliknij **Zainstaluj / uaktualnij** — jeśli istniejąca wersja jest aktualna, pozostanie bez zmian. Instalator uzupełni brakujące `bsdtar` przez Homebrew.
5. Kliknij **Symuluj**, by zobaczyć potencjalne aktualizacje bez instalowania.
6. Włącz **Automatyczne aktualizacje w tle** i **Aktualizuj również plasmoid-updater** (oba przełączniki są domyślnie włączone).
7. Jeśli istnieją wcześniej utworzone `plasmoid-updater.service/timer`, zaznacz **Przejmij istniejącą usługę i harmonogram plasmoid-updater**. Instalator utworzy kopie jednostek, wyłączy stary timer i dopiero wtedy skonfiguruje nowy. Wcześniejsze jednostki `bazzite-plasmoid-updater.*` utworzone przez ten instalator są wyłączane automatycznie.
8. Wybierz harmonogram i kliknij **Zapisz i zastosuj**.
9. Kliknij **Przetestuj**, aby zweryfikować konfigurację i symulację. **Uruchom teraz** wykonuje już prawdziwy cykl instalowania dostępnych aktualizacji.

Wymagania środowiska: **Bazzite KDE Plasma 6, x86_64**, dostęp do Internetu i polecenia `curl`, `systemctl`, `kpackagetool6` oraz `bsdtar` przy aktualizacji dodatków. Jeżeli brakuje `bsdtar`, program spróbuje zainstalować `libarchive` przez działający Homebrew i podlinkować binarkę do `~/.local/bin/bsdtar`. KDE `kpackagetool6` musi pochodzić z systemu.

## Zasada działania

| Element | Ścieżka / działanie |
|---|---|
| Binarka | Istniejąca instalacja z `PATH` / `~/.cargo/bin` albo `~/.local/bin/plasmoid-updater` |
| Agent automatyzacji | `~/.local/bin/plasmoid-updater-agent` |
| Ustawienia upstream | `~/.config/plasmoid-updater.toml` |
| Ustawienia GUI | `~/.config/bazzite-plasmoid-updater-gui/settings.toml` |
| Usługa | `~/.config/systemd/user/plasmoid-updater.service` |
| Timer | `~/.config/systemd/user/plasmoid-updater.timer` |

Po zmianie ustawień program przeładowuje konfigurację `systemd --user` oraz odpowiednio włącza lub wyłącza timer. Każdy zaplanowany cykl uruchamia **samodzielnego agenta**. Najpierw sprawdza on wydanie `plasmoid-updater` na GitHub i ewentualnie weryfikuje SHA-256 oraz wymienia binarkę. Następnie wykonuje `plasmoid-updater --skip-plasma-detection update --yes --no-restart-plasma` dla dodatków użytkownika. Gdy GitHub jest chwilowo niedostępny, agent i tak próbuje zaktualizować dodatki z już zainstalowaną binarką.

Przy pierwszym uruchomieniu istniejąca konfiguracja wykluczeń jest pokazywana w GUI. Program zachowuje ręcznie ustawione `assume_yes`, `update_all_by_default`, `prompt_restart` i `excluded_patterns`. Do automatycznego uruchomienia używa opcji `--yes` niezależnie od interaktywnych ustawień użytkownika. Jeśli `plasmoid-updater` jest zainstalowany przez Cargo i jest aktualny, nie tworzy drugiej kopii; kiedy będzie wymagał aktualizacji, bezpiecznie instaluje nowszą binarkę do `~/.local/bin`, pozostawiając plik zarządzany przez Cargo bez zmian.

`Persistent=true` pozwala nadrobić pominięte uruchomienie po kolejnym zalogowaniu, a `RandomizedDelaySec=20min` rozprasza czas aktualizacji. **Nie jest to proces działający bez przerwy**: systemd uruchamia agenta na czas aktualizacji. Jeśli użytkownik nie ma włączonego `linger`, menedżer `systemd --user` nie będzie pracował przez całą noc po jego wylogowaniu; zaległy cykl zostanie wykonany po ponownym logowaniu.

Automatyzacja pracuje w sesji użytkownika, **nie uruchamia się stale jako usługa systemowa root**. Przy domyślnych ustawieniach nie restartuje Plasma; zaktualizowane efekty i motywy mogą wymagać ponownego zalogowania, aby zobaczyć zmiany.

Sprawdzenie stanu:

```bash
systemctl --user status plasmoid-updater.timer
systemctl --user list-timers --all
journalctl --user -u plasmoid-updater.service -n 50 --no-pager
systemctl --user start plasmoid-updater.service   # uruchom pełny cykl teraz (instaluje aktualizacje)
```

Zatrzymanie automatyzacji:

```bash
systemctl --user disable --now plasmoid-updater.timer
```

### Test na rzeczywistym Bazzite

GUI zawiera dwa osobne przyciski: **Przetestuj** (bez instalowania dodatków) i **Uruchom teraz** (pełny cykl aktualizacji). Dodatkowo po sklonowaniu repozytorium można przeprowadzić diagnostykę na lokalnej instalacji Bazzite jako zwykły użytkownik:

```bash
bash scripts/verify-bazzite.sh
```

Test wykonuje tylko odczyt i `--dry-run`. GitHub Actions buduje aplikację i uruchamia testy pod Ubuntu 24.04, dlatego końcowe sprawdzenie na urządzeniu z Bazzite powinno zostać wykonane po instalacji AppImage.

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

Wersja `0.3.1` używa nazw `plasmoid-updater.service`, `plasmoid-updater.timer` oraz `plasmoid-updater-agent`. Istniejące niezarządzane jednostki są archiwizowane w `~/.config/bazzite-plasmoid-updater-gui/backups/` przed przejęciem. Zgłoszenia błędów i propozycje funkcji są mile widziane przez [Issues](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues).

Projekt nie jest oficjalnie powiązany z Bazzite, KDE ani autorem `plasmoid-updater`.

**Licencja:** [GPL-3.0-or-later](LICENSE).
