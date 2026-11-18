<div align="center">

<img src="assets/io.github.wolfgangxxiii.BazzitePlasmoidUpdater.svg" width="112" alt="Bazzite Plasmoid Updater GUI logo" />

# Bazzite Plasmoid Updater GUI

**A simpler way to keep your KDE Plasma add-ons up to date.** · **Wygodny sposób na aktualizowanie dodatków KDE Plasma.**

A native **Rust + GTK4/libadwaita** companion app for [plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater), made for **Bazzite KDE**.

[![Build](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml/badge.svg)](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/actions/workflows/build.yml)
[![Latest release](https://img.shields.io/github/v/release/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui?label=release)](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases/latest)
[![License](https://img.shields.io/github/license/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Bazzite%20KDE-blue)
![Architecture](https://img.shields.io/badge/architecture-x86__64-5965a8)

**[⬇️ Download AppImage / Pobierz AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases/latest/download/Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage)** · **[📦 Releases](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases)** · **[🐛 Report an issue](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues)**

**[🇵🇱 Polski](#pl)** · **[🇬🇧 English](#en)**

</div>

---

<a id="pl"></a>

## 🇵🇱 Polski

### Wszystkie dodatki Plasma pod kontrolą

**Bazzite Plasmoid Updater GUI** to graficzny instalator, menedżer i konfigurator automatycznych aktualizacji dla narzędzia [plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater). Pozwala sprawdzać i aktualizować dodatki KDE Store bez pamiętania poleceń terminala.

Program działa w przestrzeni użytkownika, nie wymaga ciągłego działania w tle i **nie modyfikuje obrazu systemu Bazzite**.

### ✨ Najważniejsze możliwości

| | Funkcja | Co daje? |
|:--:|---|---|
| 🧩 | **Dodatki Plasma** | Lista zainstalowanych komponentów, wyszukiwanie, filtry i sprawdzanie dostępnych aktualizacji. |
| ⬇️ | **Instalacja jednym kliknięciem** | Pobranie lub aktualizacja oficjalnej binarki `plasmoid-updater` z weryfikacją SHA-256. |
| 🔄 | **Automatyczne aktualizacje** | Harmonogram co **6, 12 lub 24 godziny**, obsługiwany przez `systemd --user`. |
| 🌐 | **Odporność na awarie sieci** | Trzy próby połączenia przy przejściowych błędach KDE Store, z odstępami 3 i 9 sekund. |
| 🚀 | **Aktualizacja samego narzędzia** | Sprawdzanie nowych wydań `plasmoid-updater` i instalowanie ich w razie potrzeby. |
| 🔔 | **Powiadomienia** | Opcjonalne komunikaty KDE o powodzeniu lub niepowodzeniu aktualizacji. |
| 🛡️ | **Kontrola i bezpieczeństwo** | Symulacja aktualizacji, wykluczenia dodatków, kopie zapasowe przejmowanych usług i historia zdarzeń. |
| 🎨 | **Przyjazny interfejs** | Adaptacyjny układ, kolory akcentu z motywu KDE, obsługa języków polskiego i angielskiego. |

### 🖥️ Sześć przejrzystych ekranów

| Ekran | Zastosowanie |
|---|---|
| **Pulpit** | Stan środowiska, `plasmoid-updater`, agenta i harmonogramu; diagnostyka i szybkie działania. |
| **Instalacja** | Instalowanie/aktualizacja narzędzia, sprawdzanie dostępnych aktualizacji i symulacja. |
| **Dodatki Plasma** | Przegląd dodatków użytkownika i systemowych oraz dostępnych wersji. |
| **Automatyzacja** | Harmonogram, powiadomienia, wykluczenia, test konfiguracji i ręczne uruchomienie. |
| **Dziennik** | Ostatnie wykonania, wyniki i logi usługi systemd. |
| **O programie** | Wersja aplikacji, repozytorium, pomoc, licencja i autorzy. |

Interfejs dopasowuje się do wąskich okien, zachowuje ustawienia języka i uwzględnia kolory aktywnego motywu Plasma. Obsługuje zarówno nowsze wyniki JSON narzędzia `plasmoid-updater`, jak i tekstowy format starszych wydań.

### 📥 Instalacja

**Wymagania:** Bazzite z **KDE Plasma 6**, architektura **x86_64**, połączenie z Internetem, `curl`, `systemctl --user` i `kpackagetool6`. Do instalowania aktualizacji dodatków potrzebny jest `bsdtar` — jeżeli go brakuje, instalator może doinstalować `libarchive` przez Homebrew.

1. **[Pobierz najnowszy AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases/latest/download/Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage)**.
2. Otwórz terminal w katalogu z pobranym plikiem i wykonaj:

   ```bash
   chmod +x Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage
   ./Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage
   ```

3. Na ekranie **Pulpit** kliknij **Sprawdź składniki**.
4. Przejdź do **Instalacja → Zainstaluj / uaktualnij**.
5. Na ekranie **Dodatki Plasma** przejrzyj dodatki, a w **Automatyzacja** ustaw harmonogram i kliknij **Zapisz i zastosuj**.
6. Użyj przycisku **Przetestuj** (bez zmian w dodatkach), zanim wybierzesz **Uruchom teraz** (rzeczywista aktualizacja).

> [!TIP]
> AppImage jest przenośny, a agent aktualizacji działa niezależnie od otwartego okna aplikacji. Nie musisz pozostawiać GUI uruchomionego.

### ⚙️ Jak działa automatyzacja?

1. Timer `systemd --user` uruchamia lekki agent zgodnie z wybranym harmonogramem.
2. Agent opcjonalnie sprawdza nowe wydanie `plasmoid-updater`, pobiera je i weryfikuje sumę SHA-256.
3. Następnie aktualizuje **dodatki KDE Store użytkownika**, z uwzględnieniem wykluczeń.
4. Wynik zapisuje w dzienniku i — zależnie od ustawień — wysyła powiadomienie KDE.

Jeśli KDE Store chwilowo nie odpowiada, agent ponawia żądanie maksymalnie dwa razy. Przy dłuższej awarii kończy cykl z błędem, nie ukrywa go i spróbuje ponownie przy następnym terminie harmonogramu. Po nieudanej próbie GUI zachowuje poprzednią listę aktualizacji z ostrzeżeniem, że dane mogą być nieaktualne.

- Interwały: **6 / 12 / 24 h**; pominięty cykl może zostać nadrobiony po następnym zalogowaniu dzięki `Persistent=true`.
- Domyślnie aplikacja **nie restartuje powłoki Plasma** po aktualizacji.
- Bez włączonego `linger` usługa użytkownika nie musi działać podczas wylogowania; zaplanowany cykl zostanie wykonany po powrocie do sesji.
- Istniejące, niezarządzane przez aplikację usługi/timery są przejmowane **tylko za zgodą użytkownika**, po utworzeniu kopii zapasowych.

> [!IMPORTANT]
> Program **nie aktualizuje Bazzite, Flatpaków, sterowników ani pakietów systemowych**. Dodatki systemowe można przeglądać w trybie tylko do odczytu; automatyczne aktualizacje dotyczą wyłącznie dodatków użytkownika.

<details>
<summary><b>📁 Pliki i usługi</b></summary>

| Element | Lokalizacja |
|---|---|
| `plasmoid-updater` | Wykrywana istniejąca instalacja lub `~/.local/bin/plasmoid-updater` |
| Agent | `~/.local/bin/plasmoid-updater-agent` |
| Konfiguracja upstream | `~/.config/plasmoid-updater.toml` |
| Ustawienia GUI | `~/.config/bazzite-plasmoid-updater-gui/settings.toml` |
| Usługa | `~/.config/systemd/user/plasmoid-updater.service` |
| Timer | `~/.config/systemd/user/plasmoid-updater.timer` |
| Kopie zapasowe przejętych jednostek | `~/.config/bazzite-plasmoid-updater-gui/backups/` |

</details>

<details>
<summary><b>🔍 Diagnostyka i sterowanie harmonogramem</b></summary>

```bash
# Stan i termin kolejnego uruchomienia
systemctl --user status plasmoid-updater.timer
systemctl --user list-timers --all

# Ostatnie wpisy dziennika
journalctl --user -u plasmoid-updater.service -n 50 --no-pager

# Wyłączenie automatycznych aktualizacji
systemctl --user disable --now plasmoid-updater.timer

# Weryfikacja środowiska Bazzite (po sklonowaniu repozytorium)
bash scripts/verify-bazzite.sh
```

**Uwaga:** `systemctl --user start plasmoid-updater.service` uruchamia rzeczywistą aktualizację. Do testowania bez zmian użyj przycisku **Przetestuj**.

</details>

### 🧑‍💻 Budowanie ze źródeł

Wymagane są Rust (stable), GTK4, libadwaita oraz `pkg-config`. Na Bazzite środowisko kompilacji najlepiej przygotować w Distrobox/Toolbx.

```bash
git clone https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui.git
cd bazzite-kde-installer-plasmoid-updater-gui
cargo test
cargo build --release --bins

# Opcjonalnie: tworzenie AppImage
bash scripts/build-appimage.sh
```

Proces tworzenia AppImage jest też zautomatyzowany w GitHub Actions. Gotowe wydania znajdziesz w [Releases](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases).

### 🤝 Podziękowania i licencja

Sercem aplikacji jest **[plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater)** autorstwa **[uwuclxdy](https://github.com/uwuclxdy)**. Ten projekt jest niezależnym graficznym interfejsem i konfiguratorem, a nie zamiennikiem jego narzędzia. Dziękuję autorowi za pracę nad aktualizacjami dodatków KDE Plasma!

Kod jest dostępny na licencji **[GPL-3.0-or-later](LICENSE)**. Sugestie i zgłoszenia błędów: **[GitHub Issues](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues)**.

*Projekt nie jest oficjalnie powiązany z Bazzite, KDE ani autorem `plasmoid-updater`.*

**[↑ Wróć na górę](#bazzite-plasmoid-updater-gui) · [🇬🇧 Read in English](#en)**

---

<a id="en"></a>

## 🇬🇧 English

### Your KDE Plasma add-ons, under control

**Bazzite Plasmoid Updater GUI** is a graphical installer, manager, and automatic update scheduler for [plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater). Check and update KDE Store add-ons without having to remember terminal commands.

It runs in user space, doesn't need a permanently running background GUI, and **does not modify the Bazzite system image**.

### ✨ Highlights

| | Feature | What it does |
|:--:|---|---|
| 🧩 | **Plasma add-on browser** | Browse installed components, search, filter, and check available updates. |
| ⬇️ | **One-click setup** | Download or update the official `plasmoid-updater` binary with SHA-256 verification. |
| 🔄 | **Scheduled updates** | Run automatically every **6, 12, or 24 hours** with `systemd --user`. |
| 🌐 | **Network resilience** | Three attempts for transient KDE Store failures, with 3- and 9-second delays. |
| 🚀 | **Self-updating CLI tool** | Check for and install newer releases of `plasmoid-updater`. |
| 🔔 | **Desktop notifications** | Optional KDE notifications for successful and failed update cycles. |
| 🛡️ | **Control and safety** | Dry runs, add-on exclusions, backups of adopted systemd units, and activity logs. |
| 🎨 | **Native-feeling UI** | Responsive layout, KDE accent color integration, and English/Polish localization. |

### 🖥️ Six focused screens

| Screen | Purpose |
|---|---|
| **Dashboard** | Environment, CLI, agent, and timer status; diagnostics and quick actions. |
| **Installation** | Install/update the CLI, check available updates, and preview changes. |
| **Plasma Addons** | Browse user and system add-ons and available versions. |
| **Automation** | Schedule, notifications, exclusions, configuration tests, and manual runs. |
| **Log** | Recent runs, results, and systemd service logs. |
| **About** | Application version, repository, support, license, and credits. |

The interface adapts to smaller windows, remembers your preferred language, and follows the active Plasma accent color. It supports both newer JSON responses from `plasmoid-updater` and the text output of older releases.

### 📥 Installation

**Requirements:** Bazzite with **KDE Plasma 6**, **x86_64** architecture, Internet access, `curl`, `systemctl --user`, and `kpackagetool6`. Updating add-ons requires `bsdtar`; if missing, the installer can install `libarchive` through Homebrew.

1. **[Download the latest AppImage](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases/latest/download/Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage)**.
2. Open a terminal in the download directory and run:

   ```bash
   chmod +x Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage
   ./Bazzite-Plasmoid-Updater-GUI-v1.0.0-x86_64.AppImage
   ```

3. On the **Dashboard**, select **Check requirements**.
4. Go to **Installation → Install / upgrade**.
5. Browse components under **Plasma Addons**, then choose your schedule under **Automation** and click **Save and apply**.
6. Choose **Test** (no add-on changes) before **Run now** (performs actual updates).

> [!TIP]
> The AppImage is portable, and the updater agent works independently of the application window. You don't have to keep the GUI open.

### ⚙️ How automation works

1. A `systemd --user` timer launches the lightweight agent on your chosen schedule.
2. Optionally, the agent checks for a new `plasmoid-updater` release, downloads it, and verifies its SHA-256 checksum.
3. It then updates **user-installed KDE Store add-ons**, respecting your exclusions.
4. Results are recorded in the journal, with optional KDE notifications.

If KDE Store is temporarily unavailable, the agent makes up to two additional attempts. A longer outage still causes an explicit failure, and the timer will try again on its next scheduled run. After a failed update, the GUI retains the previous update list and warns that it may be outdated.

- Frequencies: **6 / 12 / 24 hours**. With `Persistent=true`, a missed run can be triggered after the next login.
- By default, the application **does not restart Plasma** after an update.
- Without `linger`, the user systemd manager may stop while logged out; missed updates can run after signing in again.
- Existing systemd services/timers not managed by the app are adopted **only with consent** and backed up first.

> [!IMPORTANT]
> This application **does not update Bazzite itself, Flatpaks, drivers, or system packages**. System-wide KDE add-ons are listed read-only; automated updates affect user-installed add-ons only.

<details>
<summary><b>📁 Files and services</b></summary>

| Item | Location |
|---|---|
| `plasmoid-updater` | Existing installation, if found, or `~/.local/bin/plasmoid-updater` |
| Agent | `~/.local/bin/plasmoid-updater-agent` |
| Upstream config | `~/.config/plasmoid-updater.toml` |
| GUI settings | `~/.config/bazzite-plasmoid-updater-gui/settings.toml` |
| Service | `~/.config/systemd/user/plasmoid-updater.service` |
| Timer | `~/.config/systemd/user/plasmoid-updater.timer` |
| Backups of adopted units | `~/.config/bazzite-plasmoid-updater-gui/backups/` |

</details>

<details>
<summary><b>🔍 Troubleshooting and timer controls</b></summary>

```bash
# Status and next scheduled run
systemctl --user status plasmoid-updater.timer
systemctl --user list-timers --all

# Recent journal entries
journalctl --user -u plasmoid-updater.service -n 50 --no-pager

# Disable automatic updates
systemctl --user disable --now plasmoid-updater.timer

# Check a real Bazzite environment (after cloning the repository)
bash scripts/verify-bazzite.sh
```

**Note:** `systemctl --user start plasmoid-updater.service` performs a real update. Use the **Test** button for a dry run.

</details>

### 🧑‍💻 Build from source

You'll need stable Rust, GTK4, libadwaita, and `pkg-config`. On Bazzite, using a Distrobox/Toolbx development container is recommended.

```bash
git clone https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui.git
cd bazzite-kde-installer-plasmoid-updater-gui
cargo test
cargo build --release --bins

# Optional: package the AppImage
bash scripts/build-appimage.sh
```

AppImage packaging is also automated with GitHub Actions. Prebuilt versions are available under [Releases](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases).

### 🤝 Credits and license

This application is powered by **[plasmoid-updater](https://github.com/uwuclxdy/plasmoid-updater)**, created by **[uwuclxdy](https://github.com/uwuclxdy)**. It is an independent GUI and configuration companion, not a replacement for the original tool. Many thanks to its author for making KDE Plasma add-on updates possible!

Licensed under **[GPL-3.0-or-later](LICENSE)**. Suggestions and bug reports are welcome in **[GitHub Issues](https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues)**.

*Not officially affiliated with Bazzite, KDE, or the author of `plasmoid-updater`.*

**[↑ Back to top](#bazzite-plasmoid-updater-gui) · [🇵🇱 Czytaj po polsku](#pl)**
