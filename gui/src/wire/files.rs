use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::AppCtx;
use crate::util::{collect_from_dir, is_video};
use crate::{AppWindow, Callabler};

pub fn wire(win: &AppWindow, ctx: Rc<AppCtx>) {
    let cb = win.global::<Callabler>();

    cb.on_add_files({
        let ctx = ctx.clone();
        move || {
            let Some(paths) = rfd::FileDialog::new()
                .add_filter("Videos", crate::util::VIDEO_EXTS)
                .add_filter("All files", &["*"])
                .pick_files()
            else {
                return;
            };
            for p in paths {
                ctx.add_path(p);
            }
        }
    });

    cb.on_add_folder({
        let ctx = ctx.clone();
        move || {
            let Some(dir) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            for p in collect_from_dir(&dir) {
                ctx.add_path(p);
            }
        }
    });

    cb.on_clear_files({
        let ctx = ctx.clone();
        move || ctx.clear()
    });

    cb.on_remove_selected({
        let ctx = ctx.clone();
        move |idx| ctx.remove_at(idx)
    });
}

/// Pick up files / folders passed on the command line.
pub fn load_cli_args(ctx: &AppCtx) {
    use std::path::PathBuf;
    for arg in std::env::args().skip(1) {
        let path = PathBuf::from(&arg);
        if path.is_file() && is_video(&path) {
            ctx.add_path(path);
        } else if path.is_dir() {
            for p in collect_from_dir(&path) {
                ctx.add_path(p);
            }
        }
    }
}
