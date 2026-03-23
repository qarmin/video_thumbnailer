slint::include_modules!();

mod config;
mod settings;
mod state;
mod util;
mod wire;

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc};
use thumbnailer_core::check_ffmpeg;

use crate::state::AppCtx;

fn main() {
    let window = AppWindow::new().expect("Failed to create window");

    settings::apply(&window, &settings::load());
    // Re-sync the std-widgets palette now that Settings.dark-theme reflects the
    // persisted value (the Slint `init` handler ran with defaults).
    window.invoke_sync_theme();

    if !check_ffmpeg() {
        window
            .global::<GuiState>()
            .set_status_text("WARNING: ffmpeg / ffprobe not found in PATH. Processing will fail.".into());
    }

    let ctx = Rc::new(AppCtx::new());
    window.set_file_items(ModelRc::from(ctx.file_model.clone()));

    wire::files::load_cli_args(&ctx);
    wire::files::wire(&window, ctx.clone());
    wire::misc::wire(&window);
    wire::processing::wire(&window, ctx);

    window.run().expect("Window run failed");

    settings::save(&window);
}
