mod backend;
mod i18n;
mod settings;
mod ui;

use adw::prelude::*;

fn main() {
    i18n::Language::initialize();
    let app = adw::Application::builder()
        .application_id("io.github.wolfgangxxiii.BazzitePlasmoidUpdater")
        .build();
    app.connect_activate(ui::build);
    app.run();
}
