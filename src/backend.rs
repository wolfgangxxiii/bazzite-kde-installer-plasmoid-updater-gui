use crate::settings::Settings;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use toml_edit::{value, Array, DocumentMut};

pub const UPSTREAM: &str = "https://github.com/uwuclxdy/plasmoid-updater";
const RELEASE_API: &str = "https://api.github.com/repos/uwuclxdy/plasmoid-updater/releases/latest";
const SERVICE: &str = "plasmoid-updater.service";
const TIMER: &str = "plasmoid-updater.timer";
const PREVIOUS_TIMER: &str = "bazzite-plasmoid-updater.timer";
const AGENT: &str = "plasmoid-updater-agent";
const MANAGED: &str = "# Managed by Bazzite Plasmoid Updater GUI";

#[derive(Clone, Copy, Debug)]
pub enum Job {
    Install,
    Diagnose,
    Test,
    RunService,
    Check,
    DryRun,
    Update,
    Save { migrate_legacy: bool },
}

pub fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("Brak zmiennej HOME"))
}

fn updater_binary(home: &Path) -> PathBuf {
    home.join(".local/bin/plasmoid-updater")
}

fn upstream_config_path(home: &Path) -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("plasmoid-updater.toml")
}

fn executable(path: &Path) -> bool {
    path.is_file()
        && fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

fn on_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| executable(path))
}

fn version_of(path: &Path) -> Option<String> {
    let result = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&result.stdout);
    let mut columns = version.split_whitespace();
    if columns.next()? != "plasmoid-updater" {
        return None;
    }
    columns.next().map(str::to_owned)
}

fn find_existing_updater(home: &Path) -> Option<(PathBuf, String)> {
    let mut paths = vec![
        updater_binary(home),
        home.join(".cargo/bin/plasmoid-updater"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/plasmoid-updater"),
        PathBuf::from("/usr/local/bin/plasmoid-updater"),
        PathBuf::from("/usr/bin/plasmoid-updater"),
    ];
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path).map(|dir| dir.join("plasmoid-updater")));
    }
    let mut seen = HashSet::new();
    for path in paths {
        if !seen.insert(path.clone()) || !executable(&path) {
            continue;
        }
        if let Some(version) = version_of(&path) {
            return Some((path, version));
        }
    }
    None
}

pub fn initial_settings(home: &Path) -> Result<Settings> {
    import_upstream_exclusions(Settings::load(home)?, &upstream_config_path(home))
}

fn import_upstream_exclusions(mut settings: Settings, path: &Path) -> Result<Settings> {
    if path.is_file() {
        let doc: DocumentMut = fs::read_to_string(&path)?
            .parse()
            .with_context(|| format!("Nieprawidłowy TOML: {}", path.display()))?;
        if let Some(packages) = doc
            .get("excluded_packages")
            .and_then(|item| item.as_array())
        {
            settings.excluded_packages = packages
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_owned).ok_or_else(|| {
                        anyhow!("Nieprawidłowa lista excluded_packages w {}", path.display())
                    })
                })
                .collect::<Result<Vec<_>>>()?;
        }
    }
    Ok(settings)
}

fn agent_binary(home: &Path) -> PathBuf {
    home.join(".local/bin").join(AGENT)
}

fn user_path(home: &Path) -> String {
    format!(
        "{}:/home/linuxbrew/.linuxbrew/opt/libarchive/bin:/home/linuxbrew/.linuxbrew/bin:{}",
        home.join(".local/bin").display(),
        env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin".to_owned())
    )
}

fn service_bsdtar_exists(home: &Path) -> bool {
    executable(&home.join(".local/bin/bsdtar"))
        || executable(Path::new("/usr/bin/bsdtar"))
        || executable(Path::new(
            "/home/linuxbrew/.linuxbrew/opt/libarchive/bin/bsdtar",
        ))
        || executable(Path::new("/usr/local/bin/bsdtar"))
}

fn bsdtar_exists(home: &Path) -> bool {
    service_bsdtar_exists(home) || on_path("bsdtar").is_some()
}

