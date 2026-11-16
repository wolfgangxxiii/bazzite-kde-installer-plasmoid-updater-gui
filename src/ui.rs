use crate::backend::{self, AvailableAddonUpdate, InstalledAddon, Job, UpdateOverview};
use crate::settings::Settings;
use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum Task {
    Operation(Job),
    Inventory,
    AvailableUpdates,
}

enum Message {
    Progress(String),
    Inventory(Vec<InstalledAddon>),
    AvailableUpdates(UpdateOverview),
    Done(Result<(), String>),
}

#[derive(Clone)]
struct UiParts {
    status: gtk::Label,
    automation: gtk::Label,
    output: gtk::TextBuffer,
    toast: adw::ToastOverlay,
    buttons: Rc<Vec<gtk::Button>>,
    busy: Rc<Cell<bool>>,
    inventory: Rc<RefCell<Vec<InstalledAddon>>>,
    updates: Rc<RefCell<Vec<AvailableAddonUpdate>>>,
    inventory_list: gtk::ListBox,
    updates_list: gtk::ListBox,
    inventory_count: gtk::Label,
    updates_count: gtk::Label,
    inventory_search: gtk::SearchEntry,
    inventory_scope: gtk::DropDown,
}

fn append_log(buffer: &gtk::TextBuffer, message: &str) {
    let mut end = buffer.end_iter();
    if buffer.char_count() > 0 {
        buffer.insert(&mut end, "\n");
    }
    buffer.insert(&mut end, message);
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn addon_scope(addon: &InstalledAddon) -> &'static str {
    if addon.is_system {
        "Systemowy (tylko odczyt)"
    } else {
        "Użytkownika"
    }
}

fn addon_version(version: &str) -> &str {
    if version.is_empty() || version == "0.0.0" {
        "nieznana"
    } else {
        version
    }
}

fn render_inventory(parts: &UiParts) {
    clear_list(&parts.inventory_list);
    let search = parts.inventory_search.text().to_lowercase();
    let scope = parts.inventory_scope.selected();
    let inventory = parts.inventory.borrow();
    let updates = parts.updates.borrow();
    let visible: Vec<_> = inventory
        .iter()
        .filter(|addon| match scope {
            1 => !addon.is_system,
            2 => addon.is_system,
            _ => true,
        })
        .filter(|addon| {
            search.is_empty()
                || addon.name.to_lowercase().contains(&search)
                || addon.directory_name.to_lowercase().contains(&search)
                || addon.component_type.to_lowercase().contains(&search)
        })
        .collect();
    let user_count = inventory.iter().filter(|addon| !addon.is_system).count();
    let system_count = inventory.len() - user_count;
    parts.inventory_count.set_text(&format!(
        "Znaleziono: {} użytkownika · {} systemowych · {} widocznych",
        user_count,
        system_count,
        visible.len()
    ));
    for addon in visible {
        let update = updates
            .iter()
            .find(|item| item.installed.path == addon.path);
        let description = match update {
            Some(item) => format!(
                "{} · {} · Dostępna wersja: {}\n{}",
                addon_scope(addon),
                addon.component_type,
                item.latest_version,
                addon.path
            ),
            None => format!(
                "{} · {}\n{}",
                addon_scope(addon),
                addon.component_type,
                addon.path
            ),
        };
        let row = adw::ActionRow::builder()
            .title(addon.name.as_str())
            .subtitle(description.as_str())
            .build();
        let version = gtk::Label::new(Some(&format!("v{}", addon_version(&addon.version))));
        version.add_css_class("dim-label");
        version.set_valign(gtk::Align::Center);
        row.add_suffix(&version);
        parts.inventory_list.append(&row);
    }
}

fn render_updates(parts: &UiParts, overview: UpdateOverview) {
    clear_list(&parts.updates_list);
    parts.updates_count.set_text(&format!(
        "{} aktualizacji · {} komunikatów diagnostycznych",
        overview.available.len(),
        overview.diagnostics
    ));
    let mut updates = overview.available;
    updates.sort_by_key(|item| item.installed.name.to_lowercase());
    for update in &updates {
        let row = adw::ActionRow::builder()
            .title(update.installed.name.as_str())
            .subtitle(format!(
                "{} → {} · {}",
                addon_version(&update.installed.version),
                update.latest_version,
                addon_scope(&update.installed)
            ))
            .build();
        parts.updates_list.append(&row);
    }
    *parts.updates.borrow_mut() = updates;
    render_inventory(parts);
}

