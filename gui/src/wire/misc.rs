use slint::ComponentHandle;

use crate::util::open_url;
use crate::{AppWindow, Callabler, Settings};

pub const REPOSITORY_URL: &str = "https://github.com/qarmin/video_thumbnailer";

pub fn wire(win: &AppWindow) {
    let cb = win.global::<Callabler>();

    cb.on_select_output_dir({
        let weak = win.as_weak();
        move || {
            let Some(dir) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            let win = weak.upgrade().expect("AppWindow destroyed during callback");
            win.global::<Settings>()
                .set_output_dir(dir.to_string_lossy().to_string().into());
        }
    });

    cb.on_open_repository(|| open_url(REPOSITORY_URL));

    // Slint flips Settings.dark-theme; we ask the window to re-sync std-widgets
    // palette so it doesn't drift from our own Theme global.
    cb.on_toggle_theme({
        let weak = win.as_weak();
        move || {
            let win = weak.upgrade().expect("AppWindow destroyed during toggle-theme");
            win.invoke_sync_theme();
        }
    });
}
