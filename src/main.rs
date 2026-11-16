mod backend;
mod settings;
mod ui;

use adw::prelude::*;

fn main() {
    let app = adw::Application::builder()
        .application_id("io.github.wolfgangxxiii.BazzitePlasmoidUpdater")
        .build();
    app.connect_activate(ui::build);
    app.run();
}
