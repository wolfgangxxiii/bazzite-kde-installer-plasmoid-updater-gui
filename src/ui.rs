use crate::backend::{self, AvailableAddonUpdate, InstalledAddon, Job, UpdateOverview};
use crate::i18n::{t, Language};
use crate::settings::Settings;
use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
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

    fn completion_message(self) -> &'static str {
        match self {
            Self::Operation(Job::Journal) => t(
                "✓ Dziennik odczytany. Sprawdź w nim wynik aktualizacji.",
                "✓ Journal loaded. Check the entries for the update result.",
            ),
            Self::Inventory => t("✓ Lista dodatków odczytana.", "✓ Addon inventory loaded."),
            Self::AvailableUpdates => t(
                "✓ Sprawdzono dostępne aktualizacje.",
                "✓ Available updates checked.",
            ),
            _ => t(
                "✓ Operacja zakończona pomyślnie.",
                "✓ Operation completed successfully.",
            ),
        }
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
    status: gtk::Box,
    automation: gtk::Label,
    output: gtk::TextBuffer,
    toast: adw::ToastOverlay,
    activity: gtk::Label,
    status_bar: gtk::Box,
    spinner: gtk::Spinner,
    cancel_button: gtk::Button,
    cancel_token: Rc<RefCell<Option<Arc<AtomicBool>>>>,
    dirty: Rc<Cell<bool>>,
    unsaved: gtk::Label,
    buttons: Rc<Vec<gtk::Button>>,
    settings_inputs: Rc<Vec<gtk::Widget>>,
    busy: Rc<Cell<bool>>,
    status_generation: Rc<Cell<u64>>,
    inventory: Rc<RefCell<Vec<InstalledAddon>>>,
    partial_system: Rc<Cell<bool>>,
    updates: Rc<RefCell<Vec<AvailableAddonUpdate>>>,
    update_overview: Rc<RefCell<Option<UpdateOverview>>>,
    inventory_list: gtk::ListBox,
    updates_list: gtk::ListBox,
    inventory_count: gtk::Label,
    updates_count: gtk::Label,
    inventory_search: gtk::SearchEntry,
    inventory_scope: gtk::DropDown,
    environment_value: gtk::Label,
    updater_value: gtk::Label,
    agent_value: gtk::Label,
    timer_value: gtk::Label,
    repair_button: gtk::Button,
    repair_notice: gtk::Box,
}

struct ViewState {
    width: i32,
    height: i32,
    maximized: bool,
    inventory: Vec<InstalledAddon>,
    partial_system: bool,
    updates: Option<UpdateOverview>,
    search: String,
    scope: u32,
    log: String,
}

fn english() -> bool {
    Language::selected() == Language::English
}

fn refresh_status(parts: &UiParts) {
    // Several systemctl and --version calls are required for this report.
    // Never run them on the GTK event loop; ignore outdated refresh results.
    let generation = parts.status_generation.get().wrapping_add(1);
    parts.status_generation.set(generation);
    let is_english = english();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let summary = backend::dashboard_summary();
        let status = backend::status_localized(is_english);
        let automation = backend::automation_report_localized(is_english);
        let _ = sender.send((summary, status, automation));
    });
    let parts = parts.clone();
    glib::timeout_add_local(Duration::from_millis(120), move || {
        match receiver.try_recv() {
            Ok((summary, status, automation)) => {
                if parts.status_generation.get() == generation {
                    apply_status(&parts, summary, &status, &automation);
                }
                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn apply_status(
    parts: &UiParts,
    summary: backend::DashboardSummary,
    status: &str,
    automation: &str,
) {
    let repair_needed = summary.agent_managed
        && summary.agent_version.as_deref() != Some(env!("CARGO_PKG_VERSION"));
    parts.environment_value.remove_css_class("success");
    parts.environment_value.remove_css_class("warning");
    parts
        .environment_value
        .add_css_class(if summary.environment_ready {
            "success"
        } else {
            "warning"
        });
    parts
        .environment_value
        .set_text(if summary.environment_ready {
            t("Gotowe", "Ready")
        } else {
            t("Wymaga uwagi", "Needs attention")
        });
    parts.updater_value.set_text(
        &summary
            .updater_version
            .map(|version| format!("v{version}"))
            .unwrap_or_else(|| t("Nie zainstalowano", "Not installed").into()),
    );
    parts.agent_value.set_text(&match summary.agent_version {
        Some(version) if version == env!("CARGO_PKG_VERSION") => {
            format!("v{version} · {}", t("Aktualny", "Up to date"))
        }
        Some(version) => format!("v{version} · {}", t("Wymaga naprawy", "Repair needed")),
        None => t("Brak agenta", "Agent missing").into(),
    });
    parts.timer_value.remove_css_class("warning");
    if summary.last_service_failed {
        parts.timer_value.add_css_class("warning");
    }
    parts
        .timer_value
        .set_text(if summary.timer_active && summary.last_service_failed {
            t(
                "Aktywny · ostatni cykl nieudany",
                "Active · last update failed",
            )
        } else if summary.timer_active {
            t("Aktywny", "Active")
        } else if summary.last_service_failed {
            t(
                "Nieaktywny · ostatni cykl nieudany",
                "Inactive · last update failed",
            )
        } else {
            t("Nieaktywny", "Inactive")
        });
    parts.repair_notice.set_visible(repair_needed);
    parts
        .repair_button
        .set_sensitive(repair_needed && !parts.busy.get());
    render_status_checks(&parts.status, status);
    parts.automation.set_text(automation);
}

fn append_log(buffer: &gtk::TextBuffer, message: &str) {
    let mut end = buffer.end_iter();
    if buffer.char_count() > 0 {
        buffer.insert(&mut end, "\n");
    }
    buffer.insert(&mut end, message);
}

// A diagnostic item gets its own row and semantic colour. In particular, the
// seven environment checks must not run together in two long paragraphs.
fn diagnostic_row(text: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("diagnostic-row");
    let (symbol, body, state) = if let Some(label) = text.strip_prefix("✓ ") {
        ("✓", label, "diagnostic-pass")
    } else if let Some(label) = text.strip_prefix("! ") {
        ("✕", label, "diagnostic-fail")
    } else {
        ("•", text, "diagnostic-neutral")
    };
    let marker = gtk::Label::new(Some(symbol));
    marker.add_css_class(state);
    marker.add_css_class("heading");
    marker.set_width_chars(2);
    marker.set_valign(gtk::Align::Start);
    row.append(&marker);

    let label = gtk::Label::new(Some(body));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    if state != "diagnostic-neutral" {
        label.add_css_class(state);
    }
    row.append(&label);
    row
}

fn render_status_checks(container: &gtk::Box, report: &str) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    for (index, line) in report.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        // Split checklist rows, configuration/agent, and timer/next-run into
        // independent items; keep paths and long values intact and wrappable.
        let grouped = index < 2
            || ((line.starts_with("✓ ") || line.starts_with("! ")) && line.contains(" · "))
            || line.starts_with("Konfiguracja:")
            || line.starts_with("Configuration:")
            || line.starts_with("Timer:");
        if grouped {
            for entry in line.split(" · ") {
                container.append(&diagnostic_row(entry));
            }
        } else {
            container.append(&diagnostic_row(line));
        }
    }
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
        t("Systemowy (tylko odczyt)", "System (read-only)")
    } else {
        t("Użytkownika", "User")
    }
}

fn addon_version(version: &str) -> &str {
    if version.is_empty() || version == "0.0.0" {
        t("nieznana", "unknown")
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
        "{}: {} {} · {} {} · {} {}{}",
        t("Znaleziono", "Found"),
        user_count,
        t("użytkownika", "user"),
        system_count,
        t("systemowych", "system"),
        visible.len(),
        t("widocznych", "visible"),
        if parts.partial_system.get() {
            t(
                " · UWAGA: skan systemowych nie powiódł się",
                " · WARNING: system scan failed",
            )
        } else {
            ""
        }
    ));
    if visible.is_empty() {
        empty_list_message(
            &parts.inventory_list,
            if inventory.is_empty() {
                t(
                    "Nie znaleziono dodatków. Uruchom skanowanie, aby sprawdzić instalację KDE.",
                    "No addons found. Run a scan to check your KDE installation.",
                )
            } else {
                t(
                    "Brak dodatków pasujących do wyszukiwania i wybranego filtru.",
                    "No addons match the search and selected filter.",
                )
            },
        );
    }
    for addon in visible {
        let update = updates.iter().find(|item| {
            if !item.installed.path.is_empty() && !addon.path.is_empty() {
                item.installed.path == addon.path
            } else {
                item.installed.name == addon.name
                    && item.installed.component_type == addon.component_type
                    && item.installed.is_system == addon.is_system
            }
        });
        let description = match update {
            Some(item) => format!(
                "{} · {} · {}: {}{}",
                addon_scope(addon),
                addon.component_type,
                t("Dostępna wersja", "Available version"),
                item.latest_version,
                if addon.path.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", addon.path)
                }
            ),
            None => format!(
                "{} · {}{}",
                addon_scope(addon),
                addon.component_type,
                if addon.path.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", addon.path)
                }
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
    *parts.update_overview.borrow_mut() = Some(overview.clone());
    clear_list(&parts.updates_list);
    parts.updates_count.set_text(&format!(
        "{} {} · {} {} · {} {} · {} {}",
        overview.available.len(),
        t("aktualizacji", "updates"),
        overview.issues.len(),
        t("problemów", "issues"),
        overview.skipped,
        t("pominiętych", "skipped"),
        overview.unmatched,
        t("bez dopasowania", "unmatched"),
    ));
    let mut updates = overview.available;
    updates.sort_by_key(|item| item.installed.name.to_lowercase());
    if updates.is_empty() && overview.issues.is_empty() {
        empty_list_message(
            &parts.updates_list,
            t(
                "Nie wykryto dostępnych aktualizacji dodatków.",
                "No addon updates were found.",
            ),
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
        let warning = vector_icon(UiIcon::Warning);
        warning.add_css_class("error");
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
    parts
        .activity
        .set_text(t("Trwa operacja…", "Operation in progress…"));
    parts.status_bar.set_visible(true);
    parts.spinner.start();
    parts.cancel_button.set_sensitive(task.cancellable());
    parts.cancel_button.set_visible(task.cancellable());
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
                            "{} {} {}.",
                            t("Znaleziono", "Found"),
                            addons.len(),
                            t("dodatków użytkownika", "user addons")
                        )));
                        let system = backend::installed_addons(true);
                        let incomplete = system.is_err();
                        if let Ok(extra) = system.as_ref() {
                            addons.extend(extra.iter().cloned());
                        }
                        let _ = sender.send(Message::Inventory(addons, incomplete));
                        if let Err(error) = system {
                            anyhow::bail!("{}: {error:#}", t("Lista dodatków użytkownika jest dostępna, ale odczyt dodatków systemowych się nie powiódł", "User addons are available, but the system addon scan failed"));
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
                    if english() {
                        parts.activity.set_text("Working… details in the log");
                    } else {
                        let preview = line.lines().last().unwrap_or(&line);
                        parts
                            .activity
                            .set_text(&preview.chars().take(100).collect::<String>());
                    }
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
                            append_log(&parts.output, task.completion_message());
                            let toast_text = if matches!(task, Task::Operation(Job::Journal)) {
                                t("Dziennik odczytany", "Journal loaded")
                            } else {
                                t("Gotowe", "Done")
                            };
                            parts.toast.add_toast(adw::Toast::new(toast_text));
                        }
                        Err(error) => {
                            append_log(&parts.output, &format!("✗ {error}"));
                            parts.toast.add_toast(adw::Toast::new(
                                if error.contains("Anulowano operację") {
                                    t("Anulowano operację", "Operation cancelled")
                                } else {
                                    t(
                                        "Operacja nie powiodła się — sprawdź dziennik",
                                        "Operation failed — check the log",
                                    )
                                },
                            ));
                        }
                    }
                    parts.spinner.stop();
                    parts.status_bar.set_visible(false);
                    parts.activity.set_text(if succeeded {
                        t("Gotowe", "Done")
                    } else {
                        t("Sprawdź dziennik", "Check the log")
                    });
                    parts.cancel_button.set_sensitive(false);
                    parts.cancel_button.set_visible(false);
                    *parts.cancel_token.borrow_mut() = None;
                    for button in parts.buttons.iter() {
                        button.set_sensitive(true);
                    }
                    for input in parts.settings_inputs.iter() {
                        input.set_sensitive(true);
                    }
                    parts.busy.set(false);
                    // Keep this action disabled until the fresh agent status arrives.
                    parts.repair_button.set_sensitive(false);
                    refresh_status(&parts);
                    if succeeded && matches!(task, Task::Operation(Job::Save { .. })) {
                        parts.dirty.set(false);
                        parts.unsaved.set_visible(false);
                    }
                    if !succeeded && matches!(task, Task::AvailableUpdates) {
                        parts.updates_count.set_text(t(
                            "Nie udało się sprawdzić KDE Store · poprzednie wyniki mogą być nieaktualne",
                            "KDE Store check failed · previous results may be outdated",
                        ));
                    }
                    if matches!(task, Task::Operation(Job::Update | Job::RunService)) {
                        if succeeded {
                            parts.updates.borrow_mut().clear();
                            parts.update_overview.borrow_mut().take();
                            clear_list(&parts.updates_list);
                            parts.updates_count.set_text(t(
                                "Po aktualizacji sprawdź dostępne wersje ponownie.",
                                "Check available versions again after the update.",
                            ));
                        } else {
                            parts.updates_count.set_text(t(
                                "Cykl nieudany · zachowano poprzednią listę (może być nieaktualna)",
                                "Update failed · previous results kept (may be outdated)",
                            ));
                        }
                        launch(parts.clone(), Task::Inventory, Settings::default());
                    }
                    return glib::ControlFlow::Break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    append_log(
                        &parts.output,
                        t(
                            "Przerwano zadanie bez wyniku.",
                            "Task stopped without a result.",
                        ),
                    );
                    parts.spinner.stop();
                    parts.status_bar.set_visible(false);
                    parts
                        .activity
                        .set_text(t("Przerwano zadanie", "Task stopped"));
                    parts.cancel_button.set_sensitive(false);
                    parts.cancel_button.set_visible(false);
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

