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
                if let Some(img_bytes) = self.platform.read_clipboard_image_bytes()? {
                    if !img_bytes.is_empty() {
                        return Ok(CapturedSelection::ImageBytes(img_bytes));
                    }
                }
                anyhow::bail!("No active text, image, or file selection detected on Alt+C")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::macos::MacOsPlatformAdapter;

    #[test]
    fn test_capture_text_image_and_filesystem_paths() {
        // 1. FileManager selected items take priority
        let fm_adapter = MacOsPlatformAdapter {
            simulated_focus: Some(FocusedWindowContext::FileManager {
                app_name: "Finder".to_string(),
                current_dir: PathBuf::from("/Users/test/Desktop"),
                selected_items: vec![PathBuf::from("/Users/test/Desktop/课程资料")],
            }),
            simulated_selected_text: Some("ignored".to_string()),
            ..Default::default()
        };
        let engine = ContextCaptureEngine::new(fm_adapter);
        assert_eq!(
            engine.capture_on_alt_c().unwrap(),
            CapturedSelection::FileSystemPaths(vec![PathBuf::from("/Users/test/Desktop/课程资料")])
        );

        // 2. TextEditor captures selected text without clobbering clipboard
        let txt_adapter = MacOsPlatformAdapter {
            simulated_focus: Some(FocusedWindowContext::TextEditor {
                app_name: "VSCode".to_string(),
            }),
            simulated_selected_text: Some("fn main() {}".to_string()),
            ..Default::default()
        };
        let engine_txt = ContextCaptureEngine::new(txt_adapter);
        assert_eq!(
            engine_txt.capture_on_alt_c().unwrap(),
            CapturedSelection::Text("fn main() {}".to_string())
        );

        // 3. ImageBytes fallback when no text is selected
        let img_adapter = MacOsPlatformAdapter {
            simulated_focus: Some(FocusedWindowContext::OtherWindow {
                app_name: "Preview".to_string(),
            }),
            simulated_image_bytes: Some(vec![0x89, b'P', b'N', b'G']),
            ..Default::default()
        };
        let engine_img = ContextCaptureEngine::new(img_adapter);
        assert_eq!(
            engine_img.capture_on_alt_c().unwrap(),
            CapturedSelection::ImageBytes(vec![0x89, b'P', b'N', b'G'])
        );
    }
}

