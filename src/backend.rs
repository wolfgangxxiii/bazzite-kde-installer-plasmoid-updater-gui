use crate::settings::Settings;
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use toml_edit::{value, Array, DocumentMut};

pub const UPSTREAM: &str = "https://github.com/uwuclxdy/plasmoid-updater";
const RELEASE_URL: &str = "https://github.com/uwuclxdy/plasmoid-updater/releases/latest/download";
const ASSET: &str = "plasmoid-updater-linux-x86_64";
const SERVICE: &str = "bazzite-plasmoid-updater.service";
const TIMER: &str = "bazzite-plasmoid-updater.timer";
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

fn user_path(home: &Path) -> String {
    format!(
        "{}:/home/linuxbrew/.linuxbrew/opt/libarchive/bin:/home/linuxbrew/.linuxbrew/bin:{}",
        home.join(".local/bin").display(),
        env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin".to_owned())
    )
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
    let bsdtar = home.join(".local/bin/bsdtar").exists()
        || Path::new("/usr/bin/bsdtar").exists()
        || Path::new("/home/linuxbrew/.linuxbrew/opt/libarchive/bin/bsdtar").exists();
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
        Job::Install => install_updater(&home, log),
        Job::Check => run_updater(&home, &["check"], true, log),
        Job::DryRun => run_updater(&home, &["update", "--dry-run"], false, log),
        Job::Update => run_updater(
            &home,
            &["update", "--yes", "--no-restart-plasma"],
            false,
            log,
        ),
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

fn expected_checksum(content: &str, filename: &str) -> Result<String> {
    let matched: Vec<&str> = content
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let digest = columns.next()?;
            let file = columns.next()?;
            (file.trim_start_matches('*') == filename).then_some(digest)
        })
        .collect();
    if matched.len() != 1
        || matched[0].len() != 64
        || !matched[0].bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("Brak prawidłowej, pojedynczej sumy SHA-256 dla {filename}");
    }
    Ok(matched[0].to_ascii_lowercase())
}

fn verify_checksum(binary: &Path, sums: &Path) -> Result<()> {
    let expected = expected_checksum(&fs::read_to_string(sums)?, ASSET)?;
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
        actual == expected,
        "Błędna suma SHA-256 — instalacja przerwana"
    );
    Ok(())
}

fn install_updater(home: &Path, log: &mut dyn FnMut(&str)) -> Result<()> {
    anyhow::ensure!(
        env::consts::ARCH == "x86_64",
        "Wydanie obsługuje obecnie tylko x86_64"
    );
    let temporary = tempfile::tempdir().context("Nie można utworzyć katalogu tymczasowego")?;
    let binary = temporary.path().join(ASSET);
    let sums = temporary.path().join("sha256sums.txt");
    log("Pobieranie pliku wykonywalnego z GitHub Releases…");
    download(&format!("{RELEASE_URL}/{ASSET}"), &binary)?;
    log("Pobieranie sum kontrolnych…");
    download(&format!("{RELEASE_URL}/sha256sums.txt"), &sums)?;
    verify_checksum(&binary, &sums)?;
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

fn run_updater(home: &Path, args: &[&str], check: bool, log: &mut dyn FnMut(&str)) -> Result<()> {
    let binary = updater_binary(home);
    anyhow::ensure!(binary.is_file(), "Najpierw zainstaluj plasmoid-updater");
    if args.first() == Some(&"update")
        && !Path::new("/usr/bin/bsdtar").exists()
        && !home.join(".local/bin/bsdtar").exists()
        && !Path::new("/home/linuxbrew/.linuxbrew/opt/libarchive/bin/bsdtar").exists()
    {
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
    let restart = if settings.restart_plasma {
        "--restart-plasma"
    } else {
        "--no-restart-plasma"
    };
    let service = format!(
        "{MANAGED}\n[Unit]\nDescription=Update user KDE Store addons\n\n[Service]\nType=oneshot\nEnvironment=PATH=%h/.local/bin:/home/linuxbrew/.linuxbrew/opt/libarchive/bin:/home/linuxbrew/.linuxbrew/bin:/usr/local/bin:/usr/bin\nExecStart=%h/.local/bin/plasmoid-updater --skip-plasma-detection update --yes {restart}\nNice=10\n"
    );
    let timer = format!(
        "{MANAGED}\n[Unit]\nDescription=Schedule KDE Store addon updates\n\n[Timer]\nOnCalendar={}\nRandomizedDelaySec=20min\nPersistent=true\nUnit={SERVICE}\n\n[Install]\nWantedBy=timers.target\n",
        timer_schedule(settings.frequency_hours)?
    );

    log("Zapisywanie konfiguracji z zachowaniem istniejących komentarzy…");
    atomic_write(&config_path, doc.to_string().as_bytes())?;
    guarded_write(&service_path, &service)?;
    guarded_write(&timer_path, &timer)?;

    log("Odświeżanie usług użytkownika systemd…");
    output(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    if settings.automatic {
        anyhow::ensure!(
            updater_binary(home).is_file(),
            "Zapisano konfigurację; przed włączeniem timera zainstaluj program"
        );
        output(Command::new("systemctl").args(["--user", "enable", "--now", TIMER]))?;
        log("Automatyczne aktualizacje zostały włączone.");
    } else {
        output(Command::new("systemctl").args(["--user", "disable", "--now", TIMER]))?;
        log("Automatyczne aktualizacje zostały wyłączone.");
    }
    settings.save(home)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_must_match_exact_filename() {
        let checksums = format!("{}  other\n{} *{ASSET}\n", "a".repeat(64), "b".repeat(64));
        assert_eq!(
            expected_checksum(&checksums, ASSET).unwrap(),
            "b".repeat(64)
        );
        assert!(expected_checksum(&checksums, "not-here").is_err());
    }

    #[test]
    fn checksum_duplicate_is_rejected() {
        let line = format!("{}  {ASSET}\n", "a".repeat(64));
        assert!(expected_checksum(&format!("{line}{line}"), ASSET).is_err());
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
}