fn install_missing_requirements(home: &Path, log: &mut dyn FnMut(&str)) -> Result<()> {
    anyhow::ensure!(
        executable(Path::new("/usr/bin/kpackagetool6")) || on_path("kpackagetool6").is_some(),
        "Brakuje kpackagetool6. To składnik KDE systemu Bazzite, którego nie należy zastępować pakietem Homebrew."
    );
    if service_bsdtar_exists(home) {
        return Ok(());
    }
    if let Some(source) = on_path("bsdtar") {
        let destination = home.join(".local/bin/bsdtar");
        fs::create_dir_all(
            destination
                .parent()
                .context("Nieprawidłowa ścieżka bsdtar")?,
        )?;
        if !destination.exists() {
            symlink(&source, &destination)?;
            log(&format!(
                "Udostępniono istniejący bsdtar z {} dla usługi systemd.",
                source.display()
            ));
        }
        anyhow::ensure!(
            service_bsdtar_exists(home),
            "bsdtar nie jest dostępny dla systemd"
        );
        return Ok(());
    }
    log("Nie znaleziono bsdtar. Instalowanie libarchive przez Homebrew…");
    let brew = if executable(Path::new("/home/linuxbrew/.linuxbrew/bin/brew")) {
        "/home/linuxbrew/.linuxbrew/bin/brew"
    } else {
        anyhow::ensure!(
            on_path("brew").is_some(),
            "Brakuje Homebrew: zainstaluj brew albo bsdtar przed kontynuowaniem"
        );
        "brew"
    };
    output(
        Command::new(brew)
            .args(["install", "libarchive"])
            .stdin(Stdio::null()),
    )
    .context("Instalacja Homebrew nie powiodła się; może wymagać potwierdzenia w terminalu")?;
    let prefix_output = output(Command::new(brew).args(["--prefix", "libarchive"]))?;
    let prefix = String::from_utf8_lossy(&prefix_output.stdout);
    let source = PathBuf::from(prefix.trim()).join("bin/bsdtar");
    anyhow::ensure!(
        source.is_file(),
        "Homebrew nie udostępnił bsdtar w {}",
        source.display()
    );
    let directory = home.join(".local/bin");
    fs::create_dir_all(&directory)?;
    let link = directory.join("bsdtar");
    if !link.exists() {
        symlink(&source, &link).context("Nie można utworzyć dowiązania do bsdtar")?;
    }
    anyhow::ensure!(
        service_bsdtar_exists(home),
        "Nie można znaleźć bsdtar po instalacji"
    );
    log("bsdtar został zainstalowany i jest dostępny dla agenta aktualizacji.");
    Ok(())
}

fn output(command: &mut Command) -> Result<Output> {
    let result = command.output().context("Nie można uruchomić polecenia")?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        bail!(
            "Polecenie zakończone kodem {}: {} {}",
            result.status,
            stderr.trim(),
            stdout.trim()
        );
    }
    Ok(result)
}

fn user_unit_state(unit: &str, action: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", action, "--quiet", unit])
        .status()
        .is_ok_and(|status| status.success())
}

fn user_units(home: &Path) -> PathBuf {
    home.join(".config/systemd/user")
}

fn is_managed_unit(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|content| content.starts_with(MANAGED))
}

fn unit_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn unmanaged_unit_collision(home: &Path) -> bool {
    let units = user_units(home);
    let local_collision = [SERVICE, TIMER]
        .iter()
        .any(|name| unit_exists(&units.join(name)) && !is_managed_unit(&units.join(name)));
    let external_timer = !is_managed_unit(&units.join(TIMER))
        && (user_unit_state(TIMER, "is-enabled") || user_unit_state(TIMER, "is-active"));
    local_collision || external_timer
}

fn legacy_timer_exists(home: &Path) -> bool {
    let old_managed_active = user_unit_state(PREVIOUS_TIMER, "is-enabled")
        || user_unit_state(PREVIOUS_TIMER, "is-active");
    old_managed_active || unmanaged_unit_collision(home)
}

fn unit_property(unit: &str, property: &str) -> Option<String> {
    let out = Command::new("systemctl")
        .args(["--user", "show", "--value", "-p", property, unit])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!value.is_empty() && value != "n/a" && value != "0").then_some(value)
}

pub fn status() -> String {
    let Ok(home) = home() else {
        return "Nie można ustalić katalogu domowego".to_owned();
    };
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let bazzite = os_release.to_lowercase().contains("bazzite");
    let kde = home.join(".local/share/knewstuff3").is_dir()
        || env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_uppercase()
            .contains("KDE");
    let binary_status = find_existing_updater(&home)
        .map(|(path, version)| format!("plasmoid-updater {version} ({})", path.display()))
        .unwrap_or_else(|| "plasmoid-updater: nie znaleziono".to_owned());
    let bsdtar = bsdtar_exists(&home);
    let managed = is_managed_unit(&user_units(&home).join(TIMER));
    let enabled = managed && user_unit_state(TIMER, "is-enabled");
    let active = managed && user_unit_state(TIMER, "is-active");
    let next = if enabled {
        unit_property(TIMER, "NextElapseUSecRealtime").unwrap_or_else(|| "brak danych".into())
    } else {
        "wyłączony".into()
    };
    let upstream_config = upstream_config_path(&home);
    let configuration = if upstream_config.is_file() {
        "istnieje"
    } else {
        "brak"
    };
    format!(
        "{} Bazzite · {} KDE · {} x86_64\n{} curl · {} Homebrew · {} bsdtar · {} kpackagetool6\n{}\nKonfiguracja: {} · Agent: {}\nTimer: {} · następny termin: {}{}",
        if bazzite { "✓" } else { "!" },
        if kde { "✓" } else { "!" },
        if env::consts::ARCH == "x86_64" { "✓" } else { "!" },
        if on_path("curl").is_some() { "✓" } else { "!" },
        if on_path("brew").is_some() || executable(Path::new("/home/linuxbrew/.linuxbrew/bin/brew")) { "✓" } else { "!" },
        if bsdtar { "✓" } else { "!" },
        if executable(Path::new("/usr/bin/kpackagetool6")) || on_path("kpackagetool6").is_some() {
            "✓"
        } else {
            "!"
        },
        binary_status,
        configuration,
        if executable(&agent_binary(&home)) { "gotowy" } else { "brak" },
        if active { "aktywny" } else if enabled { "włączony, ale nieaktywny" } else { "wyłączony" },
        next,
        if legacy_timer_exists(&home) { "\n! Wykryto starszy harmonogram — wymaga migracji" } else { "" }
    )
}