// These icons are drawn locally instead of relying on the host's icon theme.
// Some Bazzite/AppImage combinations do not provide the stock symbolic names.
#[derive(Clone, Copy)]
enum UiIcon {
    Dashboard,
    Install,
    Addons,
    Schedule,
    Journal,
    About,
    Computer,
    Updater,
    Agent,
    Menu,
    Refresh,
    ChevronDown,
    ChevronUp,
    Warning,
    ExternalLink,
}

fn vector_icon(kind: UiIcon) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(18);
    icon.set_content_height(18);
    icon.set_size_request(18, 18);
    icon.set_draw_func(move |area, cr, width, height| {
        let color = area.color();
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        cr.scale(width as f64 / 24.0, height as f64 / 24.0);
        cr.set_line_width(2.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        match kind {
            UiIcon::Dashboard => {
                for (x, y, w, h) in [
                    (3.0, 3.0, 8.0, 8.0),
                    (13.0, 3.0, 8.0, 5.0),
                    (3.0, 13.0, 8.0, 8.0),
                    (13.0, 10.0, 8.0, 11.0),
                ] {
                    cr.rectangle(x, y, w, h);
                }
            }
            UiIcon::Install => {
                cr.move_to(12.0, 3.0);
                cr.line_to(12.0, 15.0);
                cr.move_to(7.0, 10.0);
                cr.line_to(12.0, 15.0);
                cr.line_to(17.0, 10.0);
                cr.move_to(4.0, 17.0);
                cr.line_to(4.0, 21.0);
                cr.line_to(20.0, 21.0);
                cr.line_to(20.0, 17.0);
            }
            UiIcon::Addons => {
                for (x, y) in [(3.0, 3.0), (14.0, 3.0), (3.0, 14.0), (14.0, 14.0)] {
                    cr.rectangle(x, y, 7.0, 7.0);
                }
            }
            UiIcon::Schedule => {
                cr.arc(12.0, 12.0, 9.0, 0.0, std::f64::consts::TAU);
                cr.move_to(12.0, 6.0);
                cr.line_to(12.0, 12.0);
                cr.line_to(16.0, 14.0);
            }
            UiIcon::Journal => {
                cr.move_to(15.0, 3.0);
                cr.line_to(6.0, 3.0);
                cr.line_to(6.0, 21.0);
                cr.line_to(19.0, 21.0);
                cr.line_to(19.0, 7.0);
                cr.close_path();
                cr.move_to(15.0, 3.0);
                cr.line_to(15.0, 7.0);
                cr.line_to(19.0, 7.0);
                cr.move_to(9.0, 12.0);
                cr.line_to(16.0, 12.0);
                cr.move_to(9.0, 16.0);
                cr.line_to(16.0, 16.0);
            }
            UiIcon::About => {
                cr.arc(12.0, 12.0, 9.0, 0.0, std::f64::consts::TAU);
                cr.move_to(12.0, 11.0);
                cr.line_to(12.0, 17.0);
                cr.move_to(12.0, 7.0);
                cr.line_to(12.0, 7.1);
            }
            UiIcon::Computer => {
                cr.rectangle(3.0, 4.0, 18.0, 13.0);
                cr.move_to(12.0, 17.0);
                cr.line_to(12.0, 21.0);
                cr.move_to(8.0, 21.0);
                cr.line_to(16.0, 21.0);
            }
            UiIcon::Updater => {
                cr.arc(12.0, 12.0, 8.0, 3.75, 6.6);
                cr.move_to(17.0, 4.0);
                cr.line_to(20.0, 7.0);
                cr.line_to(20.0, 3.0);
                cr.new_sub_path();
                cr.arc(12.0, 12.0, 8.0, 0.6, 3.45);
                cr.move_to(7.0, 20.0);
                cr.line_to(4.0, 17.0);
                cr.line_to(4.0, 21.0);
            }
            UiIcon::Agent => {
                cr.move_to(12.0, 2.0);
                cr.line_to(20.0, 6.0);
                cr.line_to(20.0, 12.0);
                cr.curve_to(20.0, 16.0, 17.0, 20.0, 12.0, 22.0);
                cr.curve_to(7.0, 20.0, 4.0, 16.0, 4.0, 12.0);
                cr.line_to(4.0, 6.0);
                cr.close_path();
                cr.move_to(8.0, 12.0);
                cr.line_to(11.0, 15.0);
                cr.line_to(16.0, 9.0);
            }
            UiIcon::Menu => {
                for y in [6.0, 12.0, 18.0] {
                    cr.move_to(4.0, y);
                    cr.line_to(20.0, y);
                }
            }
            UiIcon::Refresh => {
                // Balanced opposing arcs, with both arrowheads inside 24x24.
                cr.arc(12.0, 12.0, 8.0, 3.85, 6.0);
                cr.move_to(17.0, 3.0);
                cr.line_to(20.0, 6.0);
                cr.line_to(15.0, 6.0);
                cr.new_sub_path();
                cr.arc(12.0, 12.0, 8.0, 0.7, 2.85);
                cr.move_to(7.0, 21.0);
                cr.line_to(4.0, 18.0);
                cr.line_to(9.0, 18.0);
            }
            UiIcon::ChevronDown => {
                cr.move_to(5.0, 9.0);
                cr.line_to(12.0, 16.0);
                cr.line_to(19.0, 9.0);
            }
            UiIcon::ChevronUp => {
                cr.move_to(5.0, 15.0);
                cr.line_to(12.0, 8.0);
                cr.line_to(19.0, 15.0);
            }
            UiIcon::Warning => {
                cr.move_to(12.0, 3.0);
                cr.line_to(22.0, 21.0);
                cr.line_to(2.0, 21.0);
                cr.close_path();
                cr.move_to(12.0, 9.0);
                cr.line_to(12.0, 14.0);
                cr.move_to(12.0, 18.0);
                cr.line_to(12.0, 18.1);
            }
            UiIcon::ExternalLink => {
                cr.move_to(12.0, 4.0);
                cr.line_to(20.0, 4.0);
                cr.line_to(20.0, 12.0);
                cr.move_to(20.0, 4.0);
                cr.line_to(10.0, 14.0);
                cr.move_to(17.0, 14.0);
                cr.line_to(17.0, 20.0);
                cr.line_to(4.0, 20.0);
                cr.line_to(4.0, 7.0);
                cr.line_to(10.0, 7.0);
            }
        }
        let _ = cr.stroke();
    });
    icon
}

