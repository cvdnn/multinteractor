//! `Alt+V` 智能粘贴路由器：编辑器光标注入 vs 资源管理器当前目录直写 vs 原生【另存为】对话框兜底 (FR-PASTE-01~03).

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use mi_protocol::envelope::{CloudEventEnvelope, ContentCategory};
use crate::platform::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartPasteDecision {
    /// 焦点在编辑器/输入框 (Word / TXT / IDE)，校验 BLAKE3 摘要后直接将文本粘贴至光标处 (TC-CLIP-01).
    PasteTextIntoEditor { text: String, content_hash: String },
    /// 焦点在文件资源管理器 (Explorer / Finder)，直接将文件/文件夹落盘至当前打开目录 (TC-CLIP-02, TC-CLIP-04).
    DirectSaveToFolder {
        event_id: String,
        target_dir: PathBuf,
        item_name: String,
    },
    /// 焦点不在文件资源管理器且待粘贴内容为文件/文件夹，自动弹出系统原生【另存为 / 选择保存目录】对话框 (TC-CLIP-03).
    ShowNativeSaveAsDialog {
        event_id: String,
        default_item_name: String,
        chosen_dir: Option<PathBuf>,
    },
}

pub struct SmartPasteRouter<P: OsPlatformAdapter> {
    platform: P,
}

impl<P: OsPlatformAdapter> SmartPasteRouter<P> {
    pub fn new(platform: P) -> Self {
        Self { platform }
    }

    pub fn route_alt_v(&self, latest_event: &CloudEventEnvelope) -> anyhow::Result<SmartPasteDecision> {
        let focus = self.platform.inspect_focused_window()?;
        match latest_event.payload.content_category {
            ContentCategory::Text | ContentCategory::Image => {
                let text = latest_event
                    .payload
                    .inline_data
                    .clone()
                    .unwrap_or_default();
                // 粘贴前强制校验全文 BLAKE3 摘要
                latest_event.payload.digest.verify_bytes(text.as_bytes())?;
                Ok(SmartPasteDecision::PasteTextIntoEditor {
                    text,
                    content_hash: latest_event.payload.digest.full_hash.clone(),
                })
            }
            ContentCategory::File | ContentCategory::Directory => {
                let default_name = latest_event
                    .payload
                    .items_meta
                    .first()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "downloaded_item".to_string());

                match focus {
                    FocusedWindowContext::FileManager { current_dir, .. } => {
                        Ok(SmartPasteDecision::DirectSaveToFolder {
                            event_id: latest_event.event_id.clone(),
                            target_dir: current_dir,
                            item_name: default_name,
                        })
                    }
                    FocusedWindowContext::TextEditor { .. } | FocusedWindowContext::OtherWindow { .. } => {
                        let chosen_dir = self.platform.prompt_native_save_dialog(&default_name)?;
                        Ok(SmartPasteDecision::ShowNativeSaveAsDialog {
                            event_id: latest_event.event_id.clone(),
                            default_item_name: default_name,
                            chosen_dir,
                        })
                    }
                }
            }
        }
    }
}
