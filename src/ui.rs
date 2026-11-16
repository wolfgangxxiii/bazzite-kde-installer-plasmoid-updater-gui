use crate::backend::{self, AvailableAddonUpdate, InstalledAddon, Job, UpdateOverview};
use crate::settings::Settings;
use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum Task {
    Operation(Job),
    Inventory,
    AvailableUpdates,
}

impl Task {
    fn cancellable(self) -> bool {
        matches!(
            self,
            Self::Inventory
                | Self::AvailableUpdates
                | Self::Operation(Job::Check | Job::DryRun | Job::Diagnose | Job::Journal)
        )
    }
}

enum Message {
    Progress(String),
    Inventory(Vec<InstalledAddon>, bool),
    AvailableUpdates(UpdateOverview),
    Done(Result<(), String>),
}

#[derive(Clone)]
struct UiParts {
    status: gtk::Label,
    automation: gtk::Label,
    output: gtk::TextBuffer,
    toast: adw::ToastOverlay,
    activity: gtk::Label,
    spinner: gtk::Spinner,
    cancel_button: gtk::Button,
    cancel_token: Rc<RefCell<Option<Arc<AtomicBool>>>>,
    dirty: Rc<Cell<bool>>,
    unsaved: gtk::Label,
    buttons: Rc<Vec<gtk::Button>>,
    settings_inputs: Rc<Vec<gtk::Widget>>,
    busy: Rc<Cell<bool>>,
    inventory: Rc<RefCell<Vec<InstalledAddon>>>,
    partial_system: Rc<Cell<bool>>,
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

fn empty_list_message(list: &gtk::ListBox, message: &str) {
    let label = gtk::Label::new(Some(message));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label.set_margin_top(20);
    label.set_margin_bottom(20);
    label.set_margin_start(16);
    label.set_margin_end(16);
    list.append(&label);
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
        "Znaleziono: {} użytkownika · {} systemowych · {} widocznych{}",
        user_count,
        system_count,
        visible.len(),
        if parts.partial_system.get() {
            " · UWAGA: skan systemowych nie powiódł się"
        } else {
            ""
        }
    ));
    if visible.is_empty() {
        empty_list_message(
            &parts.inventory_list,
            if inventory.is_empty() {
                "Nie znaleziono dodatków. Uruchom skanowanie, aby sprawdzić instalację KDE."
            } else {
                "Brak dodatków pasujących do wyszukiwania i wybranego filtru."
            },
        );
    }
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
        row.set_title_lines(2);
        row.set_subtitle_lines(2);
        let version = gtk::Label::new(Some(&format!("v{}", addon_version(&addon.version))));
        version.add_css_class("dim-label");
        version.set_valign(gtk::Align::Center);
        version.set_max_width_chars(14);
        version.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.add_suffix(&version);
        parts.inventory_list.append(&row);
    }
}