pub fn execute(job: Job, settings: Settings, log: &mut dyn FnMut(&str)) -> Result<()> {
    let home = home()?;
    match job {
        Job::Install => {
            install_missing_requirements(&home, log)?;
            install_updater(&home, false, log)
        }
        Job::Diagnose => diagnose(&home, log),
        Job::Test => test_configuration(&home, &settings, log),
        Job::RunService => {
            anyhow::ensure!(
                is_managed_unit(&user_units(&home).join(SERVICE)),
                "Usługa plasmoid-updater.service nie jest skonfigurowana przez instalator. Najpierw kliknij «Zapisz i zastosuj»."
            );
            anyhow::ensure!(
                unit_property(SERVICE, "LoadState").as_deref() == Some("loaded"),
                "Usługa plasmoid-updater.service nie została załadowana przez systemd. Użyj «Zapisz i zastosuj», aby przeładować jednostki."
            );
            log("Uruchamianie cyklu aktualizacji przez systemd --user…");
            output(Command::new("systemctl").args(["--user", "start", SERVICE]))?;
            log("Usługa zakończyła się sukcesem. Wynik jest dostępny w journalctl --user.");
            Ok(())
        }
        Job::Check => run_updater(&home, &["check"], true, log),
        Job::DryRun => run_updater(&home, &["update", "--dry-run"], false, log),
        Job::Update => run_update_cycle(&home, &settings, log),
        Job::Save { migrate_legacy } => save_configuration(&home, &settings, migrate_legacy, log),
    }
}

fn diagnose(home: &Path, log: &mut dyn FnMut(&str)) -> Result<()> {
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let bazzite = os_release.to_lowercase().contains("bazzite");
    let kde = env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| env::var("XDG_SESSION_DESKTOP"))
        .unwrap_or_default()
        .to_uppercase()
        .contains("KDE");
    let mut missing = Vec::new();
    for (name, detected) in [
        ("Bazzite", bazzite),
        ("KDE Plasma", kde),
        ("x86_64", env::consts::ARCH == "x86_64"),
        ("curl", on_path("curl").is_some()),
        (
            "kpackagetool6",
            executable(Path::new("/usr/bin/kpackagetool6")) || on_path("kpackagetool6").is_some(),
        ),
        (
            "systemctl --user",
            Command::new("systemctl")
                .args(["--user", "show-environment"])
                .output()
                .is_ok_and(|output| output.status.success()),
        ),
    ] {
        log(&format!("{} {name}", if detected { "✓" } else { "✗" }));
        if !detected {
            missing.push(name);
        }
    }
    log(&format!(
        "{} Homebrew — opcjonalny, potrzebny tylko do instalacji brakującego bsdtar",
        if on_path("brew").is_some() || executable(Path::new("/home/linuxbrew/.linuxbrew/bin/brew"))
        {
            "✓"
        } else {
            "!"
        }
    ));
    log(&format!(
        "{} bsdtar — {}",
        if bsdtar_exists(home) { "✓" } else { "!" },
        if bsdtar_exists(home) {
            "dostępny"
        } else {
            "instalator spróbuje użyć Homebrew"
        }
    ));
    if let Some((path, version)) = find_existing_updater(home) {
        log(&format!("✓ plasmoid-updater {version}: {}", path.display()));
    } else {
        log("! plasmoid-updater: brak; można go zainstalować przyciskiem Instaluj.");
    }
    let config = upstream_config_path(home);
    if config.is_file() {
        let _: DocumentMut = fs::read_to_string(&config)?
            .parse()
            .with_context(|| format!("Błędny plik {}", config.display()))?;
        log(&format!("✓ Istniejąca konfiguracja: {}", config.display()));
    } else {
        log("! Konfiguracja plasmoid-updater nie istnieje (można ją utworzyć z GUI).");
    }
    if legacy_timer_exists(home) {
        log("! Starszy timer: wymagane świadome przejęcie konfiguracji.");
    }
    if !missing.is_empty() {
        bail!(
            "Brakuje wymaganych elementów środowiska: {}",
            missing.join(", ")
        );
    }
    log("Kontrola podstawowych elementów środowiska zakończona.");
    Ok(())
}