// Keep actions in their own row. The previous AdwBreakpointBin could allocate
// the card's desktop height after switching to a vertical layout; on small
// windows that clipped the button and overlapped the following card.
fn action_row(title: &str, subtitle: &str, button: &gtk::Button) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("card");
    card.set_margin_bottom(10);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 3);
    body.set_hexpand(true);
    body.set_margin_top(12);
    body.set_margin_start(16);
    body.set_margin_end(16);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_wrap(true);
    heading.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    heading.set_max_width_chars(46);
    heading.set_xalign(0.0);
    let detail = gtk::Label::new(Some(subtitle));
    detail.add_css_class("dim-label");
    detail.set_wrap(true);
    detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    detail.set_max_width_chars(46);
    detail.set_xalign(0.0);
    body.append(&heading);
    body.append(&detail);
    card.append(&body);

    button.set_halign(gtk::Align::Fill);
    button.set_margin_start(16);
    button.set_margin_end(16);
    button.set_margin_bottom(12);
    card.append(button);
    card
}

fn status_tile(title: &str, kind: UiIcon) -> (gtk::Box, gtk::Label) {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 5);
    tile.add_css_class("card");
    tile.set_margin_bottom(0);
    // Let FlowBox shrink to a single column on narrow and scaled desktops.
    tile.set_hexpand(true);
    let heading_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    heading_line.set_margin_top(12);
    heading_line.set_margin_start(14);
    heading_line.set_margin_end(14);
    let icon = vector_icon(kind);
    icon.add_css_class("dim-label");
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("caption");
    heading.set_wrap(true);
    heading.set_xalign(0.0);
    heading_line.append(&icon);
    heading_line.append(&heading);
    tile.append(&heading_line);
    let value = gtk::Label::new(None);
    value.add_css_class("heading");
    value.set_wrap(true);
    value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value.set_xalign(0.0);
    value.set_margin_start(14);
    value.set_margin_end(14);
    value.set_margin_bottom(12);
    tile.append(&value);
    (tile, value)
}

fn info_card(title: &str, subtitle: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 5);
    card.add_css_class("card");
    card.set_margin_bottom(10);
    card.set_spacing(6);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 5);
    body.set_margin_top(10);
    body.set_margin_bottom(4);
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

// Expanding preferences must not depend on a nested breakpoint. On a narrow
// screen each test stays in one column, with its action below the description.
fn test_action_card(title: &str, subtitle: &str, action: &gtk::Button) -> gtk::Box {
    let card = info_card(title, subtitle);
    action.set_width_request(-1);
    action.set_hexpand(true);
    action.set_halign(gtk::Align::Fill);
    action.set_margin_start(16);
    action.set_margin_end(16);
    action.set_margin_bottom(12);
    card.append(action);
    card
}

// One predictable, theme-independent expander for every page. The toggle
// exposes a large click target and our own drawn chevron; GtkExpander and
// AdwExpanderRow used different (sometimes missing) icon-theme assets.
fn expander_card(title: &str, child: &impl IsA<gtk::Widget>, expanded: bool) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    card.set_margin_bottom(6);

    let toggle = gtk::ToggleButton::new();
    toggle.add_css_class("flat");
    toggle.set_hexpand(true);
    toggle.set_halign(gtk::Align::Fill);
    toggle.set_tooltip_text(Some(title));
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    heading.set_margin_start(16);
    heading.set_margin_end(16);
    heading.set_margin_top(12);
    heading.set_margin_bottom(12);
    let text = gtk::Label::new(Some(title));
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_hexpand(true);
    heading.append(&text);
    let arrows = gtk::Stack::new();
    arrows.set_transition_type(gtk::StackTransitionType::None);
    arrows.add_named(&vector_icon(UiIcon::ChevronDown), Some("down"));
    arrows.add_named(&vector_icon(UiIcon::ChevronUp), Some("up"));
    arrows.set_visible_child_name(if expanded { "up" } else { "down" });
    heading.append(&arrows);
    toggle.set_child(Some(&heading));
    card.append(&toggle);

    let contents = gtk::Box::new(gtk::Orientation::Vertical, 10);
    contents.set_margin_start(12);
    contents.set_margin_end(12);
    contents.set_margin_bottom(12);
    contents.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    contents.append(child);
    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_child(Some(&contents));
    revealer.set_reveal_child(expanded);
    toggle.set_active(expanded);
    card.append(&revealer);
    toggle.connect_toggled(move |toggle| {
        let open = toggle.is_active();
        revealer.set_reveal_child(open);
        arrows.set_visible_child_name(if open { "up" } else { "down" });
    });
    card
}

fn repair_card(title: &str, subtitle: &str, action: &gtk::Button) -> gtk::Box {
    let card = info_card(title, subtitle);
    card.add_css_class("warning");
    action.set_halign(gtk::Align::End);
    action.set_margin_start(16);
    action.set_margin_end(16);
    action.set_margin_bottom(12);
    card.append(action);
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
    let button = gtk::Button::new();
    button.add_css_class("theme-action");
    // Reserve a readable action width in horizontal cards; the narrow
    // breakpoint expands the button to the full card width instead.
    button.set_width_request(160);
    let text = gtk::Label::new(Some(label));
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_justify(gtk::Justification::Center);
    text.set_max_width_chars(28);
    button.set_child(Some(&text));
    button.set_valign(gtk::Align::Center);
    if suggested {
        button.add_css_class("suggested-action");
    }
    button
}

fn page(title: &str, subtitle: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(22);
    content.set_margin_bottom(20);
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

fn about_section(title: &str) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.set_margin_top(4);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("title-3");
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    section.append(&heading);
    section
}

fn about_link(url: &'static str, caption: &str) -> gtk::LinkButton {
    let link = gtk::LinkButton::new(url);
    link.add_css_class("about-link");
    link.set_halign(gtk::Align::Start);
    link.set_tooltip_text(Some(url));
    let contents = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let text = gtk::Label::new(Some(caption));
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_xalign(0.0);
    contents.append(&text);
    contents.append(&vector_icon(UiIcon::ExternalLink));
    link.set_child(Some(&contents));
    link
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
        "about" => "help-about-symbolic",
        _ => "text-x-generic-symbolic",
    });
}