fn launch(parts: UiParts, task: Task, settings: Settings) {
    if parts.busy.replace(true) {
        return;
    }
    for button in parts.buttons.iter() {
        button.set_sensitive(false);
    }
    append_log(&parts.output, &format!("▶ {task:?}"));

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<()> {
            match task {
                Task::Operation(job) => backend::execute(job, settings, &mut |line| {
                    let _ = sender.send(Message::Progress(line.to_owned()));
                }),
                Task::Inventory => {
                    let mut addons = backend::installed_addons(false)?;
                    let _ = sender.send(Message::Progress(format!(
                        "Znaleziono {} dodatków użytkownika.",
                        addons.len()
                    )));
                    match backend::installed_addons(true) {
                        Ok(system) => addons.extend(system),
                        Err(error) => {
                            let _ = sender.send(Message::Progress(format!(
                                "Nie odczytano dodatków systemowych: {error:#}"
                            )));
                        }
                    }
                    let _ = sender.send(Message::Inventory(addons));
                    Ok(())
                }
                Task::AvailableUpdates => {
                    let _ = sender.send(Message::AvailableUpdates(
                        backend::available_addon_updates()?,
                    ));
                    Ok(())
                }
            }
        })();
        let _ = sender.send(Message::Done(result.map_err(|error| format!("{error:#}"))));
    });

    glib::timeout_add_local(Duration::from_millis(120), move || {
        loop {
            match receiver.try_recv() {
                Ok(Message::Progress(line)) => append_log(&parts.output, &line),
                Ok(Message::Inventory(addons)) => {
                    *parts.inventory.borrow_mut() = addons;
                    render_inventory(&parts);
                }
                Ok(Message::AvailableUpdates(overview)) => render_updates(&parts, overview),
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
                    parts.automation.set_text(&backend::automation_report());
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

fn page(title: &str, subtitle: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(30);
    content.set_margin_end(30);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("title-1");
    heading.set_xalign(0.0);
    content.append(&heading);
    let intro = gtk::Label::new(Some(subtitle));
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    intro.add_css_class("dim-label");
    content.append(&intro);
    content
}

fn add_page(stack: &gtk::Stack, key: &str, title: &str, content: &gtk::Box) {
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(content)
        .build();
    stack.add_titled(&scroll, Some(key), title);
}

pub fn build(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Instalator plasmoid-updater • Bazzite")
        .default_width(1080)
        .default_height(760)
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

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_size_request(210, -1);
    sidebar.add_css_class("navigation-sidebar");
    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    layout.append(&stack);
    toolbar.set_content(Some(&layout));
    overlay.set_child(Some(&toolbar));
    window.set_content(Some(&overlay));

    let dashboard = page(
        "Pulpit",
        "Stan środowiska Bazzite KDE, instalacji plasmoid-updater i wymaganych narzędzi.",
    );

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
    dashboard.append(&environment);
    let footer =
        gtk::LinkButton::with_label(backend::UPSTREAM, "Projekt źródłowy plasmoid-updater");
    footer.set_halign(gtk::Align::Start);
    dashboard.append(&footer);
    add_page(&stack, "dashboard", "Pulpit", &dashboard);

    let installation = page(
        "Instalacja",
        "Instalacja z GitHub i ręczna obsługa aktualizacji przez oryginalny plasmoid-updater.",
    );

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
    installation.append(&install_group);
    add_page(&stack, "installation", "Instalacja", &installation);

    let addons_page = page(
        "Dodatki Plasma",
        "Komponenty wykrywane przez plasmoid-updater. Dodatki systemowe są wyświetlane tylko do odczytu.",
    );
    let addons_group = adw::PreferencesGroup::new();
    addons_group.set_title("Zainstalowane dodatki KDE Store");
    let scan_button = button("Skanuj dodatki", true);
    addons_group.add(&action_row(
        "Odczytaj dodatki użytkownika i systemowe",
        "Skanowanie niczego nie instaluje, nie usuwa ani nie modyfikuje",
        &scan_button,
    ));
    let inventory_filters = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let inventory_search = gtk::SearchEntry::new();
    inventory_search.set_placeholder_text(Some("Szukaj nazwy lub rodzaju dodatku"));
    inventory_search.set_hexpand(true);
    let inventory_scope = gtk::DropDown::from_strings(&["Wszystkie", "Użytkownika", "Systemowe"]);
    inventory_filters.append(&inventory_search);
    inventory_filters.append(&inventory_scope);
    addons_group.add(&inventory_filters);
    let inventory_count = gtk::Label::new(Some("Nie skanowano jeszcze dodatków."));
    inventory_count.set_xalign(0.0);
    inventory_count.add_css_class("dim-label");
    addons_group.add(&inventory_count);
    let inventory_list = gtk::ListBox::new();
    inventory_list.set_selection_mode(gtk::SelectionMode::None);
    inventory_list.add_css_class("boxed-list");
    addons_group.add(&inventory_list);
    addons_page.append(&addons_group);

    let updates_group = adw::PreferencesGroup::new();
    updates_group.set_title("Dostępne aktualizacje");
    let scan_updates_button = button("Sprawdź wersje", false);
    updates_group.add(&action_row(
        "Sprawdź aktualizacje KDE Store",
        "Odczytuje dostępne wersje przez plasmoid-updater check --json",
        &scan_updates_button,
    ));
    let updates_count = gtk::Label::new(Some("Nie sprawdzano jeszcze aktualizacji."));
    updates_count.set_xalign(0.0);
    updates_count.add_css_class("dim-label");
    updates_group.add(&updates_count);
    let updates_list = gtk::ListBox::new();
    updates_list.set_selection_mode(gtk::SelectionMode::None);
    updates_list.add_css_class("boxed-list");
    updates_group.add(&updates_list);
    addons_page.append(&updates_group);
    add_page(&stack, "addons", "Dodatki Plasma", &addons_page);

    let automation_page = page(
        "Automatyzacja",
        "Aktualizowanie dodatków oraz opcjonalnie samego programu w tle przez systemd --user.",
    );
    let automation_group = adw::PreferencesGroup::new();
    automation_group.set_title("Stan harmonogramu");
    let automation = gtk::Label::new(Some(&backend::automation_report()));
    automation.set_xalign(0.0);
    automation.set_wrap(true);
    automation.set_selectable(true);
    automation.set_margin_top(12);
    automation.set_margin_bottom(12);
    automation_group.add(&automation);
    automation_page.append(&automation_group);

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
    automation_page.append(&settings_group);
    add_page(&stack, "automation", "Automatyzacja", &automation_page);

    let journal = page(
        "Dziennik",
        "Wyniki operacji instalatora oraz historia usługi plasmoid-updater.service.",
    );
    let log_group = adw::PreferencesGroup::new();
    log_group.set_title("Dziennik operacji");
    let journal_button = button("Wczytaj historię systemd", false);
    log_group.add(&action_row(
        "Ostatnie 100 wpisów usługi",
        "journalctl --user -u plasmoid-updater.service",
        &journal_button,
    ));
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
        .min_content_height(420)
        .vexpand(true)
        .child(&log_view)
        .build();
    log_scroll.add_css_class("card");
    log_group.add(&log_scroll);
    journal.append(&log_group);
    add_page(&stack, "journal", "Dziennik", &journal);

    let parts = UiParts {
        status,
        automation,
        output,
        toast: overlay,
        buttons: Rc::new(vec![
            diagnose_button.clone(),
            install_button.clone(),
            check_button.clone(),
            dry_run_button.clone(),
            update_button.clone(),
            scan_button.clone(),
            scan_updates_button.clone(),
            save_button.clone(),
            test_button.clone(),
            run_service_button.clone(),
            journal_button.clone(),
        ]),
        busy: Rc::new(Cell::new(false)),
        inventory: Rc::new(RefCell::new(Vec::new())),
        updates: Rc::new(RefCell::new(Vec::new())),
        inventory_list,
        updates_list,
        inventory_count,
        updates_count,
        inventory_search: inventory_search.clone(),
        inventory_scope: inventory_scope.clone(),
    };
    let refresh_parts = parts.clone();
    refresh.connect_clicked(move |_| {
        refresh_parts.status.set_text(&backend::status());
        refresh_parts
            .automation
            .set_text(&backend::automation_report());
    });
    let search_parts = parts.clone();
    inventory_search.connect_search_changed(move |_| render_inventory(&search_parts));
    let scope_parts = parts.clone();
    inventory_scope.connect_selected_notify(move |_| render_inventory(&scope_parts));

    let parts_diagnose = parts.clone();
    diagnose_button.connect_clicked(move |_| {
        launch(
            parts_diagnose.clone(),
            Task::Operation(Job::Diagnose),
            Settings::default(),
        )
    });

    let parts_install = parts.clone();
    install_button.connect_clicked(move |_| {
        launch(
            parts_install.clone(),
            Task::Operation(Job::Install),
            Settings::default(),
        )
    });
    let parts_check = parts.clone();
    check_button.connect_clicked(move |_| {
        launch(
            parts_check.clone(),
            Task::Operation(Job::Check),
            Settings::default(),
        )
    });
    let parts_dry = parts.clone();
    dry_run_button.connect_clicked(move |_| {
        launch(
            parts_dry.clone(),
            Task::Operation(Job::DryRun),
            Settings::default(),
        )
    });
    let parts_scan = parts.clone();
    scan_button
        .connect_clicked(move |_| launch(parts_scan.clone(), Task::Inventory, Settings::default()));
    let parts_scan_updates = parts.clone();
    scan_updates_button.connect_clicked(move |_| {
        launch(
            parts_scan_updates.clone(),
            Task::AvailableUpdates,
            Settings::default(),
        )
    });
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
    update_button.connect_clicked(move |_| {
        launch(
            parts_update.clone(),
            Task::Operation(Job::Update),
            read_update(),
        )
    });
    let read_test = read_form.clone();
    let parts_test = parts.clone();
    test_button.connect_clicked(move |_| {
        launch(parts_test.clone(), Task::Operation(Job::Test), read_test())
    });
    let parts_service = parts.clone();
    run_service_button.connect_clicked(move |_| {
        launch(
            parts_service.clone(),
            Task::Operation(Job::RunService),
            Settings::default(),
        )
    });
    let parts_journal = parts.clone();
    journal_button.connect_clicked(move |_| {
        launch(
            parts_journal.clone(),
            Task::Operation(Job::Journal),
            Settings::default(),
        )
    });
    save_button.connect_clicked(move |_| {
        launch(
            parts.clone(),
            Task::Operation(Job::Save {
                migrate_legacy: migrate_legacy.is_active(),
            }),
            read_form(),
        );
    });
    window.present();
}