fn test_configuration(home: &Path, settings: &Settings, log: &mut dyn FnMut(&str)) -> Result<()> {
    settings.validate()?;
    let (path, version) = find_existing_updater(home)
        .context("Nie znaleziono sprawnego plasmoid-updater — użyj przycisku Instaluj")?;
    log(&format!(
        "✓ Wykryto plasmoid-updater {version} w {}",
        path.display()
    ));
    anyhow::ensure!(
        service_bsdtar_exists(home),
        "Brakuje bsdtar dostępnego dla systemd — uruchom Instaluj / uaktualnij"
    );
    anyhow::ensure!(
        executable(Path::new("/usr/bin/kpackagetool6")) || on_path("kpackagetool6").is_some(),
        "Brakuje kpackagetool6"
    );
    let config = upstream_config_path(home);
    if config.is_file() {
        let _: DocumentMut = fs::read_to_string(&config)?
            .parse()
            .with_context(|| format!("Błędna konfiguracja: {}", config.display()))?;
        log("✓ Plik konfiguracyjny TOML ma poprawną składnię.");
    } else {
        bail!("Brak konfiguracji plasmoid-updater: {}", config.display());
    }
    log("Test bez zmian w dodatkach: plasmoid-updater update --dry-run…");
    run_updater(home, &["update", "--dry-run"], false, log)?;
    log("✓ Tryb dry-run zadziałał poprawnie.");

    if settings.automatic {
        anyhow::ensure!(
            executable(&agent_binary(home)),
            "Brakuje agenta w ~/.local/bin"
        );
        let agent_test = Command::new(agent_binary(home))
            .arg("--version")
            .output()
            .context("Nie można uruchomić agenta")?;
        anyhow::ensure!(
            agent_test.status.success(),
            "Agent nie uruchamia się poprawnie"
        );
        let service_path = home.join(".config/systemd/user").join(SERVICE);
        let timer_path = home.join(".config/systemd/user").join(TIMER);
        for file in [&service_path, &timer_path] {
            anyhow::ensure!(
                fs::read_to_string(file).is_ok_and(|value| value.contains(MANAGED)),
                "Brakuje zarządzanej jednostki systemd: {}",
                file.display()
            );
        }
        let service_text = fs::read_to_string(&service_path)?;
        anyhow::ensure!(
            service_text.contains(&format!("ExecStart=%h/.local/bin/{AGENT} run")),
            "Usługa nie uruchamia właściwego agenta"
        );
        let timer_text = fs::read_to_string(&timer_path)?;
        anyhow::ensure!(
            timer_text.contains(&format!(
                "OnCalendar={}",
                timer_schedule(settings.frequency_hours)?
            )),
            "Harmonogram systemd nie odpowiada ustawieniom GUI — zapisz konfigurację ponownie"
        );
        anyhow::ensure!(
            user_unit_state(TIMER, "is-enabled"),
            "Timer nie jest włączony"
        );
        anyhow::ensure!(
            user_unit_state(TIMER, "is-active"),
            "Timer nie jest aktywny"
        );
        anyhow::ensure!(
            unit_property(SERVICE, "LoadState").as_deref() == Some("loaded"),
            "Usługa systemd nie została poprawnie załadowana"
        );
        anyhow::ensure!(
            unit_property(TIMER, "LoadState").as_deref() == Some("loaded"),
            "Timer systemd nie został poprawnie załadowany"
        );
        anyhow::ensure!(
            !legacy_timer_exists(home),
            "Starszy timer nadal jest aktywny lub włączony"
        );
        log("✓ Agent, usługa i harmonogram systemd są gotowe.");
        if let Some(next) = unit_property(TIMER, "NextElapseUSecRealtime") {
            log(&format!("Następne uruchomienie: {next}"));
        }
    } else {
        log("Harmonogram jest wyłączony zgodnie z konfiguracją GUI.");
    }
    log("✓ Test konfiguracji zakończony pomyślnie. Nie zainstalowano żadnych dodatków.");
    Ok(())
}

fn download(url: &str, destination: &Path) -> Result<()> {
    output(
        Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error"])
            .args([
                "--retry",
                "2",
                "--connect-timeout",
                "15",
                "--max-time",
                "180",
            ])
            .args(["--proto", "=https", "--proto-redir", "=https"])
            .arg("--output")
            .arg(destination)
            .arg(url),
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn verified_asset(release: &GitHubRelease) -> Result<(&str, &str)> {
    let asset = release
        .assets
        .iter()
        .find(|asset| {
            matches!(
                asset.name.as_str(),
                "plasmoid-updater-linux-x64" | "plasmoid-updater-linux-x86_64"
            )
        })
        .context("Najnowsze wydanie GitHub nie zawiera binarki Linux x86_64")?;
    anyhow::ensure!(
        asset
            .browser_download_url
            .starts_with(&format!("{UPSTREAM}/releases/download/")),
        "Nieprawidłowy adres pobierania binarki"
    );
    let digest = asset
        .digest
        .as_deref()
        .context("Brak sumy kontrolnej w metadanych GitHub")?;
    let checksum = digest
        .strip_prefix("sha256:")
        .context("Nieobsługiwany typ sumy kontrolnej")?;
    anyhow::ensure!(
        checksum.len() == 64 && checksum.bytes().all(|b| b.is_ascii_hexdigit()),
        "Nieprawidłowa suma SHA-256 w metadanych GitHub"
    );
    Ok((&asset.browser_download_url, checksum))
}

fn verify_checksum(binary: &Path, expected: &str) -> Result<()> {
    let mut file = File::open(binary)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 16 * 1024];
    loop {
        let size = file.read(&mut buf)?;
        if size == 0 {
            break;
        }
        hasher.update(&buf[..size]);
    }
    let actual = format!("{:x}", hasher.finalize());
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(expected),
        "Błędna suma SHA-256 — instalacja przerwana"
    );
    Ok(())
}

