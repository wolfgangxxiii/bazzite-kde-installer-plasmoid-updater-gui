//! Headless companion copied from AppImage into ~/.local/bin.
//! Runs in a systemd --user service without launching GTK or requiring AppImage.
#[path = "../backend.rs"]
mod backend;
#[path = "../settings.rs"]
mod settings;

use std::process::ExitCode;

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

    let result = (|| {
        let home = backend::home()?;
        let settings = settings::Settings::load(&home)?;
        backend::execute(backend::Job::Update, settings, &mut |message| {
            println!("{message}")
        })
    })();

    match result {
        Ok(()) => {
            println!("Zakończono automatyczny cykl aktualizacji.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Automatyczne aktualizacje: {error:#}");
            ExitCode::FAILURE
        }
    }
}
