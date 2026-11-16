use crate::backend::{self, Job};
use crate::settings::Settings;
use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

enum Message {
    Progress(String),
    Done(Result<(), String>),
}

#[derive(Clone)]
struct UiParts {
    status: gtk::Label,
    output: gtk::TextBuffer,
    toast: adw::ToastOverlay,
    buttons: Rc<Vec<gtk::Button>>,
    busy: Rc<Cell<bool>>,
}

fn append_log(buffer: &gtk::TextBuffer, message: &str) {
    let mut end = buffer.end_iter();
    if buffer.char_count() > 0 {
        buffer.insert(&mut end, "\n");
    }
    buffer.insert(&mut end, message);
}

fn launch(parts: UiParts, job: Job, settings: Settings) {
    if parts.busy.replace(true) {
        return;
    }
    for button in parts.buttons.iter() {
        button.set_sensitive(false);
    }
    append_log(&parts.output, &format!("▶ {job:?}"));

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = backend::execute(job, settings, &mut |line| {
            let _ = sender.send(Message::Progress(line.to_owned()));
        });
        let _ = sender.send(Message::Done(result.map_err(|error| format!("{error:#}"))));
    });

    glib::timeout_add_local(Duration::from_millis(120), move || {
        loop {
            match receiver.try_recv() {
                Ok(Message::Progress(line)) => append_log(&parts.output, &line),
                Ok(Message::Done(result)) => {
                    match result {
                        Ok(()) => {
                            append_log(&parts.output, "✓ Operacja zakończona pomyślnie.");
                            parts.toast.add_toast(adw::Toast::new("Gotowe"));
                        }
                        Err(error) => {
                            append_log(&parts.output, &format!("✗ {error}"));
                            parts.toast.add_toast(adw::Toast::new(
                                "Operacja nie powiodła się — sprawdź dziennik",
                            ));
                        }
                    }
                    parts.status.set_text(&backend::status());
                    for button in parts.buttons.iter() {
                        button.set_sensitive(true);
                    }
                    parts.busy.set(false);
                    return glib::ControlFlow::Break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    append_log(&parts.output, "Przerwano zadanie bez wyniku.");
                    for button in parts.buttons.iter() {
                        button.set_sensitive(true);
                    }
                    parts.busy.set(false);
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

fn action_row(title: &str, subtitle: &str, button: &gtk::Button) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    row.add_suffix(button);
    row.set_activatable_widget(Some(button));
    row
}

fn button(label: &str, suggested: bool) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_valign(gtk::Align::Center);
    if suggested {
        button.add_css_class("suggested-action");
    }
    button
}

pub fn build(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Instalator plasmoid-updater • Bazzite")
        .default_width(760)
        .default_height(840)
        .build();

    let overlay = adw::ToastOverlay::new();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let header_title = adw::WindowTitle::new(
        "Instalator plasmoid-updater",
        "Bazzite KDE • instalacja i automatyzacja",
    );
    header.set_title_widget(Some(&header_title));
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Odśwież stan instalacji"));
    header.pack_end(&refresh);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let title = gtk::Label::new(Some("Zarządzaj dodatkami KDE Plasma"));
    title.add_css_class("title-1");
    title.set_xalign(0.0);
    content.append(&title);
    let intro = gtk::Label::new(Some(
        "Instalacja bez rpm-ostree i sudo. Aktualizacje dodatków z KDE Store\nwykonywane w tle przez usługę użytkownika systemd.",
    ));
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    intro.add_css_class("dim-label");
    content.append(&intro);

    let environment = adw::PreferencesGroup::new();
    environment.set_title("Środowisko");
    let diagnose_button = button("Sprawdź składniki", false);
    environment.add(&action_row(
        "Diagnostyka środowiska",
        "Bazzite, KDE, curl, Homebrew, bsdtar, kpackagetool6, systemd i instalacja",
        &diagnose_button,
    ));
    let status = gtk::Label::new(Some(&backend::status()));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_selectable(true);
    status.set_margin_top(8);
    status.set_margin_bottom(8);
    status.add_css_class("caption");
    environment.add(&status);
    content.append(&environment);

    let install_group = adw::PreferencesGroup::new();
    install_group.set_title("Instalacja i aktualizacje");
    let install_button = button("Zainstaluj / uaktualnij", true);
    install_group.add(&action_row(
        "Instaluj plasmoid-updater",
        "Wykrywa istniejącą wersję; pobiera tylko nowszą i weryfikuje SHA-256",
        &install_button,
    ));
    let check_button = button("Sprawdź", false);
    install_group.add(&action_row(
        "Dostępne aktualizacje",
        "Sprawdza wersje dodatków bez dokonywania zmian",
        &check_button,
    ));
    let dry_run_button = button("Symuluj", false);
    install_group.add(&action_row(
        "Symulacja aktualizacji",
        "Wyświetla plan bez instalacji",
        &dry_run_button,
    ));
    let update_button = button("Aktualizuj wszystko teraz", false);
    install_group.add(&action_row(
        "Ręczna aktualizacja",
        "Sprawdza GitHub (opcjonalnie), następnie aktualizuje dodatki KDE Store",
        &update_button,
    ));
    content.append(&install_group);

    let settings_group = adw::PreferencesGroup::new();
    settings_group.set_title("Automatyzacja");
    let initial = match backend::home().and_then(|h| backend::initial_settings(&h)) {
        Ok(settings) => settings,
        Err(error) => {
            status.set_text(&format!("{}\nUstawienia: {error:#}", backend::status()));
            Settings::default()
        }
    };
    let automatic = adw::SwitchRow::builder()
        .title("Automatyczne aktualizacje w tle")
        .subtitle("Zadanie systemd uruchamia się według harmonogramu")
        .active(initial.automatic)
        .build();
    settings_group.add(&automatic);

    let auto_update_tool = adw::SwitchRow::builder()
        .title("Aktualizuj również plasmoid-updater")
        .subtitle("Każdy cykl sprawdza wydanie na GitHub i pobiera program tylko, gdy jest nowszy")
        .active(initial.auto_update_tool)
        .build();
    settings_group.add(&auto_update_tool);

    let frequency = gtk::DropDown::from_strings(&["Co 6 godzin", "Co 12 godzin", "Raz dziennie"]);
    frequency.set_valign(gtk::Align::Center);
    frequency.set_selected(match initial.frequency_hours {
        6 => 0,
        12 => 1,
        _ => 2,
    });
    let frequency_row = adw::ActionRow::builder()
        .title("Harmonogram")
        .subtitle("Pominięty termin jest nadrabiany po zalogowaniu; opóźnienie do 20 minut")
        .build();
    frequency_row.add_suffix(&frequency);
    settings_group.add(&frequency_row);

    let restart = adw::SwitchRow::builder()
        .title("Restart Plasma po automatycznej aktualizacji")
        .subtitle("Domyślnie wyłączony — restart mógłby przerwać sesję KDE")
        .active(initial.restart_plasma)
        .build();
    settings_group.add(&restart);

    let migrate_legacy = adw::SwitchRow::builder()
        .title("Przejmij istniejącą usługę i harmonogram plasmoid-updater")
        .subtitle("Zapisze kopie starych jednostek, wyłączy poprzedni timer i skonfiguruje nowy")
        .active(false)
        .build();
    settings_group.add(&migrate_legacy);

    let exclusions = adw::EntryRow::builder()
        .title("Wykluczone dodatki (oddziel przecinkami)")
        .text(initial.excluded_packages.join(", "))
        .build();
    settings_group.add(&exclusions);
    let save_button = button("Zapisz i zastosuj", true);
    let save_row = adw::ActionRow::builder()
        .title("Zapisz ustawienia")
        .subtitle("Zapisuje TOML i włącza lub wyłącza timer systemd użytkownika")
        .build();
    save_row.add_suffix(&save_button);
    save_row.set_activatable_widget(Some(&save_button));
    settings_group.add(&save_row);

    let test_button = button("Przetestuj", false);
    settings_group.add(&action_row(
        "Test końcowy konfiguracji",
        "Sprawdza TOML, narzędzia, agenta, systemd oraz dry-run bez zmian w dodatkach",
        &test_button,
    ));
    let run_service_button = button("Uruchom teraz", false);
    settings_group.add(&action_row(
        "Przetestuj pełny cykl aktualizacji",
        "Uruchamia usługę systemd i instaluje dostępne aktualizacje (wykonuje zmiany)",
        &run_service_button,
    ));
    content.append(&settings_group);

    let log_group = adw::PreferencesGroup::new();
    log_group.set_title("Dziennik operacji");
    let log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(10)
        .right_margin(10)
        .top_margin(10)
        .bottom_margin(10)
        .build();
    let output = log_view.buffer();
    output.set_text("Gotowe. Sprawdź składniki, zainstaluj program i zapisz harmonogram. Na końcu wykonaj test.");
    let log_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(160)
        .max_content_height(260)
        .vexpand(false)
        .child(&log_view)
        .build();
    log_scroll.add_css_class("card");
    log_group.add(&log_scroll);
    content.append(&log_group);

    let footer = gtk::LinkButton::with_label(backend::UPSTREAM, "Źródło plasmoid-updater");
    footer.set_halign(gtk::Align::Start);
    content.append(&footer);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scroll));
    overlay.set_child(Some(&toolbar));
    window.set_content(Some(&overlay));

    let parts = UiParts {
        status: status.clone(),
        output,
        toast: overlay,
        buttons: Rc::new(vec![
            diagnose_button.clone(),
            install_button.clone(),
            check_button.clone(),
            dry_run_button.clone(),
            update_button.clone(),
            save_button.clone(),
            test_button.clone(),
            run_service_button.clone(),
        ]),
        busy: Rc::new(Cell::new(false)),
    };
    refresh.connect_clicked(move |_| status.set_text(&backend::status()));

    let parts_diagnose = parts.clone();
    diagnose_button.connect_clicked(move |_| {
        launch(parts_diagnose.clone(), Job::Diagnose, Settings::default())
    });

    let parts_install = parts.clone();
    install_button
        .connect_clicked(move |_| launch(parts_install.clone(), Job::Install, Settings::default()));
    let parts_check = parts.clone();
    check_button
        .connect_clicked(move |_| launch(parts_check.clone(), Job::Check, Settings::default()));
    let parts_dry = parts.clone();
    dry_run_button
        .connect_clicked(move |_| launch(parts_dry.clone(), Job::DryRun, Settings::default()));
    let read_form: Rc<dyn Fn() -> Settings> = Rc::new(move || Settings {
        automatic: automatic.is_active(),
        auto_update_tool: auto_update_tool.is_active(),
        frequency_hours: match frequency.selected() {
            0 => 6,
            1 => 12,
            _ => 24,
        },
        restart_plasma: restart.is_active(),
        excluded_packages: Settings::parse_exclusions(exclusions.text().as_str()),
    });
    let parts_update = parts.clone();
    let read_update = read_form.clone();
    update_button
        .connect_clicked(move |_| launch(parts_update.clone(), Job::Update, read_update()));
    let read_test = read_form.clone();
    let parts_test = parts.clone();
    test_button.connect_clicked(move |_| launch(parts_test.clone(), Job::Test, read_test()));
    let parts_service = parts.clone();
    run_service_button.connect_clicked(move |_| {
        launch(parts_service.clone(), Job::RunService, Settings::default())
    });
    save_button.connect_clicked(move |_| {
        launch(
            parts.clone(),
            Job::Save {
                migrate_legacy: migrate_legacy.is_active(),
            },
            read_form(),
        );
    });
    window.present();
}
