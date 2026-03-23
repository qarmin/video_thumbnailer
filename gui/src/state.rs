use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use slint::{StandardListViewItem, VecModel};

/// Resources shared by all the wiring modules.
pub struct AppCtx {
    /// Authoritative list of selected source paths.
    pub files: Rc<RefCell<Vec<PathBuf>>>,
    /// Visual model mirroring `files` (display strings only).
    pub file_model: Rc<VecModel<StandardListViewItem>>,
    /// Cooperative stop flag for background processing.
    pub stop_flag: Arc<AtomicBool>,
}

impl AppCtx {
    pub fn new() -> Self {
        Self {
            files: Rc::new(RefCell::new(Vec::new())),
            file_model: Rc::new(VecModel::default()),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Append a path to both the store and the visual model (deduped).
    pub fn add_path(&self, path: PathBuf) {
        if self.files.borrow().contains(&path) {
            return;
        }
        let mut item = StandardListViewItem::default();
        item.text = path.to_string_lossy().to_string().into();
        self.file_model.push(item);
        self.files.borrow_mut().push(path);
    }

    pub fn clear(&self) {
        self.files.borrow_mut().clear();
        self.file_model.set_vec(Vec::new());
    }

    pub fn remove_at(&self, idx: i32) {
        if idx < 0 {
            return;
        }
        let i = idx as usize;
        let mut files = self.files.borrow_mut();
        if i < files.len() {
            files.remove(i);
            self.file_model.remove(i);
        }
    }
}