pub fn build(app: &adw::Application) {
    // CI can open every page without depending on sidebar coordinates,
    // which vary with GTK font scaling and accessibility settings.
    let smoke_page = std::env::var("BAZZITE_GUI_SMOKE_PAGE").unwrap_or_default();
    let initial_page = match smoke_page.as_str() {
        "installation" | "addons" | "automation" | "journal" | "about" => smoke_page.as_str(),
        _ => "dashboard",
    };
    build_with_page(app, initial_page, None);
}

fn ini_value<'a>(content: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut current_section = "";
    for line in content.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current_section = name.trim();
        } else if current_section == section {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key {
                    return Some(value.trim());
                }
            }
        }
    }
    None
}

fn parse_accent_color(value: &str) -> Option<(u8, u8, u8)> {
    let value = value.trim().trim_matches('"');
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        return Some((
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ));
    }
    let mut rgb = value.split(',').map(|part| part.trim().parse::<u8>().ok());
    let result = (rgb.next()??, rgb.next()??, rgb.next()??);
    rgb.next().is_none().then_some(result)
}

fn host_accent_color() -> Option<(u8, u8, u8)> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let kdeglobals = fs::read_to_string(config.join("kdeglobals")).ok()?;
    ini_value(&kdeglobals, "General", "AccentColor")
        .and_then(parse_accent_color)
        .or_else(|| {
            ini_value(&kdeglobals, "Colors:Selection", "BackgroundNormal")
                .and_then(parse_accent_color)
        })
}

fn contrast_text_color((red, green, blue): (u8, u8, u8)) -> &'static str {
    let linear = |value: u8| {
        let channel = f64::from(value) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);
    if luminance > 0.179 {
        "#161616"
    } else {
        "#ffffff"
    }
}

thread_local! {
    static APP_STYLE_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

fn apply_host_accent_theme() {
    // Base styles remain neutral and legible in both dark and light GTK themes.
    // Only the accent is taken from KDE; don't hard-code Bazzite blue.
    let mut css = String::from(
        r#"
        .app-header-title { font-size: 16px; font-weight: 750; letter-spacing: .15px; }
        .app-header-subtitle { font-size: 11px; opacity: .73; }
        .app-version-badge { border-radius: 10px; padding: 1px 7px;
                             background-color: rgba(127,127,127,.15); font-size: 10px; font-weight: 700; }
        .about-product-name { font-size: 20px; font-weight: 750; }
        .about-logo { margin: 4px; }
        button.about-link { border-radius: 10px; padding: 5px 10px; min-height: 32px; }
        button.about-link:hover { background-color: rgba(127,127,127,.12); }
        .navigation-heading { font-size: 13px; font-weight: 750; }
        .navigation-caption { font-size: 10px; letter-spacing: .6px; opacity: .68; }
        .navigation-rule { opacity: .45; }
        button.theme-action { border-radius: 10px; min-height: 36px; }
        button.upstream-link { border-radius: 12px; min-height: 36px; padding: 5px 14px;
                               background-color: rgba(127,127,127,.12);
                               border: 1px solid rgba(127,127,127,.25); }
        .diagnostic-row { border-radius: 7px; background-color: rgba(127,127,127,.075);
                          padding: 7px 10px; }
        .diagnostic-pass { color: @success_color; }
        .diagnostic-fail { color: @error_color; }
        .diagnostic-neutral { opacity: .78; }
        "#,
    );
    if let Some((red, green, blue)) = host_accent_color() {
        let foreground = contrast_text_color((red, green, blue));
        let (hover_red, hover_green, hover_blue) = if foreground == "#161616" {
            (
                red.saturating_sub(16),
                green.saturating_sub(16),
                blue.saturating_sub(16),
            )
        } else {
            (
                red.saturating_add(16),
                green.saturating_add(16),
                blue.saturating_add(16),
            )
        };
        css.push_str(&format!(
            r#"
            button.theme-action.suggested-action, button.upstream-link {{
                background-color: rgb({red},{green},{blue});
                color: {foreground};
                border: 1px solid rgba({red},{green},{blue},.8);
            }}
            button.theme-action.suggested-action:hover, button.upstream-link:hover {{
                background-color: rgb({hover_red},{hover_green},{hover_blue});
            }}
            button.theme-action:not(.suggested-action) {{
                border: 1px solid rgba({red},{green},{blue},.40);
            }}
            button.theme-action:not(.suggested-action):hover {{
                background-color: rgba({red},{green},{blue},.13);
            }}
            button.header-action:hover {{
                background-color: rgba({red},{green},{blue},.20);
            }}
            switch:checked {{ background-color: rgb({red},{green},{blue}); }}
            .navigation-sidebar row:selected {{
                background-color: rgba({red},{green},{blue},.20);
            }}
            "#,
        ));
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    APP_STYLE_PROVIDER.with(|cached| {
        let mut cached = cached.borrow_mut();
        let provider = cached.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            provider
        });
        provider.load_from_data(&css);
    });
}

#[cfg(test)]
mod appearance_tests {
    use super::{contrast_text_color, ini_value, parse_accent_color};

    #[test]
    fn reads_only_the_requested_section() {
        let ini =
            "[General]\ncursorTheme=Wrong\n[Mouse]\ncursorTheme = Breeze_Snow\ncursorSize=32\n";
        assert_eq!(ini_value(ini, "Mouse", "cursorTheme"), Some("Breeze_Snow"));
        assert_eq!(ini_value(ini, "Mouse", "cursorSize"), Some("32"));
        assert_eq!(ini_value(ini, "Settings", "cursorTheme"), None);
    }

    #[test]
    fn reads_kde_accent_rgb_and_hex() {
        assert_eq!(parse_accent_color("61, 174, 233"), Some((61, 174, 233)));
        assert_eq!(parse_accent_color("#3daee9"), Some((61, 174, 233)));
        assert_eq!(parse_accent_color("300,0,0"), None);
        assert_eq!(parse_accent_color("0,0,0,1"), None);
        assert_eq!(contrast_text_color((255, 250, 255)), "#161616");
        assert_eq!(contrast_text_color((25, 40, 70)), "#ffffff");
    }
}

fn apply_host_cursor_theme() {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    let Some(config_home) = config_home else {
        return;
    };

    // Plasma's cursor selection is authoritative on KDE/Wayland; a portal
    // can otherwise return a stale Adwaita default to GTK4 AppImages.
    let on_kde = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .is_some_and(|desktop| {
            desktop
                .split(':')
                .any(|name| name.eq_ignore_ascii_case("KDE"))
        });
    let kde_ini = if on_kde {
        fs::read_to_string(config_home.join("kcminputrc")).ok()
    } else {
        None
    };
    let gtk_ini = fs::read_to_string(config_home.join("gtk-4.0/settings.ini")).ok();
    let theme = kde_ini
        .as_deref()
        .and_then(|ini| ini_value(ini, "Mouse", "cursorTheme"))
        .or_else(|| {
            gtk_ini
                .as_deref()
                .and_then(|ini| ini_value(ini, "Settings", "gtk-cursor-theme-name"))
        });
    let size = kde_ini
        .as_deref()
        .and_then(|ini| ini_value(ini, "Mouse", "cursorSize"))
        .or_else(|| {
            gtk_ini
                .as_deref()
                .and_then(|ini| ini_value(ini, "Settings", "gtk-cursor-theme-size"))
        })
        .and_then(|size| size.parse::<i32>().ok())
        .filter(|size| (1..=256).contains(size));

    if let Some(settings) = gtk::Settings::default() {
        if let Some(theme) = theme.filter(|name| !name.is_empty()) {
            settings.set_gtk_cursor_theme_name(Some(theme));
        }
        if let Some(size) = size {
            settings.set_gtk_cursor_theme_size(size);
        }
    }
}

