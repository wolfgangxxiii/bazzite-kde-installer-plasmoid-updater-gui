//! Headless companion copied from AppImage into ~/.local/bin.
//! Runs in a systemd --user service without launching GTK or requiring AppImage.
#[path = "../backend.rs"]
mod backend;
#[path = "../settings.rs"]
mod settings;

use std::process::ExitCode;

fn notify(summary: &str, message: &str) {
    // Optional desktop integration. Never fail a systemd cycle because a
    // notification daemon or notify-send is unavailable (e.g. logged out).
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=plasmoid-updater", "--", summary, message])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--version"] {
        println!("plasmoid-updater-agent {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.as_slice() != ["run"] {
        eprintln!("Usage: plasmoid-updater-agent run|--version");
        return ExitCode::from(2);
    }

    let mut notification_preferences = (false, true);
    let result = (|| {
        let home = backend::home()?;
        let settings = settings::Settings::load(&home)?;
        notification_preferences = (settings.notify_on_success, settings.notify_on_error);
        backend::execute(backend::Job::Update, settings, &mut |message| {
            println!("{message}")
        })
    })();

    match result {
        Ok(()) => {
            println!("Zakończono automatyczny cykl aktualizacji.");
            if notification_preferences.0 {
                notify(
                    "plasmoid-updater",
                    "Automatyczny cykl aktualizacji zakończony.",
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Automatyczne aktualizacje: {error:#}");
            if notification_preferences.1 {
                notify(
                    "Błąd aktualizacji dodatków Plasma",
                    "Sprawdź dziennik usługi plasmoid-updater.service.",
                );
            }
            ExitCode::FAILURE
        }
    }
}
