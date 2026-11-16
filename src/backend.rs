use crate::settings::Settings;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
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
const SERVICE: &str = "bazzite-plasmoid-updater.service";
const TIMER: &str = "bazzite-plasmoid-updater.timer";
const AGENT: &str = "bazzite-plasmoid-updater-agent";
const MANAGED: &str = "# Managed by Bazzite Plasmoid Updater GUI";

#[derive(Clone, Copy, Debug)]
pub enum Job {
    Install,
    Check,
    DryRun,
    Update,
    Save,
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

fn bsdtar_exists(home: &Path) -> bool {
    home.join(".local/bin/bsdtar").exists()
        || Path::new("/usr/bin/bsdtar").exists()
        || Path::new("/home/linuxbrew/.linuxbrew/opt/libarchive/bin/bsdtar").exists()
}

fn install_missing_requirements(home: &Path, log: &mut dyn FnMut(&str)) -> Result<()> {
    anyhow::ensure!(
        Path::new("/usr/bin/kpackagetool6").is_file(),
        "Brakuje kpackagetool6. To składnik KDE systemu Bazzite, którego nie należy zastępować pakietem Homebrew."
    );
    if bsdtar_exists(home) {
        return Ok(());
    }
    log("Nie znaleziono bsdtar. Instalowanie libarchive przez Homebrew…");
    let brew = if Path::new("/home/linuxbrew/.linuxbrew/bin/brew").exists() {
        "/home/linuxbrew/.linuxbrew/bin/brew"
    } else {
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
        bsdtar_exists(home),
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
    let updater = updater_binary(&home);
    let binary_status = if updater.is_file() {
        Command::new(&updater)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_else(|| "zainstalowany (nie można odczytać wersji)".into())
    } else {
        "niezainstalowany".to_owned()
    };
    let bsdtar = bsdtar_exists(&home);
    let enabled = Command::new("systemctl")
        .args(["--user", "is-enabled", TIMER])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    format!(
        "{} Bazzite  ·  {} KDE  ·  {} bsdtar  ·  {} kpackagetool6\n{}\nAutomatyzacja: {}",
        if bazzite { "✓" } else { "!" },
        if kde { "✓" } else { "!" },
        if bsdtar { "✓" } else { "!" },
        if Path::new("/usr/bin/kpackagetool6").exists() {
            "✓"
        } else {
            "!"
        },
        binary_status,
        if enabled { "włączona" } else { "wyłączona" }
    )
}

pub fn execute(job: Job, settings: Settings, log: &mut dyn FnMut(&str)) -> Result<()> {
    let home = home()?;
    match job {
        Job::Install => {
            install_missing_requirements(&home, log)?;
            install_updater(&home, true, log)
        }
        Job::Check => run_updater(&home, &["check"], true, log),
        Job::DryRun => run_updater(&home, &["update", "--dry-run"], false, log),
        Job::Update => run_update_cycle(&home, &settings, log),
        Job::Save => save_configuration(&home, &settings, log),
    }
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
    let result = Command::new(updater_binary(home))
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
        log(&format!(
            "plasmoid-updater {} jest aktualny — pominięto pobieranie.",
            release.tag_name
        ));
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
    let binary = updater_binary(home);
    anyhow::ensure!(binary.is_file(), "Najpierw zainstaluj plasmoid-updater");
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
    if path.exists() && !fs::read_to_string(path)?.contains(MANAGED) {
        bail!(
            "Plik {} już istnieje i nie został utworzony przez ten program. Nie nadpisano go.",
            path.display()
        );
    }
    atomic_write(path, content.as_bytes())
}

fn timer_schedule(hours: u8) -> Result<&'static str> {
    match hours {
        6 => Ok("*-*-* 00,06,12,18:00:00"),
        12 => Ok("*-*-* 00,12:00:00"),
        24 => Ok("*-*-* 04:00:00"),
        _ => bail!("Nieobsługiwana częstotliwość: {hours}"),
    }
}

fn save_configuration(home: &Path, settings: &Settings, log: &mut dyn FnMut(&str)) -> Result<()> {
    settings.validate()?;
    if settings.automatic {
        anyhow::ensure!(
            updater_binary(home).is_file(),
            "Najpierw zainstaluj plasmoid-updater"
        );
        anyhow::ensure!(
            bsdtar_exists(home),
            "Brakuje bsdtar. Użyj najpierw przycisku Instaluj, aby program mógł dodać libarchive przez Homebrew."
        );
        let legacy = Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", "plasmoid-updater.timer"])
            .status()
            .is_ok_and(|status| status.success());
        anyhow::ensure!(
            !legacy,
            "Wykryto aktywny starszy plasmoid-updater.timer. Wyłącz go przed uruchomieniem nowego harmonogramu, aby nie wykonywać aktualizacji dwa razy."
        );
    }
    let config_path = home.join(".config/plasmoid-updater.toml");
    let old = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let mut doc: DocumentMut = old
        .parse()
        .context("Błędny istniejący plik plasmoid-updater.toml")?;
    let mut excludes = Array::new();
    for package in &settings.excluded_packages {
        excludes.push(package.as_str());
    }
    doc["excluded_packages"] = value(excludes);
    // Manual invocations still ask for confirmation; only systemd has --yes.
    doc["assume_yes"] = value(false);
    doc["update_all_by_default"] = value(false);
    doc["prompt_restart"] = value(true);

    let units = home.join(".config/systemd/user");
    let service_path = units.join(SERVICE);
    let timer_path = units.join(TIMER);
    for path in [&service_path, &timer_path] {
        if path.exists() && !fs::read_to_string(path)?.contains(MANAGED) {
            bail!("Nie nadpisano niezarządzanego pliku {}", path.display());
        }
    }
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
    log("Zapisywanie konfiguracji z zachowaniem istniejących komentarzy…");
    atomic_write(&config_path, doc.to_string().as_bytes())?;
    guarded_write(&service_path, &service)?;
    guarded_write(&timer_path, &timer)?;

    log("Odświeżanie usług użytkownika systemd…");
    output(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    // Write the settings before enabling a persistent timer: an overdue timer
    // may start immediately during 'enable --now'.
    settings.save(home)?;
    if settings.automatic {
        output(Command::new("systemctl").args(["--user", "enable", "--now", TIMER]))?;
        log("Automatyczne aktualizacje zostały włączone.");
    } else {
        output(Command::new("systemctl").args(["--user", "disable", "--now", TIMER]))?;
        log("Automatyczne aktualizacje zostały wyłączone.");
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
}