fn installed_version(home: &Path) -> Option<String> {
    find_existing_updater(home).map(|(_, version)| version)
}

fn already_current(version: Option<&str>, release_tag: &str) -> bool {
    version.is_some_and(|version| {
        let release = release_tag.trim_start_matches('v');
        version == release
            || (parse_version(version)
                .zip(parse_version(release))
                .is_some_and(|(installed, latest)| installed >= latest))
    })
}

fn parse_version(value: &str) -> Option<Vec<u32>> {
    value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()
        .filter(|parts| parts.len() == 3)
}

fn install_updater(home: &Path, force: bool, log: &mut dyn FnMut(&str)) -> Result<()> {
    anyhow::ensure!(
        env::consts::ARCH == "x86_64",
        "Wydanie obsługuje obecnie tylko x86_64"
    );
    let temporary = tempfile::tempdir().context("Nie można utworzyć katalogu tymczasowego")?;
    let metadata = temporary.path().join("release.json");
    let binary = temporary.path().join("plasmoid-updater-linux-x86_64");
    log("Odczytywanie metadanych najnowszego wydania GitHub…");
    download(RELEASE_API, &metadata)?;
    let release: GitHubRelease = serde_json::from_slice(&fs::read(metadata)?)
        .context("GitHub zwrócił nieprawidłowe metadane wydania")?;
    if !force && already_current(installed_version(home).as_deref(), &release.tag_name) {
        if let Some((path, version)) = find_existing_updater(home) {
            log(&format!("plasmoid-updater {version} ({}) jest już aktualny — zachowano istniejącą instalację.", path.display()));
        }
        return Ok(());
    }
    let (url, digest) = verified_asset(&release)?;
    log(&format!(
        "Wydanie {}: pobieranie {}…",
        release.tag_name, url
    ));
    download(url, &binary)?;
    verify_checksum(&binary, digest)?;
    log("SHA-256 poprawne. Instalowanie w ~/.local/bin…");

    let target = updater_binary(home);
    let folder = target.parent().context("Niepoprawna ścieżka instalacji")?;
    fs::create_dir_all(folder)?;
    let mut staged = tempfile::NamedTempFile::new_in(folder)?;
    std::io::copy(&mut File::open(&binary)?, &mut staged)?;
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    anyhow::ensure!(
        version_of(staged.path()).is_some(),
        "Pobrana binarka nie odpowiada na --version — nie zastąpiono poprzedniej"
    );
    staged
        .persist(&target)
        .context("Nie można zastąpić programu")?;
    log("Program został zainstalowany pomyślnie.");
    log("Uwaga: tylko dodatki użytkownika; obraz Bazzite pozostaje nietknięty.");
    Ok(())
}

fn run_update_cycle(home: &Path, settings: &Settings, log: &mut dyn FnMut(&str)) -> Result<()> {
    // The GitHub release check is best-effort: a temporary outage must not
    // prevent existing plasmoids from receiving KDE Store updates.
    let tool_update_failure = if settings.auto_update_tool {
        log("Sprawdzanie aktualizacji samego plasmoid-updater na GitHub…");
        install_updater(home, false, log).err()
    } else {
        None
    };

    let restart = if settings.restart_plasma {
        "--restart-plasma"
    } else {
        "--no-restart-plasma"
    };
    log("Sprawdzanie i instalowanie aktualizacji dodatków KDE Store…");
    let addons = run_updater(home, &["update", "--yes", restart], false, log);
    match (tool_update_failure, addons) {
        (None, Ok(())) => Ok(()),
        (Some(error), Ok(())) => {
            bail!("Dodatki zaktualizowano, ale nie udało się sprawdzić GitHub: {error:#}")
        }
        (None, Err(error)) => Err(error),
        (Some(tool_error), Err(addon_error)) => {
            bail!("Aktualizacja programu: {tool_error:#}; aktualizacja dodatków: {addon_error:#}")
        }
    }
}

