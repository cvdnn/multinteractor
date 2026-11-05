//! `Alt+C` 上下文抓取器：自动提取当前活动窗口选中文本、图片或 Explorer/Finder 选中文件/文件夹路径 (FR-COPY-01).

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::platform::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapturedSelection {
    Text(String),
    ImageBytes(Vec<u8>),
    FileSystemPaths(Vec<PathBuf>),
}

pub struct ContextCaptureEngine<P: OsPlatformAdapter> {
    platform: P,
}

impl<P: OsPlatformAdapter> ContextCaptureEngine<P> {
    pub fn new(platform: P) -> Self {
        Self { platform }
    }

    /// 当用户按下 `Alt+C` 时抓取当前焦点窗口选中项，且不污染系统默认 `Ctrl+V` 剪贴板 (TC-CLIP-01).
    pub fn capture_on_alt_c(&self) -> anyhow::Result<CapturedSelection> {
        let ctx = self.platform.inspect_focused_window()?;
        match ctx {
            FocusedWindowContext::FileManager { selected_items, .. } if !selected_items.is_empty() => {
                Ok(CapturedSelection::FileSystemPaths(selected_items))
            }
            _ => {
                if let Some(selected_text) = self.platform.read_selected_text_without_clobber()? {
                    if !selected_text.is_empty() {
                        return Ok(CapturedSelection::Text(selected_text));
                    }
                }
                anyhow::bail!("No active text or file selection detected on Alt+C")
            }
        }
    }
}