fn build_with_page(app: &adw::Application, initial_page: &str, initial_state: Option<ViewState>) {
    apply_host_cursor_theme();
    apply_host_accent_theme();
    let initial_width = initial_state
        .as_ref()
        .map_or(1020, |state| state.width.max(420));
    let initial_height = initial_state
        .as_ref()
        .map_or(740, |state| state.height.max(480));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(t(
            "Instalator plasmoid-updater • Bazzite",
            "Plasmoid Updater Installer • Bazzite",
        ))
        .default_width(initial_width)
        .default_height(initial_height)
        .build();

    let overlay = adw::ToastOverlay::new();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let header_title = gtk::Box::new(gtk::Orientation::Vertical, 1);
    header_title.set_halign(gtk::Align::Center);
    header_title.set_valign(gtk::Align::Center);
    let header_main = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    header_main.set_halign(gtk::Align::Center);
    let product_name = gtk::Label::new(Some("Plasmoid Updater"));
    product_name.add_css_class("app-header-title");
    header_main.append(&product_name);
    let version_badge = gtk::Label::new(Some(&format!("v{}", env!("CARGO_PKG_VERSION"))));
    version_badge.add_css_class("app-version-badge");
    header_main.append(&version_badge);
    header_title.append(&header_main);
    let header_subtitle = gtk::Label::new(Some(t(
        "Aktualizacje dodatków · Bazzite KDE",
        "Addon updates · Bazzite KDE",
    )));
    header_subtitle.add_css_class("app-header-subtitle");
    header_subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    header_subtitle.set_max_width_chars(31);
    header_title.append(&header_subtitle);
    header.set_title_widget(Some(&header_title));
    let navigation_toggle = gtk::ToggleButton::new();
    navigation_toggle.set_child(Some(&vector_icon(UiIcon::Menu)));
    navigation_toggle.set_size_request(36, 36);
    navigation_toggle.set_active(true);
    navigation_toggle.set_tooltip_text(Some(t(
        "Pokaż lub ukryj menu nawigacji",
        "Show or hide navigation",
    )));
    header.pack_start(&navigation_toggle);
    let refresh = gtk::Button::new();
    let refresh_icon = vector_icon(UiIcon::Refresh);
    refresh_icon.set_halign(gtk::Align::Center);
    refresh_icon.set_valign(gtk::Align::Center);
    refresh.set_child(Some(&refresh_icon));
    refresh.add_css_class("header-action");
    refresh.set_size_request(36, 36);
    refresh.set_valign(gtk::Align::Center);
    refresh.set_tooltip_text(Some(t(
        "Odśwież stan instalacji",
        "Refresh installation status",
    )));
    header.pack_end(&refresh);
    let language_choice = gtk::DropDown::from_strings(&["PL", "EN"]);
    language_choice.set_height_request(36);
    language_choice.set_selected(Language::selected().index());
    language_choice.set_tooltip_text(Some(t("Język interfejsu", "Interface language")));
    header.pack_end(&language_choice);

    // Long operation messages belong in a dedicated status bar, not the header.
    let cancel_button = gtk::Button::with_label(t("Anuluj", "Cancel"));
    cancel_button.set_tooltip_text(Some(t(
        "Anuluje tylko bezpieczne zadania odczytu i diagnostyki",
        "Cancels read-only scans and diagnostic operations only",
    )));
    cancel_button.set_sensitive(false);
    cancel_button.set_visible(false);
    let spinner = gtk::Spinner::new();
    let activity = gtk::Label::new(Some(t("Gotowe", "Ready")));
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
    status_bar.set_visible(false);
    toolbar.add_bottom_bar(&status_bar);

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    // Explicit navigation rows always show all six destinations. GtkStackSidebar
    // showed only the selected page on some Bazzite KDE configurations.
    let sidebar = gtk::ListBox::new();
    sidebar.set_selection_mode(gtk::SelectionMode::Single);
    sidebar.set_size_request(190, -1);
    sidebar.add_css_class("navigation-sidebar");
    let nav_entries = [
        (t("Pulpit", "Dashboard"), UiIcon::Dashboard),
        (t("Instalacja", "Installation"), UiIcon::Install),
        (t("Dodatki Plasma", "Plasma Addons"), UiIcon::Addons),
        (t("Automatyzacja", "Automation"), UiIcon::Schedule),
        (t("Dziennik", "Log"), UiIcon::Journal),
        (t("O programie", "About"), UiIcon::About),
    ];
    for (label, kind) in nav_entries {
        let item = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        item.set_margin_start(12);
        item.set_margin_end(12);
        item.set_margin_top(10);
        item.set_margin_bottom(10);
        item.append(&vector_icon(kind));
        let text = gtk::Label::new(Some(label));
        text.set_xalign(0.0);
        item.append(&text);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&item));
        sidebar.append(&row);
    }
    let navigation = gtk::Box::new(gtk::Orientation::Vertical, 10);
    navigation.set_margin_top(18);
    let nav_header = gtk::Box::new(gtk::Orientation::Vertical, 4);
    nav_header.set_margin_start(18);
    nav_header.set_margin_end(14);
    nav_header.set_margin_bottom(6);
    let nav_heading_line = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    let nav_icon = vector_icon(UiIcon::Menu);
    nav_icon.add_css_class("dim-label");
    nav_heading_line.append(&nav_icon);
    let navigation_heading = gtk::Label::new(Some(t("Nawigacja", "Navigation")));
    navigation_heading.add_css_class("navigation-heading");
    navigation_heading.set_xalign(0.0);
    navigation_heading.set_hexpand(true);
    nav_heading_line.append(&navigation_heading);
    let nav_count = gtk::Label::new(Some("06"));
    nav_count.add_css_class("app-version-badge");
    nav_heading_line.append(&nav_count);
    nav_header.append(&nav_heading_line);
    let nav_caption = gtk::Label::new(Some(t("SEKCJE APLIKACJI", "APP SECTIONS")));
    nav_caption.add_css_class("navigation-caption");
    nav_caption.set_xalign(0.0);
    nav_header.append(&nav_caption);
    navigation.append(&nav_header);
    let nav_separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    nav_separator.add_css_class("navigation-rule");
    nav_separator.set_margin_start(16);
    nav_separator.set_margin_end(16);
    navigation.append(&nav_separator);
    let nav_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&sidebar)
        .build();
    navigation.append(&nav_scroll);
    // When collapsed, libadwaita overlays the sidebar instead of shrinking the
    // content area by another 190px. It also handles sidebar reveal gestures.
    let split_view = adw::OverlaySplitView::new();
    split_view.set_sidebar(Some(&navigation));
    split_view.set_content(Some(&stack));
    split_view.set_min_sidebar_width(190.0);
    split_view.set_max_sidebar_width(240.0);
    split_view.set_show_sidebar(true);
    let split_for_toggle = split_view.clone();
    navigation_toggle.connect_toggled(move |toggle| {
        split_for_toggle.set_show_sidebar(toggle.is_active());
    });
    let toggle_for_revealer = navigation_toggle.clone();
    split_view.connect_show_sidebar_notify(move |view| {
        if toggle_for_revealer.is_active() != view.shows_sidebar() {
            toggle_for_revealer.set_active(view.shows_sidebar());
        }
    });
    // On narrow screens the sidebar overlays the content. Automatically
    // close it when entering the collapsed layout so controls stay readable.
    split_view.connect_collapsed_notify(move |view| {
        view.set_show_sidebar(!view.is_collapsed());
    });

    let split_for_page = split_view.clone();
    stack.connect_visible_child_name_notify(move |_| {
        if split_for_page.is_collapsed() {
            split_for_page.set_show_sidebar(false);
        }
    });

    let adaptive_layout = adw::BreakpointBin::new();
    adaptive_layout.set_size_request(320, 320);
    adaptive_layout.set_child(Some(&split_view));
    let narrow = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 820px")
            .expect("Prawidłowy warunek szerokości okna"),
    );
    narrow.add_setter(&split_view, "collapsed", Some(&true.to_value()));
    adaptive_layout.add_breakpoint(narrow);
    toolbar.set_content(Some(&adaptive_layout));
    overlay.set_child(Some(&toolbar));
    window.set_content(Some(&overlay));

    let dashboard = page(
        t("Pulpit", "Dashboard"),
        t(
            "Wszystko, czego potrzebujesz do konfiguracji aktualizacji dodatków KDE.",
            "Install, configure and monitor KDE addon updates in one place.",
        ),
    );

    let tiles = gtk::FlowBox::new();
    tiles.set_selection_mode(gtk::SelectionMode::None);
    tiles.set_min_children_per_line(1);
    tiles.set_max_children_per_line(2);
    tiles.set_column_spacing(16);
    tiles.set_row_spacing(16);
    tiles.set_homogeneous(true);
    let (environment_tile, environment_value) =
        status_tile(t("Bazzite KDE", "Bazzite KDE"), UiIcon::Computer);
    let (updater_tile, updater_value) = status_tile("plasmoid-updater", UiIcon::Updater);
    let (agent_tile, agent_value) =
        status_tile(t("Agent aktualizacji", "Update agent"), UiIcon::Agent);
    let (timer_tile, timer_value) = status_tile(t("Harmonogram", "Schedule"), UiIcon::Schedule);
    for tile in [&environment_tile, &updater_tile, &agent_tile, &timer_tile] {
        tiles.insert(tile, -1);
    }
    dashboard.append(&tiles);

    let steps = gtk::Label::new(Some(t(
        "1. Sprawdź środowisko i zainstaluj plasmoid-updater.\n2. Przejrzyj dodatki Plasma i dostępne aktualizacje.\n3. W Automatyzacji ustaw harmonogram i przetestuj konfigurację.",
        "1. Check your system and install plasmoid-updater.\n2. Review installed Plasma addons and available updates.\n3. Set a schedule in Automation and test the configuration.",
    )));
    steps.set_wrap(true);
    steps.set_xalign(0.0);
    steps.set_margin_top(8);
    steps.set_margin_bottom(8);
    let expand_dashboard =
        std::env::var("BAZZITE_GUI_SMOKE_EXPAND_DASHBOARD").as_deref() == Ok("1");
    let first_steps = expander_card(t("Pierwsze kroki", "First steps"), &steps, expand_dashboard);
    dashboard.append(&first_steps);

    let repair_button = button(t("Napraw agenta", "Repair agent"), true);
    let repair_notice = repair_card(
        t("Agent wymaga aktualizacji", "Update agent needs repair"),
        t(
            "Bezpiecznie uaktualnij agenta. Harmonogram i ustawienia pozostaną bez zmian.",
            "Update the agent safely. Your schedule and settings will stay unchanged.",
        ),
        &repair_button,
    );
    repair_notice.set_visible(false);
    dashboard.append(&repair_notice);

    let environment = adw::PreferencesGroup::new();
    environment.set_title(t("Diagnostyka", "Diagnostics"));
    let diagnose_button = button(t("Sprawdź składniki", "Check requirements"), false);
    environment.add(&action_row(
        t("Diagnostyka środowiska", "System diagnostics"),
        t(
            "Bazzite, KDE, curl, Homebrew, bsdtar, kpackagetool6, systemd i instalacja",
            "Check Bazzite, KDE, curl, Homebrew, bsdtar, kpackagetool6 and systemd",
        ),
        &diagnose_button,
    ));
    let status = gtk::Box::new(gtk::Orientation::Vertical, 5);
    render_status_checks(&status, &backend::status_localized(english()));
    status.set_margin_top(8);
    status.set_margin_bottom(8);
    let diagnostics = expander_card(
        t("Szczegóły stanu i ścieżki", "Status details and paths"),
        &status,
        expand_dashboard,
    );
    environment.add(&diagnostics);
    dashboard.append(&environment);
    let footer = gtk::LinkButton::new(backend::UPSTREAM);
    footer.add_css_class("upstream-link");
    let footer_content = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    footer_content.append(&vector_icon(UiIcon::ExternalLink));
    let footer_title = gtk::Label::new(Some(t(
        "Projekt źródłowy plasmoid-updater",
        "Upstream plasmoid-updater project",
    )));
    footer_title.set_wrap(true);
    footer_content.append(&footer_title);
    footer.set_child(Some(&footer_content));
    footer.set_tooltip_text(Some(backend::UPSTREAM));
    footer.set_halign(gtk::Align::Start);
    footer.set_margin_top(10);
    dashboard.append(&footer);
    add_page(&stack, "dashboard", t("Pulpit", "Dashboard"), &dashboard);

    let installation = page(
        t("Instalacja", "Installation"),
        t(
            "Instalacja z GitHub i ręczna obsługa aktualizacji przez oryginalny plasmoid-updater.",
            "Install from GitHub and manage addon updates using the original plasmoid-updater.",
        ),
    );

    let install_group = adw::PreferencesGroup::new();
    install_group.set_title(t("Instalacja i aktualizacje", "Installation and updates"));
    let install_button = button(t("Zainstaluj / uaktualnij", "Install / upgrade"), true);
    install_group.add(&action_row(
        t("Instaluj plasmoid-updater", "Install plasmoid-updater"),
        t(
            "Wykrywa istniejącą wersję; pobiera tylko nowszą i weryfikuje SHA-256",
            "Detects an existing version, downloads only newer releases and verifies SHA-256",
        ),
        &install_button,
    ));
    let check_button = button(t("Sprawdź", "Check"), false);
    install_group.add(&action_row(
        t("Dostępne aktualizacje", "Available updates"),
        t(
            "Sprawdza wersje dodatków bez dokonywania zmian",
            "Checks addon versions without making changes",
        ),
        &check_button,
    ));
    let dry_run_button = button(t("Symuluj", "Simulate"), false);
    install_group.add(&action_row(
        t("Symulacja aktualizacji", "Update simulation"),
        t(
            "Wyświetla plan bez instalacji",
            "Previews planned updates without installing",
        ),
        &dry_run_button,
    ));
    let update_button = button(t("Aktualizuj wszystko teraz", "Update now"), false);
    install_group.add(&action_row(
        t("Ręczna aktualizacja", "Manual update"),
        t(
            "Sprawdza GitHub (opcjonalnie), następnie aktualizuje dodatki KDE Store",
            "Optionally checks GitHub, then installs available KDE Store addon updates",
        ),
        &update_button,
    ));
    installation.append(&install_group);
    add_page(
        &stack,
        "installation",
        t("Instalacja", "Installation"),
        &installation,
    );

    let addons_page = page(
        t("Dodatki Plasma", "Plasma Addons"),
        t("Komponenty wykrywane przez plasmoid-updater. Dodatki systemowe są wyświetlane tylko do odczytu.",
          "Components detected by plasmoid-updater. System-wide addons are read-only."),
    );
    let addons_group = adw::PreferencesGroup::new();
    addons_group.set_title(t(
        "Zainstalowane dodatki KDE Store",
        "Installed KDE Store addons",
    ));
    let scan_button = button(t("Skanuj dodatki", "Scan addons"), true);
    addons_group.add(&action_row(
        t(
            "Odczytaj dodatki użytkownika i systemowe",
            "Scan user and system addons",
        ),
        t(
            "Skanowanie niczego nie instaluje, nie usuwa ani nie modyfikuje",
            "Scanning never installs, removes or modifies addons",
        ),
        &scan_button,
    ));
    let inventory_filters = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let inventory_search = gtk::SearchEntry::new();
    inventory_search.set_placeholder_text(Some(t(
        "Szukaj nazwy lub rodzaju dodatku",
        "Search by addon name or type",
    )));
    inventory_search.set_hexpand(true);
    let inventory_scope = gtk::DropDown::from_strings(&[
        t("Wszystkie", "All"),
        t("Użytkownika", "User"),
        t("Systemowe", "System"),
    ]);
    inventory_scope.set_halign(gtk::Align::Start);
    inventory_filters.append(&inventory_search);
    inventory_filters.append(&inventory_scope);
    addons_group.add(&inventory_filters);
    let inventory_count = gtk::Label::new(Some(t(
        "Nie skanowano jeszcze dodatków.",
        "Addons have not been scanned yet.",
    )));
    inventory_count.set_xalign(0.0);
    inventory_count.set_wrap(true);
    inventory_count.add_css_class("dim-label");
    addons_group.add(&inventory_count);
    let inventory_list = gtk::ListBox::new();
    inventory_list.set_selection_mode(gtk::SelectionMode::None);
    inventory_list.add_css_class("boxed-list");
    empty_list_message(
        &inventory_list,
        t(
            "Kliknij „Skanuj dodatki”, aby wyświetlić zainstalowane komponenty.",
            "Click Scan addons to view installed components.",
        ),
    );
    addons_group.add(&inventory_list);
    addons_page.append(&addons_group);

    let updates_group = adw::PreferencesGroup::new();
    updates_group.set_title(t("Dostępne aktualizacje", "Available updates"));
    let scan_updates_button = button(t("Sprawdź wersje", "Check versions"), false);
    updates_group.add(&action_row(
        t("Sprawdź aktualizacje KDE Store", "Check KDE Store updates"),
        t(
            "Odczytuje dostępne wersje przez plasmoid-updater check --json",
            "Reads available versions using plasmoid-updater check --json",
        ),
        &scan_updates_button,
    ));
    let updates_count = gtk::Label::new(Some(t(
        "Nie sprawdzano jeszcze aktualizacji.",
        "Updates have not been checked yet.",
    )));
    updates_count.set_xalign(0.0);
    updates_count.set_wrap(true);
    updates_count.add_css_class("dim-label");
    updates_group.add(&updates_count);
    let updates_list = gtk::ListBox::new();
    updates_list.set_selection_mode(gtk::SelectionMode::None);
    updates_list.add_css_class("boxed-list");
    empty_list_message(
        &updates_list,
        t(
            "Kliknij „Sprawdź wersje”, aby zobaczyć dostępne aktualizacje.",
            "Click Check versions to see available updates.",
        ),
    );
    updates_group.add(&updates_list);
    addons_page.append(&updates_group);
    add_page(
        &stack,
        "addons",
        t("Dodatki Plasma", "Plasma Addons"),
        &addons_page,
    );

    let automation_page = page(
        t("Automatyzacja", "Automation"),
        t(
            "Aktualizowanie dodatków oraz opcjonalnie samego programu w tle przez systemd --user.",
            "Update addons and optionally plasmoid-updater automatically with systemd --user.",
        ),
    );
    let automation_group = adw::PreferencesGroup::new();
    automation_group.set_title(t("Stan harmonogramu", "Schedule status"));
    let automation = gtk::Label::new(Some(&backend::automation_report_localized(english())));
    automation.set_xalign(0.0);
    automation.set_wrap(true);
    automation.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    automation.set_selectable(true);
    automation.set_margin_top(10);
    automation.set_margin_bottom(10);
    automation.set_margin_start(16);
    automation.set_margin_end(16);
    let automation_card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    automation_card.add_css_class("card");
    automation_card.append(&automation);
    automation_group.add(&automation_card);
    automation_page.append(&automation_group);

    let settings_group = adw::PreferencesGroup::new();
    settings_group.set_title(t("Harmonogram aktualizacji", "Update schedule"));
    let behavior_group = adw::PreferencesGroup::new();
    behavior_group.set_title(t("Dodatki i powiadomienia", "Addons and notifications"));
    let migration_group = adw::PreferencesGroup::new();
    let apply_group = adw::PreferencesGroup::new();
    apply_group.set_title(t("Testowanie", "Testing"));
    let initial = match backend::home().and_then(|h| backend::initial_settings(&h)) {
        Ok(settings) => settings,
        Err(error) => {
            render_status_checks(
                &status,
                &format!(
                    "{}\n{}: {error:#}",
                    backend::status_localized(english()),
                    t("Ustawienia", "Settings")
                ),
            );
            Settings::default()
        }
    };
    let automatic = switch_row(
        t(
            "Automatyczne aktualizacje w tle",
            "Automatic background updates",
        ),
        t(
            "Zadanie systemd uruchamia się według harmonogramu",
            "The systemd timer runs the update task on schedule",
        ),
        initial.automatic,
    );
    settings_group.add(&automatic);

    let auto_update_tool = switch_row(
        t(
            "Aktualizuj również plasmoid-updater",
            "Also update plasmoid-updater",
        ),
        t(
            "Każdy cykl sprawdza wydanie na GitHub i pobiera program tylko, gdy jest nowszy",
            "Every cycle checks GitHub and downloads the tool only when a newer version exists",
        ),
        initial.auto_update_tool,
    );
    settings_group.add(&auto_update_tool);

    let frequency = gtk::DropDown::from_strings(&[
        t("Co 6 godzin", "Every 6 hours"),
        t("Co 12 godzin", "Every 12 hours"),
        t("Raz dziennie", "Daily"),
    ]);
    frequency.set_halign(gtk::Align::Start);
    frequency.set_selected(match initial.frequency_hours {
        6 => 0,
        12 => 1,
        _ => 2,
    });
    let frequency_row = info_card(
        t("Harmonogram", "Schedule"),
        t(
            "Pominięty termin jest nadrabiany po zalogowaniu; opóźnienie do 20 minut",
            "Missed runs are caught up after login, with up to 20 minutes of randomized delay",
        ),
    );
    frequency_row.set_margin_top(12);
    frequency.set_margin_start(16);
    frequency.set_margin_end(16);
    frequency.set_margin_bottom(10);
    frequency_row.append(&frequency);
    settings_group.add(&frequency_row);

    let restart = switch_row(
        t(
            "Restart Plasma po automatycznej aktualizacji",
            "Restart Plasma after automatic updates",
        ),
        t(
            "Domyślnie wyłączony — restart mógłby przerwać sesję KDE",
            "Off by default — restarting Plasma could interrupt your KDE session",
        ),
        initial.restart_plasma,
    );
    migration_group.add(&restart);

    let notify_success = switch_row(
        t(
            "Powiadamiaj po zakończonym cyklu",
            "Notify after successful updates",
        ),
        t(
            "Wymaga dostępnego notify-send i sesji powiadomień KDE",
            "Requires notify-send and an active KDE notification session",
        ),
        initial.notify_on_success,
    );
    behavior_group.add(&notify_success);
    let notify_error = switch_row(
        t(
            "Powiadamiaj o błędach aktualizacji",
            "Notify on update errors",
        ),
        t(
            "Domyślnie włączone; brak usługi powiadomień nie zatrzymuje aktualizacji",
            "Enabled by default; missing notifications never block updates",
        ),
        initial.notify_on_error,
    );
    behavior_group.add(&notify_error);

    let migrate_legacy = switch_row(
        t(
            "Przejmij istniejącą usługę i harmonogram plasmoid-updater",
            "Take over existing plasmoid-updater systemd units",
        ),
        t(
            "Zapisze kopie starych jednostek, wyłączy poprzedni timer i skonfiguruje nowy",
            "Backs up old units, disables the previous timer and configures the new one",
        ),
        false,
    );
    migration_group.add(&migrate_legacy);

    let exclusions = gtk::Entry::new();
    exclusions.set_text(&initial.excluded_packages.join(", "));
    exclusions.set_placeholder_text(Some(t(
        "np. moja-wtyczka, drugi-motyw",
        "e.g. my-widget, second-theme",
    )));
    exclusions.set_hexpand(true);
    exclusions.set_margin_start(16);
    exclusions.set_margin_end(16);
    exclusions.set_margin_bottom(10);
    let exclusions_card = info_card(
        t("Wykluczone dodatki", "Excluded addons"),
        t(
            "Nazwy komponentów, których nie należy aktualizować. Oddziel je przecinkami.",
            "Components to skip during updates. Separate names with commas.",
        ),
    );
    exclusions_card.set_margin_top(12);
    exclusions_card.append(&exclusions);
    behavior_group.add(&exclusions_card);
    let unsaved = gtk::Label::new(Some(t("Niezapisane zmiany", "Unsaved changes")));
    unsaved.add_css_class("warning");
    unsaved.set_xalign(0.0);
    unsaved.set_wrap(true);
    unsaved.set_visible(false);
    let save_button = button(t("Zapisz i zastosuj", "Save and apply"), true);
    // The save action must never disappear among scrolled preference widgets.
    // Keep it in a persistent footer, visible only on the Automation page.
    let save_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    save_bar.add_css_class("toolbar");
    save_bar.set_margin_start(14);
    save_bar.set_margin_end(14);
    save_bar.set_margin_top(8);
    save_bar.set_margin_bottom(8);
    let save_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    save_spacer.set_hexpand(true);
    save_bar.append(&unsaved);
    save_bar.append(&save_spacer);
    save_bar.append(&save_button);
    toolbar.add_bottom_bar(&save_bar);
    save_bar.set_visible(stack.visible_child_name().as_deref() == Some("automation"));
    let save_bar_for_navigation = save_bar.clone();
    stack.connect_visible_child_name_notify(move |stack| {
        save_bar_for_navigation
            .set_visible(stack.visible_child_name().as_deref() == Some("automation"));
    });

    let test_button = button(t("Przetestuj", "Test"), false);
    let tests = gtk::Box::new(gtk::Orientation::Vertical, 10);
    tests.append(&test_action_card(
        t("Test końcowy konfiguracji", "Configuration check"),
        t(
            "Sprawdza TOML, narzędzia, agenta, systemd oraz dry-run bez zmian w dodatkach",
            "Checks TOML, dependencies, agent, systemd and a dry run without changing addons",
        ),
        &test_button,
    ));
    let run_service_button = button(t("Uruchom teraz", "Run now"), false);
    tests.append(&test_action_card(
        t(
            "Przetestuj pełny cykl aktualizacji",
            "Run a full update cycle",
        ),
        t(
            "Uruchamia usługę systemd i instaluje dostępne aktualizacje (wykonuje zmiany)",
            "Starts the systemd service and installs available updates (makes changes)",
        ),
        &run_service_button,
    ));
    let tests_expander = expander_card(
        t(
            "Testowanie i uruchamianie ręczne",
            "Testing and manual execution",
        ),
        &tests,
        std::env::var("BAZZITE_GUI_SMOKE_EXPAND_TESTS").as_deref() == Ok("1"),
    );
    apply_group.add(&tests_expander);
    let advanced_expander = expander_card(
        t("Ustawienia zaawansowane", "Advanced settings"),
        &migration_group,
        std::env::var("BAZZITE_GUI_SMOKE_EXPAND_ADVANCED").as_deref() == Ok("1"),
    );
    automation_page.append(&settings_group);
    automation_page.append(&behavior_group);
    automation_page.append(&advanced_expander);
    automation_page.append(&apply_group);
    add_page(
        &stack,
        "automation",
        t("Automatyzacja", "Automation"),
        &automation_page,
    );

    let journal = page(
        t("Dziennik", "Log"),
        t(
            "Wyniki operacji instalatora oraz historia usługi plasmoid-updater.service.",
            "Installer output and plasmoid-updater.service history.",
        ),
    );
    let log_group = adw::PreferencesGroup::new();
    log_group.set_title(t("Dziennik operacji", "Operation log"));
    let journal_button = button(t("Wczytaj historię systemd", "Load systemd history"), false);
    log_group.add(&action_row(
        t("Ostatnie 100 wpisów usługi", "Last 100 service entries"),
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
    output.set_text(t("Gotowe. Sprawdź składniki, zainstaluj program i zapisz harmonogram. Na końcu wykonaj test.",
        "Ready. Check requirements, install the tool and save a schedule. Finally, run a test."));
    let log_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(420)
        .vexpand(true)
        .child(&log_view)
        .build();
    log_scroll.add_css_class("card");
    log_group.add(&log_scroll);
    journal.append(&log_group);
    add_page(&stack, "journal", t("Dziennik", "Log"), &journal);

    let about = page(
        t("O programie", "About"),
        t(
            "Informacje o aplikacji, jej autorze, wykorzystanym narzędziu i licencji.",
            "Application information, its author, the underlying tool and license.",
        ),
    );
    let identity = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    identity.add_css_class("card");
    identity.set_margin_bottom(8);
    identity.set_margin_top(4);
    let logo = gtk::Image::from_icon_name("io.github.wolfgangxxiii.BazzitePlasmoidUpdater");
    logo.add_css_class("about-logo");
    logo.set_pixel_size(72);
    logo.set_valign(gtk::Align::Start);
    logo.set_margin_start(14);
    logo.set_margin_top(14);
    logo.set_margin_bottom(14);
    identity.append(&logo);
    let identity_text = gtk::Box::new(gtk::Orientation::Vertical, 6);
    identity_text.set_hexpand(true);
    identity_text.set_margin_top(14);
    identity_text.set_margin_end(14);
    identity_text.set_margin_bottom(14);
    let name = gtk::Label::new(Some("Bazzite Plasmoid Updater GUI"));
    name.add_css_class("about-product-name");
    name.set_wrap(true);
    name.set_xalign(0.0);
    identity_text.append(&name);
    let version = gtk::Label::new(Some(&format!("v{}", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("dim-label");
    version.set_xalign(0.0);
    identity_text.append(&version);
    let description = gtk::Label::new(Some(t(
        "Graficzna instalacja i zarządzanie aktualizacjami dodatków KDE Plasma na Bazzite. Harmonogram działa w tle bez uruchomionego okna programu.",
        "Graphical installation and KDE Plasma addon updates on Bazzite. Scheduled updates run in the background without the application window.",
    )));
    description.set_xalign(0.0);
    description.set_wrap(true);
    identity_text.append(&description);
    identity.append(&identity_text);
    about.append(&identity);

    let project = about_section(t("Strona internetowa", "Website"));
    project.append(&about_link(
        env!("CARGO_PKG_REPOSITORY"),
        t(
            "Repozytorium projektu na GitHub",
            "Project repository on GitHub",
        ),
    ));
    project.append(&about_link(
        "https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/releases",
        t(
            "Wydania i pliki AppImage",
            "Releases and AppImage downloads",
        ),
    ));
    about.append(&project);

    about.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let help = about_section(t("Uzyskaj pomoc", "Get help"));
    help.append(&about_link(
        "https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/issues",
        t(
            "Zgłoś błąd lub zaproponuj funkcję",
            "Report an issue or request a feature",
        ),
    ));
    about.append(&help);

    about.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let copyright = about_section(t("Prawa autorskie", "Copyright"));
    let license = gtk::Label::new(Some(t(
        "Licencja: GNU GPL-3.0-or-later",
        "License: GNU GPL-3.0-or-later",
    )));
    license.set_xalign(0.0);
    license.set_wrap(true);
    copyright.append(&license);
    copyright.append(&about_link(
        "https://github.com/wolfgangxxiii/bazzite-kde-installer-plasmoid-updater-gui/blob/main/LICENSE",
        t("Przeczytaj licencję", "Read the license"),
    ));
    about.append(&copyright);

    about.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let authors = about_section(t("Autor programu", "Application author"));
    authors.append(&about_link(
        "https://github.com/wolfgangxxiii",
        t(
            "wolfgangxxiii · autor aplikacji",
            "wolfgangxxiii · application author",
        ),
    ));
    about.append(&authors);

    about.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let upstream = about_section(t("Wykorzystane narzędzie", "Underlying tool"));
    let upstream_note = gtk::Label::new(Some(t(
        "Aplikacja wykorzystuje niezależny projekt plasmoid-updater do aktualizacji dodatków KDE.",
        "This application uses the independent plasmoid-updater project to update KDE addons.",
    )));
    upstream_note.set_xalign(0.0);
    upstream_note.set_wrap(true);
    upstream.append(&upstream_note);
    upstream.append(&about_link(
        backend::UPSTREAM,
        t(
            "uwuclxdy · autor narzędzia plasmoid-updater",
            "uwuclxdy · author of the plasmoid-updater tool",
        ),
    ));
    let disclaimer = gtk::Label::new(Some(t(
        "Projekt niezależny, niepowiązany oficjalnie z Bazzite ani KDE.",
        "Independent project, not officially affiliated with Bazzite or KDE.",
    )));
    disclaimer.add_css_class("dim-label");
    disclaimer.set_wrap(true);
    disclaimer.set_xalign(0.0);
    disclaimer.set_margin_top(8);
    upstream.append(&disclaimer);
    about.append(&upstream);
    add_page(&stack, "about", t("O programie", "About"), &about);

    const PAGE_KEYS: [&str; 6] = [
        "dashboard",
        "installation",
        "addons",
        "automation",
        "journal",
        "about",
    ];
    let stack_for_navigation = stack.clone();
    sidebar.connect_row_selected(move |_, row| {
        if let Some(index) = row.map(|row| row.index()) {
            if let Some(key) = PAGE_KEYS.get(index as usize) {
                stack_for_navigation.set_visible_child_name(key);
            }
        }
    });
    let selected_index = PAGE_KEYS
        .iter()
        .position(|name| *name == initial_page)
        .unwrap_or(0);
    if let Some(row) = sidebar.row_at_index(selected_index as i32) {
        sidebar.select_row(Some(&row));
    }

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
        status_bar,
        spinner,
        cancel_button: cancel_button.clone(),
        cancel_token: Rc::new(RefCell::new(None)),
        dirty,
        unsaved,
        buttons: Rc::new(vec![
            repair_button.clone(),
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
            language_choice.clone().upcast(),
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
        status_generation: Rc::new(Cell::new(0)),
        inventory: Rc::new(RefCell::new(Vec::new())),
        partial_system: Rc::new(Cell::new(false)),
        updates: Rc::new(RefCell::new(Vec::new())),
        update_overview: Rc::new(RefCell::new(None)),
        inventory_list,
        updates_list,
        inventory_count,
        updates_count,
        inventory_search: inventory_search.clone(),
        inventory_scope: inventory_scope.clone(),
        environment_value,
        updater_value,
        agent_value,
        timer_value,
        repair_button: repair_button.clone(),
        repair_notice,
    };
    if let Some(saved) = initial_state {
        *parts.inventory.borrow_mut() = saved.inventory;
        parts.partial_system.set(saved.partial_system);
        parts.inventory_search.set_text(&saved.search);
        parts.inventory_scope.set_selected(saved.scope);
        render_inventory(&parts);
        if let Some(overview) = saved.updates {
            render_updates(&parts, overview);
        }
        parts.output.set_text(&saved.log);
        if saved.maximized {
            window.maximize();
        }
    }
    refresh_status(&parts);
    let refresh_parts = parts.clone();
    refresh.connect_clicked(move |_| {
        apply_host_accent_theme();
        refresh_status(&refresh_parts);
    });
    let parts_language = parts.clone();
    let app_language = app.clone();
    let window_language = window.clone();
    let stack_language = stack.clone();
    language_choice.connect_selected_notify(move |choice| {
        let requested = Language::from_index(choice.selected());
        if requested == Language::selected() {
            return;
        }
        if parts_language.busy.get() || parts_language.dirty.get() {
            parts_language.toast.add_toast(adw::Toast::new(t(
                "Zakończ operację i zapisz zmiany przed zmianą języka",
                "Finish the operation and save your changes before switching language",
            )));
            choice.set_selected(Language::selected().index());
            return;
        }
        if let Err(error) = requested.save() {
            parts_language.toast.add_toast(adw::Toast::new(&format!(
                "{}: {error:#}",
                t("Nie udało się zapisać języka", "Could not save language")
            )));
            choice.set_selected(Language::selected().index());
            return;
        }
        let page = stack_language
            .visible_child_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "dashboard".into());
        let log = parts_language.output.text(
            &parts_language.output.start_iter(),
            &parts_language.output.end_iter(),
            true,
        );
        let state = ViewState {
            width: window_language.width(),
            height: window_language.height(),
            maximized: window_language.is_maximized(),
            inventory: parts_language.inventory.borrow().clone(),
            partial_system: parts_language.partial_system.get(),
            updates: parts_language.update_overview.borrow().clone(),
            search: parts_language.inventory_search.text().to_string(),
            scope: parts_language.inventory_scope.selected(),
            log: log.to_string(),
        };
        build_with_page(&app_language, &page, Some(state));
        window_language.close();
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
            cancel_parts
                .activity
                .set_text(t("Anulowanie zadania…", "Cancelling operation…"));
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

    let parts_repair = parts.clone();
    repair_button.connect_clicked(move |_| {
        launch(
            parts_repair.clone(),
            Task::Operation(Job::RepairAgent),
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
            parts_update.toast.add_toast(adw::Toast::new(t(
                "Najpierw zapisz zmienione ustawienia automatyzacji i wykluczeń",
                "Save your changed automation settings and exclusions first",
            )));
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
            parts_service.toast.add_toast(adw::Toast::new(t(
                "Zapisz ustawienia przed uruchomieniem cyklu",
                "Save settings before running the update cycle",
            )));
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
