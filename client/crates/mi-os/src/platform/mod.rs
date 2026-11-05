//! 操作系统平台特定 API 抽象与条件编译适配层 (`windows` / `macos`).

pub mod macos;
pub mod windows;

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusedWindowContext {
    TextEditor {
        app_name: String,
    },
    FileManager {
        app_name: String,
        current_dir: PathBuf,
        selected_items: Vec<PathBuf>,
    },
    OtherWindow {
        app_name: String,
    },
}

pub trait OsPlatformAdapter: Send + Sync {
    fn inspect_focused_window(&self) -> anyhow::Result<FocusedWindowContext>;
    fn read_selected_text_without_clobber(&self) -> anyhow::Result<Option<String>>;
    fn prompt_native_save_dialog(&self, default_file_name: &str) -> anyhow::Result<Option<PathBuf>>;
}