fn run_updater(home: &Path, args: &[&str], check: bool, log: &mut dyn FnMut(&str)) -> Result<()> {
    let binary = find_existing_updater(home)
        .map(|(path, _)| path)
        .context("Najpierw zainstaluj plasmoid-updater")?;
    if args.first() == Some(&"update") && !bsdtar_exists(home) {
        bail!("Brakuje bsdtar. Zainstaluj: brew install libarchive");
    }
    let result = Command::new(binary)
        .arg("--skip-plasma-detection")
        .args(args)
        .env("PATH", user_path(home))
        .stdin(Stdio::null())
        .output()
        .context("Nie udało się uruchomić plasmoid-updater")?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    if !stdout.trim().is_empty() {
        log(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        log(stderr.trim());
    }
    if result.status.success() || (check && result.status.code() == Some(100)) {
        Ok(())
    } else {
        bail!(
            "plasmoid-updater zakończył działanie kodem {}",
            result.status
        )
    }
}

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().context("Niepoprawna ścieżka")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content)?;
    file.flush()?;
    file.persist(path)
        .with_context(|| format!("Nie udało się zapisać {}", path.display()))?;
    Ok(())
}

fn install_agent(home: &Path, log: &mut dyn FnMut(&str)) -> Result<()> {
    let executable = env::current_exe().context("Nie można ustalić lokalizacji GUI")?;
    let source = executable
        .parent()
        .context("Nieprawidłowa lokalizacja GUI")?
        .join(AGENT);
    anyhow::ensure!(
        source.is_file(),
        "Nie znaleziono agenta automatycznych aktualizacji: {}. Uruchom oficjalne AppImage lub skompiluj projekt przez cargo build --bins.",
        source.display()
    );
    let destination = agent_binary(home);
    let parent = destination.parent().context("Niepoprawna ścieżka agenta")?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut File::open(&source)?, &mut staged)?;
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    staged.persist(&destination)?;
    log("Zainstalowano agenta automatycznych aktualizacji w ~/.local/bin.");
    Ok(())
}

fn guarded_write(path: &Path, content: &str) -> Result<()> {
    if unit_exists(path) && !is_managed_unit(path) {
        bail!(
            "Plik {} już istnieje i nie został utworzony przez ten program. Nie nadpisano go.",
            path.display()
        );
    }
    atomic_write(path, content.as_bytes())
}

fn guarded_write_or_takeover(path: &Path, content: &str, takeover: bool) -> Result<()> {
    if takeover && !is_managed_unit(path) {
        atomic_write(path, content.as_bytes())
    } else {
        guarded_write(path, content)
    }
}

struct UnitSnapshot {
    path: PathBuf,
    previous: Option<Vec<u8>>,
}

fn snapshot_units(units: &Path) -> Result<Vec<UnitSnapshot>> {
    [SERVICE, TIMER]
        .iter()
        .map(|name| {
            let path = units.join(name);
            let previous = if fs::symlink_metadata(&path).is_ok() {
                Some(
                    fs::read(&path)
                        .with_context(|| format!("Nie można odczytać {}", path.display()))?,
                )
            } else {
                None
            };
            Ok(UnitSnapshot { path, previous })
        })
        .collect()
}

fn backup_unmanaged_units(
    home: &Path,
    snapshots: &[UnitSnapshot],
    log: &mut dyn FnMut(&str),
) -> Result<()> {
    let originals: Vec<_> = snapshots
        .iter()
        .filter(|item| item.previous.is_some() && !is_managed_unit(&item.path))
        .collect();
    if originals.is_empty() {
        log("Brak niezarządzanych plików do archiwizacji — pozostałe stare jednostki pozostają nietknięte.");
        return Ok(());
    }
    let parent = home.join(".config/bazzite-plasmoid-updater-gui/backups");
    fs::create_dir_all(&parent)?;
    let backup = tempfile::Builder::new()
        .prefix("units-")
        .tempdir_in(&parent)?;
    for snapshot in originals {
        let name = snapshot.path.file_name().context("Brak nazwy jednostki")?;
        fs::write(
            backup.path().join(name),
            snapshot.previous.as_ref().unwrap(),
        )?;
    }
    let saved = backup.keep();
    log(&format!(
        "Kopia starszych jednostek systemd: {}",
        saved.display()
    ));
    Ok(())
}

fn restore_units(snapshots: &[UnitSnapshot], log: &mut dyn FnMut(&str)) {
    for snapshot in snapshots {
        let result = match &snapshot.previous {
            Some(content) => atomic_write(&snapshot.path, content),
            None if is_managed_unit(&snapshot.path) => {
                fs::remove_file(&snapshot.path).map_err(Into::into)
            }
            None => Ok(()),
        };
        if let Err(error) = result {
            log(&format!(
                "Nie udało się przywrócić {}: {error:#}",
                snapshot.path.display()
            ));
        }
    }
}

fn timer_schedule(hours: u8) -> Result<&'static str> {
    match hours {
        6 => Ok("*-*-* 00,06,12,18:00:00"),
        12 => Ok("*-*-* 00,12:00:00"),
        24 => Ok("*-*-* 04:00:00"),
        _ => bail!("Nieobsługiwana częstotliwość: {hours}"),
    }
}