fn render_updates(parts: &UiParts, overview: UpdateOverview) {
    clear_list(&parts.updates_list);
    parts.updates_count.set_text(&format!(
        "{} aktualizacji · {} problemów · {} pominiętych · {} bez dopasowania",
        overview.available.len(),
        overview.issues.len(),
        overview.skipped,
        overview.unmatched,
    ));
    let mut updates = overview.available;
    updates.sort_by_key(|item| item.installed.name.to_lowercase());
    if updates.is_empty() && overview.issues.is_empty() {
        empty_list_message(
            &parts.updates_list,
            "Nie wykryto dostępnych aktualizacji dodatków.",
        );
    }
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
        row.set_title_lines(2);
        row.set_subtitle_lines(2);
        parts.updates_list.append(&row);
    }
    for issue in overview.issues {
        let row = adw::ActionRow::builder()
            .title(issue.name)
            .subtitle(issue.reason)
            .build();
        row.add_css_class("error");
        let warning = gtk::Image::from_icon_name("dialog-warning-symbolic");
        row.add_prefix(&warning);
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
    for input in parts.settings_inputs.iter() {
        input.set_sensitive(false);
    }
    parts.activity.set_text("Trwa operacja…");
    parts.spinner.start();
    parts.cancel_button.set_sensitive(task.cancellable());
    let cancel = Arc::new(AtomicBool::new(false));
    *parts.cancel_token.borrow_mut() = Some(cancel.clone());
    append_log(&parts.output, &format!("▶ {task:?}"));

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = backend::with_cancel(cancel, || {
            (|| -> anyhow::Result<()> {
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
                        let system = backend::installed_addons(true);
                        let incomplete = system.is_err();
                        if let Ok(extra) = system.as_ref() {
                            addons.extend(extra.iter().cloned());
                        }
                        let _ = sender.send(Message::Inventory(addons, incomplete));
                        if let Err(error) = system {
                            anyhow::bail!("Lista dodatków użytkownika jest dostępna, ale odczyt dodatków systemowych się nie powiódł: {error:#}");
                        }
                        Ok(())
                    }
                    Task::AvailableUpdates => {
                        let _ = sender.send(Message::AvailableUpdates(
                            backend::available_addon_updates()?,
                        ));
                        Ok(())
                    }
                }
            })()
        });
        let _ = sender.send(Message::Done(result.map_err(|error| format!("{error:#}"))));
    });

    glib::timeout_add_local(Duration::from_millis(120), move || {
        loop {
            match receiver.try_recv() {
                Ok(Message::Progress(line)) => {
                    append_log(&parts.output, &line);
                    let preview = line.lines().last().unwrap_or(&line);
                    parts
                        .activity
                        .set_text(&preview.chars().take(100).collect::<String>());
                }
                Ok(Message::Inventory(addons, incomplete)) => {
                    parts.partial_system.set(incomplete);
                    *parts.inventory.borrow_mut() = addons;
                    render_inventory(&parts);
                }
                Ok(Message::AvailableUpdates(overview)) => render_updates(&parts, overview),
                Ok(Message::Done(result)) => {
                    let succeeded = result.is_ok();
                    match result {
                        Ok(()) => {
                            append_log(&parts.output, "✓ Operacja zakończona pomyślnie.");
                            parts.toast.add_toast(adw::Toast::new("Gotowe"));
                        }
                        Err(error) => {
                            append_log(&parts.output, &format!("✗ {error}"));
                            parts.toast.add_toast(adw::Toast::new(
                                if error.contains("Anulowano operację") {
                                    "Anulowano operację"
                                } else {
                                    "Operacja nie powiodła się — sprawdź dziennik"
                                },
                            ));
                        }
                    }
                    parts.spinner.stop();
                    parts.activity.set_text(if succeeded {
                        "Gotowe"
                    } else {
                        "Sprawdź dziennik"
                    });
                    parts.cancel_button.set_sensitive(false);
                    *parts.cancel_token.borrow_mut() = None;
                    parts.status.set_text(&backend::status());
                    parts.automation.set_text(&backend::automation_report());
                    for button in parts.buttons.iter() {
                        button.set_sensitive(true);
                    }
                    for input in parts.settings_inputs.iter() {
                        input.set_sensitive(true);
                    }
                    parts.busy.set(false);
                    if succeeded && matches!(task, Task::Operation(Job::Save { .. })) {
                        parts.dirty.set(false);
                        parts.unsaved.set_visible(false);
                    }
                    if matches!(task, Task::Operation(Job::Update | Job::RunService)) {
                        parts.updates.borrow_mut().clear();
                        clear_list(&parts.updates_list);
                        parts
                            .updates_count
                            .set_text("Po aktualizacji sprawdź dostępne wersje ponownie.");
                        launch(parts.clone(), Task::Inventory, Settings::default());
                    }
                    return glib::ControlFlow::Break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    append_log(&parts.output, "Przerwano zadanie bez wyniku.");
                    parts.spinner.stop();
                    parts.activity.set_text("Przerwano zadanie");
                    parts.cancel_button.set_sensitive(false);
                    *parts.cancel_token.borrow_mut() = None;
                    for button in parts.buttons.iter() {
                        button.set_sensitive(true);
                    }
                    for input in parts.settings_inputs.iter() {
                        input.set_sensitive(true);
                    }
                    parts.busy.set(false);
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

// Keep actions on their own line: suffix buttons on AdwActionRow caused
// overly-wide minimum sizes and overlapping controls in narrow windows.
fn action_row(title: &str, subtitle: &str, button: &gtk::Button) -> gtk::Box {
    let card = info_card(title, subtitle);
    button.set_halign(gtk::Align::End);
    button.set_margin_start(16);
    button.set_margin_end(16);
    button.set_margin_bottom(14);
    card.append(button);
    card
}

fn info_card(title: &str, subtitle: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("card");
    card.set_margin_bottom(4);
    card.set_spacing(10);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 5);
    body.set_margin_top(14);
    body.set_margin_bottom(8);
    body.set_margin_start(16);
    body.set_margin_end(16);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    heading.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.append(&heading);
    let detail = gtk::Label::new(Some(subtitle));
    detail.add_css_class("dim-label");
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.append(&detail);
    card.append(&body);
    card
}

fn switch_row(title: &str, subtitle: &str, active: bool) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(active)
        .build();
    row.set_title_lines(2);
    row.set_subtitle_lines(3);
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
    let content = gtk::Box::new(gtk::Orientation::Vertical, 20);
    content.set_margin_top(28);
    content.set_margin_bottom(28);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("title-1");
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    content.append(&heading);
    let intro = gtk::Label::new(Some(subtitle));
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    intro.add_css_class("dim-label");
    content.append(&intro);
    content
}

fn add_page(stack: &gtk::Stack, key: &str, title: &str, content: &gtk::Box) {
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(840);
    clamp.set_tightening_threshold(620);
    clamp.set_child(Some(content));
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let page = stack.add_titled(&scroll, Some(key), title);
    page.set_icon_name(match key {
        "dashboard" => "view-dashboard-symbolic",
        "installation" => "system-software-install-symbolic",
        "addons" => "application-x-addon-symbolic",
        "automation" => "alarm-symbolic",
        _ => "text-x-generic-symbolic",
    });
}

pub fn build(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Instalator plasmoid-updater • Bazzite")
        .default_width(1020)
        .default_height(740)
        .build();

    let overlay = adw::ToastOverlay::new();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let header_title = adw::WindowTitle::new("Plasmoid Updater", "Instalator dla Bazzite KDE");
    header.set_title_widget(Some(&header_title));
    let navigation_toggle = gtk::ToggleButton::new();
    navigation_toggle.set_icon_name("sidebar-show-symbolic");
    navigation_toggle.set_active(true);
    navigation_toggle.set_tooltip_text(Some("Pokaż lub ukryj menu nawigacji"));
    header.pack_start(&navigation_toggle);
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Odśwież stan instalacji"));
    header.pack_end(&refresh);

    // Long operation messages belong in a dedicated status bar, not the header.
    let cancel_button = gtk::Button::with_label("Anuluj");
    cancel_button.set_tooltip_text(Some(
        "Anuluje tylko bezpieczne zadania odczytu i diagnostyki",
    ));
    cancel_button.set_sensitive(false);
    let spinner = gtk::Spinner::new();
    let activity = gtk::Label::new(Some("Gotowe"));
    activity.add_css_class("dim-label");
    activity.set_xalign(0.0);
    activity.set_hexpand(true);
    activity.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    status_bar.set_margin_start(20);
    status_bar.set_margin_end(20);
    status_bar.set_margin_top(8);
    status_bar.set_margin_bottom(8);
    status_bar.append(&spinner);
    status_bar.append(&activity);
    status_bar.append(&cancel_button);
    toolbar.add_bottom_bar(&status_bar);

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_size_request(190, -1);
    sidebar.add_css_class("navigation-sidebar");
    let navigation = gtk::Box::new(gtk::Orientation::Vertical, 8);
    navigation.set_margin_top(20);
    let navigation_heading = gtk::Label::new(Some("NAWIGACJA"));
    navigation_heading.add_css_class("caption");
    navigation_heading.add_css_class("dim-label");
    navigation_heading.set_xalign(0.0);
    navigation_heading.set_margin_start(16);
    navigation_heading.set_margin_bottom(10);
    navigation.append(&navigation_heading);
    navigation.append(&sidebar);
    let sidebar_revealer = gtk::Revealer::new();
    sidebar_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    sidebar_revealer.set_reveal_child(true);
    sidebar_revealer.set_child(Some(&navigation));
    let sidebar_separator = gtk::Separator::new(gtk::Orientation::Vertical);
    let revealer_for_toggle = sidebar_revealer.clone();
    navigation_toggle.connect_toggled(move |toggle| {
        revealer_for_toggle.set_reveal_child(toggle.is_active());
    });
    let toggle_for_revealer = navigation_toggle.clone();
    let separator_for_revealer = sidebar_separator.clone();
    sidebar_revealer.connect_reveal_child_notify(move |revealer| {
        separator_for_revealer.set_visible(revealer.reveals_child());
        if toggle_for_revealer.is_active() != revealer.reveals_child() {
            toggle_for_revealer.set_active(revealer.reveals_child());
        }
    });

    let revealer_for_page = sidebar_revealer.clone();
    let window_for_page = window.clone();
    stack.connect_visible_child_name_notify(move |_| {
        if window_for_page.width() <= 820 {
            revealer_for_page.set_reveal_child(false);
        }
    });

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar_revealer);
    layout.append(&sidebar_separator);
    layout.append(&stack);
    let adaptive_layout = adw::BreakpointBin::new();
    adaptive_layout.set_child(Some(&layout));
    let narrow = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 820px")
            .expect("Prawidłowy warunek szerokości okna"),
    );
    narrow.add_setter(&sidebar_revealer, "reveal-child", Some(&false.to_value()));
    adaptive_layout.add_breakpoint(narrow);
    toolbar.set_content(Some(&adaptive_layout));
    overlay.set_child(Some(&toolbar));
    window.set_content(Some(&overlay));

    let dashboard = page(
        "Pulpit",
        "Wszystko, czego potrzebujesz do konfiguracji aktualizacji dodatków KDE.",
    );

    let welcome = info_card(
        "Zacznij w trzech krokach",
        "1. Sprawdź środowisko i zainstaluj plasmoid-updater.\n2. Przejrzyj dodatki Plasma i dostępne aktualizacje.\n3. W Automatyzacji ustaw harmonogram i przetestuj konfigurację.",
    );
    dashboard.append(&welcome);

    let environment = adw::PreferencesGroup::new();
    environment.set_title("Stan instalacji");
    let diagnose_button = button("Sprawdź składniki", false);
    environment.add(&action_row(
        "Diagnostyka środowiska",
        "Bazzite, KDE, curl, Homebrew, bsdtar, kpackagetool6, systemd i instalacja",
        &diagnose_button,
    ));
    let status = gtk::Label::new(Some(&backend::status()));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
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
    let inventory_filters = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let inventory_search = gtk::SearchEntry::new();
    inventory_search.set_placeholder_text(Some("Szukaj nazwy lub rodzaju dodatku"));
    inventory_search.set_hexpand(true);
    let inventory_scope = gtk::DropDown::from_strings(&["Wszystkie", "Użytkownika", "Systemowe"]);
    inventory_scope.set_halign(gtk::Align::Start);
    inventory_filters.append(&inventory_search);
    inventory_filters.append(&inventory_scope);
    addons_group.add(&inventory_filters);
    let inventory_count = gtk::Label::new(Some("Nie skanowano jeszcze dodatków."));
    inventory_count.set_xalign(0.0);
    inventory_count.set_wrap(true);
    inventory_count.add_css_class("dim-label");
    addons_group.add(&inventory_count);
    let inventory_list = gtk::ListBox::new();
    inventory_list.set_selection_mode(gtk::SelectionMode::None);
    inventory_list.add_css_class("boxed-list");
    empty_list_message(
        &inventory_list,
        "Kliknij „Skanuj dodatki”, aby wyświetlić zainstalowane komponenty.",
    );
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
    updates_count.set_wrap(true);
    updates_count.add_css_class("dim-label");
    updates_group.add(&updates_count);
    let updates_list = gtk::ListBox::new();
    updates_list.set_selection_mode(gtk::SelectionMode::None);
    updates_list.add_css_class("boxed-list");
    empty_list_message(
        &updates_list,
        "Kliknij „Sprawdź wersje”, aby zobaczyć dostępne aktualizacje.",
    );
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
    automation.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    automation.set_selectable(true);
    automation.set_margin_top(12);
    automation.set_margin_bottom(12);
    automation_group.add(&automation);
    automation_page.append(&automation_group);

    let settings_group = adw::PreferencesGroup::new();
    settings_group.set_title("Harmonogram aktualizacji");
    let behavior_group = adw::PreferencesGroup::new();
    behavior_group.set_title("Dodatki i powiadomienia");
    let migration_group = adw::PreferencesGroup::new();
    migration_group.set_title("Ustawienia zaawansowane");
    let apply_group = adw::PreferencesGroup::new();
    apply_group.set_title("Zapis i testowanie");
    let initial = match backend::home().and_then(|h| backend::initial_settings(&h)) {
        Ok(settings) => settings,
        Err(error) => {
            status.set_text(&format!("{}\nUstawienia: {error:#}", backend::status()));
            Settings::default()
        }
    };
    let automatic = switch_row(
        "Automatyczne aktualizacje w tle",
        "Zadanie systemd uruchamia się według harmonogramu",
        initial.automatic,
    );
    settings_group.add(&automatic);

    let auto_update_tool = switch_row(
        "Aktualizuj również plasmoid-updater",
        "Każdy cykl sprawdza wydanie na GitHub i pobiera program tylko, gdy jest nowszy",
        initial.auto_update_tool,
    );
    settings_group.add(&auto_update_tool);

    let frequency = gtk::DropDown::from_strings(&["Co 6 godzin", "Co 12 godzin", "Raz dziennie"]);
    frequency.set_halign(gtk::Align::Start);
    frequency.set_selected(match initial.frequency_hours {
        6 => 0,
        12 => 1,
        _ => 2,
    });
    let frequency_row = info_card(
        "Harmonogram",
        "Pominięty termin jest nadrabiany po zalogowaniu; opóźnienie do 20 minut",
    );
    frequency.set_margin_start(16);
    frequency.set_margin_end(16);
    frequency.set_margin_bottom(14);
    frequency_row.append(&frequency);
    settings_group.add(&frequency_row);

    let restart = switch_row(
        "Restart Plasma po automatycznej aktualizacji",
        "Domyślnie wyłączony — restart mógłby przerwać sesję KDE",
        initial.restart_plasma,
    );
    behavior_group.add(&restart);

    let notify_success = switch_row(
        "Powiadamiaj po zakończonym cyklu",
        "Wymaga dostępnego notify-send i sesji powiadomień KDE",
        initial.notify_on_success,
    );
    behavior_group.add(&notify_success);
    let notify_error = switch_row(
        "Powiadamiaj o błędach aktualizacji",
        "Domyślnie włączone; brak usługi powiadomień nie zatrzymuje aktualizacji",
        initial.notify_on_error,
    );
    behavior_group.add(&notify_error);

    let migrate_legacy = switch_row(
        "Przejmij istniejącą usługę i harmonogram plasmoid-updater",
        "Zapisze kopie starych jednostek, wyłączy poprzedni timer i skonfiguruje nowy",
        false,
    );
    migration_group.add(&migrate_legacy);

    let exclusions = gtk::Entry::new();
    exclusions.set_text(&initial.excluded_packages.join(", "));
    exclusions.set_placeholder_text(Some("np. moja-wtyczka, drugi-motyw"));
    exclusions.set_hexpand(true);
    exclusions.set_margin_start(16);
    exclusions.set_margin_end(16);
    exclusions.set_margin_bottom(14);
    let exclusions_card = info_card(
        "Wykluczone dodatki",
        "Nazwy komponentów, których nie należy aktualizować. Oddziel je przecinkami.",
    );
    exclusions_card.append(&exclusions);
    behavior_group.add(&exclusions_card);
    let unsaved = gtk::Label::new(Some(
        "Niezapisane zmiany — najpierw kliknij «Zapisz i zastosuj».",
    ));
    unsaved.add_css_class("warning");
    unsaved.set_xalign(0.0);
    unsaved.set_wrap(true);
    unsaved.set_visible(false);
    apply_group.add(&unsaved);
    let save_button = button("Zapisz i zastosuj", true);
    let save_row = action_row(
        "Zapisz ustawienia",
        "Zapisuje TOML i włącza lub wyłącza timer systemd użytkownika",
        &save_button,
    );
    apply_group.add(&save_row);

    let test_button = button("Przetestuj", false);
    apply_group.add(&action_row(
        "Test końcowy konfiguracji",
        "Sprawdza TOML, narzędzia, agenta, systemd oraz dry-run bez zmian w dodatkach",
        &test_button,
    ));
    let run_service_button = button("Uruchom teraz", false);
    apply_group.add(&action_row(
        "Przetestuj pełny cykl aktualizacji",
        "Uruchamia usługę systemd i instaluje dostępne aktualizacje (wykonuje zmiany)",
        &run_service_button,
    ));
    automation_page.append(&settings_group);
    automation_page.append(&behavior_group);
    automation_page.append(&migration_group);
    automation_page.append(&apply_group);
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

    let dirty = Rc::new(Cell::new(false));
    let mark_dirty: Rc<dyn Fn()> = {
        let dirty = dirty.clone();
        let unsaved = unsaved.clone();
        Rc::new(move || {
            dirty.set(true);
            unsaved.set_visible(true);
        })
    };
    let marker = mark_dirty.clone();
    automatic.connect_active_notify(move |_| marker());
    let marker = mark_dirty.clone();
    auto_update_tool.connect_active_notify(move |_| marker());
    let marker = mark_dirty.clone();
    frequency.connect_selected_notify(move |_| marker());
    let marker = mark_dirty.clone();
    restart.connect_active_notify(move |_| marker());
    let marker = mark_dirty.clone();
    notify_success.connect_active_notify(move |_| marker());
    let marker = mark_dirty.clone();
    notify_error.connect_active_notify(move |_| marker());
    let marker = mark_dirty.clone();
    migrate_legacy.connect_active_notify(move |_| marker());
    let marker = mark_dirty;
    exclusions.connect_changed(move |_| marker());

    let parts = UiParts {
        status,
        automation,
        output,
        toast: overlay,
        activity,
        spinner,
        cancel_button: cancel_button.clone(),
        cancel_token: Rc::new(RefCell::new(None)),
        dirty,
        unsaved,
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
        settings_inputs: Rc::new(vec![
            automatic.clone().upcast(),
            auto_update_tool.clone().upcast(),
            frequency.clone().upcast(),
            restart.clone().upcast(),
            notify_success.clone().upcast(),
            notify_error.clone().upcast(),
            migrate_legacy.clone().upcast(),
            exclusions.clone().upcast(),
        ]),
        busy: Rc::new(Cell::new(false)),
        inventory: Rc::new(RefCell::new(Vec::new())),
        partial_system: Rc::new(Cell::new(false)),
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
    let cancel_parts = parts.clone();
    cancel_button.connect_clicked(move |_| {
        if let Some(token) = cancel_parts.cancel_token.borrow().as_ref() {
            token.store(true, Ordering::Relaxed);
            cancel_parts.cancel_button.set_sensitive(false);
            cancel_parts.activity.set_text("Anulowanie zadania…");
        }
    });

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
        notify_on_success: notify_success.is_active(),
        notify_on_error: notify_error.is_active(),
        excluded_packages: Settings::parse_exclusions(exclusions.text().as_str()),
    });
    let parts_update = parts.clone();
    let read_update = read_form.clone();
    update_button.connect_clicked(move |_| {
        if parts_update.dirty.get() {
            parts_update.toast.add_toast(adw::Toast::new(
                "Najpierw zapisz zmienione ustawienia automatyzacji i wykluczeń",
            ));
            return;
        }
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
        if parts_service.dirty.get() {
            parts_service.toast.add_toast(adw::Toast::new(
                "Zapisz ustawienia przed uruchomieniem cyklu",
            ));
            return;
        }
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