fn upstream_document(home: &Path, settings: &Settings) -> Result<DocumentMut> {
    upstream_document_at(&upstream_config_path(home), settings)
}

fn upstream_document_at(config_path: &Path, settings: &Settings) -> Result<DocumentMut> {
    let old = if config_path.is_file() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let mut doc: DocumentMut = old
        .parse()
        .with_context(|| format!("Błędny istniejący plik {}", config_path.display()))?;
    let mut excludes = Array::new();
    for package in &settings.excluded_packages {
        excludes.push(package.as_str());
    }
    // Preserve manual upstream settings such as assume_yes, excluded_patterns,
    // update_all_by_default and prompt_restart. The timer controls --yes itself.
    doc["excluded_packages"] = value(excludes);
    Ok(doc)
}

fn save_configuration(
    home: &Path,
    settings: &Settings,
    migrate_legacy: bool,
    log: &mut dyn FnMut(&str),
) -> Result<()> {
    settings.validate()?;
    if settings.automatic {
        anyhow::ensure!(
            find_existing_updater(home).is_some(),
            "Najpierw zainstaluj plasmoid-updater"
        );
        anyhow::ensure!(
            service_bsdtar_exists(home),
            "Brakuje bsdtar. Użyj najpierw przycisku Instaluj, aby program mógł dodać libarchive przez Homebrew."
        );
    }
    let takeover = unmanaged_unit_collision(home);
    anyhow::ensure!(
        !takeover || migrate_legacy,
        "Istnieją starsze jednostki plasmoid-updater.service/timer. Zaznacz w GUI przejęcie harmonogramu, aby zapisać kopię i zastąpić je nową konfiguracją."
    );
    let config_path = upstream_config_path(home);
    let doc = upstream_document(home, settings)?;

    let units = user_units(home);
    let service_path = units.join(SERVICE);
    let timer_path = units.join(TIMER);
    let snapshots = snapshot_units(&units)?;
    let service = format!(
        "{MANAGED}\n[Unit]\nDescription=Update plasmoid-updater and user KDE Store addons\n\n[Service]\nType=oneshot\nEnvironment=PATH=%h/.local/bin:/home/linuxbrew/.linuxbrew/opt/libarchive/bin:/home/linuxbrew/.linuxbrew/bin:/usr/local/bin:/usr/bin\nExecStart=%h/.local/bin/{AGENT} run\nNice=10\nIOSchedulingClass=idle\n"
    );
    let timer = format!(
        "{MANAGED}\n[Unit]\nDescription=Schedule KDE Store addon updates\n\n[Timer]\nOnCalendar={}\nRandomizedDelaySec=20min\nPersistent=true\nUnit={SERVICE}\n\n[Install]\nWantedBy=timers.target\n",
        timer_schedule(settings.frequency_hours)?
    );

    if settings.automatic {
        install_agent(home, log)?;
    }

    if takeover {
        backup_unmanaged_units(home, &snapshots, log)?;
    }
    let old_target_enabled = user_unit_state(TIMER, "is-enabled");
    let old_target_active = user_unit_state(TIMER, "is-active");
    let old_previous_enabled = user_unit_state(PREVIOUS_TIMER, "is-enabled");
    let old_previous_active = user_unit_state(PREVIOUS_TIMER, "is-active");

    let apply = (|| -> Result<()> {
        if takeover && (old_target_enabled || old_target_active) {
            log("Wyłączanie starszego plasmoid-updater.timer przed przejęciem jego nazwy…");
            output(Command::new("systemctl").args(["--user", "disable", "--now", TIMER]))?;
        }
        if old_previous_enabled || old_previous_active {
            log("Wyłączanie poprzedniego bazzite-plasmoid-updater.timer…");
            output(Command::new("systemctl").args(["--user", "disable", "--now", PREVIOUS_TIMER]))?;
        }
        log("Zapisywanie konfiguracji z zachowaniem dotychczasowych opcji TOML…");
        atomic_write(&config_path, doc.to_string().as_bytes())?;
        guarded_write_or_takeover(&service_path, &service, takeover)?;
        guarded_write_or_takeover(&timer_path, &timer, takeover)?;

        output(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
        // A persistent timer can start as soon as it is enabled: settings first.
        settings.save(home)?;
        if settings.automatic {
            output(Command::new("systemctl").args(["--user", "enable", "--now", TIMER]))?;
            log("Automatyczne aktualizacje plasmoid-updater zostały włączone.");
        } else {
            output(Command::new("systemctl").args(["--user", "disable", "--now", TIMER]))?;
            log("Automatyczne aktualizacje plasmoid-updater zostały wyłączone.");
        }
        Ok(())
    })();

    if let Err(error) = apply {
        log("Konfiguracja nie powiodła się. Przywracanie poprzednich jednostek systemd…");
        restore_units(&snapshots, log);
        let _ = output(Command::new("systemctl").args(["--user", "daemon-reload"]));
        if old_target_enabled || old_target_active {
            let _ = output(Command::new("systemctl").args(["--user", "enable", "--now", TIMER]));
        }
        if old_previous_enabled || old_previous_active {
            let _ = output(Command::new("systemctl").args([
                "--user",
                "enable",
                "--now",
                PREVIOUS_TIMER,
            ]));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_asset_uses_the_github_digest() {
        let release = GitHubRelease {
            tag_name: "v0.2.1".into(),
            assets: vec![GitHubAsset {
                name: "plasmoid-updater-linux-x64".into(),
                browser_download_url: format!(
                    "{UPSTREAM}/releases/download/v0.2.1/plasmoid-updater-linux-x64"
                ),
                digest: Some(format!("sha256:{}", "b".repeat(64))),
            }],
        };
        let (_, digest) = verified_asset(&release).unwrap();
        assert_eq!(digest, "b".repeat(64));
    }

    #[test]
    fn unknown_release_asset_is_rejected() {
        let release = GitHubRelease {
            tag_name: "v0.2.1".into(),
            assets: vec![GitHubAsset {
                name: "some-other-platform".into(),
                browser_download_url: "https://example.org/download".into(),
                digest: Some(format!("sha256:{}", "b".repeat(64))),
            }],
        };
        assert!(verified_asset(&release).is_err());
    }

    #[test]
    fn does_not_replace_unmanaged_systemd_units() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("unit.service");
        fs::write(&target, "[Service]\nExecStart=/usr/bin/true\n").unwrap();
        assert!(guarded_write(&target, "new").is_err());
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "[Service]\nExecStart=/usr/bin/true\n"
        );
    }

    #[test]
    fn schedules_are_stable() {
        assert!(timer_schedule(6).unwrap().contains("06"));
        assert!(timer_schedule(12).unwrap().contains("12"));
        assert!(timer_schedule(24).unwrap().contains("04"));
    }

    #[test]
    fn upstream_check_updates_only_when_newer() {
        assert!(already_current(Some("0.2.1"), "v0.2.1"));
        assert!(already_current(Some("0.3.0"), "v0.2.1"));
        assert!(!already_current(Some("0.2.0"), "v0.2.1"));
        assert!(!already_current(None, "v0.2.1"));
    }

    #[test]
    fn detects_a_cargo_install_without_creating_a_second_binary() {
        let temp = tempfile::tempdir().unwrap();
        let location = temp.path().join(".cargo/bin/plasmoid-updater");
        fs::create_dir_all(location.parent().unwrap()).unwrap();
        fs::write(&location, "#!/bin/sh\necho 'plasmoid-updater 0.2.1'\n").unwrap();
        fs::set_permissions(&location, fs::Permissions::from_mode(0o755)).unwrap();

        let (binary, version) = find_existing_updater(temp.path()).unwrap();
        assert_eq!(binary, location);
        assert_eq!(version, "0.2.1");
        assert!(!updater_binary(temp.path()).exists());
    }

    #[test]
    fn preserves_existing_user_configuration_and_imports_exclusions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".config/plasmoid-updater.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "# user comment\nexcluded_packages = ['weather']\nexcluded_patterns = ['^org\\\\.kde']\nassume_yes = true\nprompt_restart = false\n",
        )
        .unwrap();

        let settings = import_upstream_exclusions(Settings::default(), &path).unwrap();
        assert_eq!(settings.excluded_packages, vec!["weather"]);
        let doc = upstream_document_at(&path, &settings).unwrap();
        let value = doc.to_string();
        assert!(value.contains("# user comment"));
        assert!(value.contains("assume_yes = true"));
        assert!(value.contains("prompt_restart = false"));
        assert!(value.contains("excluded_patterns"));
    }

    #[test]
    fn snapshots_and_restores_preexisting_unmanaged_units() {
        let temp = tempfile::tempdir().unwrap();
        let units = user_units(temp.path());
        fs::create_dir_all(&units).unwrap();
        let previous = "[Service]\nExecStart=/usr/bin/true\n";
        fs::write(units.join(SERVICE), previous).unwrap();
        assert!(unmanaged_unit_collision(temp.path()));
        let saved = snapshot_units(&units).unwrap();
        backup_unmanaged_units(temp.path(), &saved, &mut |_| {}).unwrap();
        let backups = temp
            .path()
            .join(".config/bazzite-plasmoid-updater-gui/backups");
        let backup = fs::read_dir(backups)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read_to_string(backup.join(SERVICE)).unwrap(), previous);

        atomic_write(
            &units.join(SERVICE),
            format!("{MANAGED}\n[Service]\n").as_bytes(),
        )
        .unwrap();
        atomic_write(
            &units.join(TIMER),
            format!("{MANAGED}\n[Timer]\n").as_bytes(),
        )
        .unwrap();
        restore_units(&saved, &mut |_| {});
        assert_eq!(fs::read_to_string(units.join(SERVICE)).unwrap(), previous);
        assert!(!units.join(TIMER).exists());
    }
}
