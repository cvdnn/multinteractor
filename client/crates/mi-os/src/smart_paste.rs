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
            ContentCategory::Text => {
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
            ContentCategory::Image if latest_event.payload.inline_data.is_some() => {
                let text = latest_event
                    .payload
                    .inline_data
                    .clone()
                    .unwrap_or_default();
                latest_event.payload.digest.verify_bytes(text.as_bytes())?;
                Ok(SmartPasteDecision::PasteTextIntoEditor {
                    text,
                    content_hash: latest_event.payload.digest.full_hash.clone(),
                })
            }
            ContentCategory::Image | ContentCategory::File | ContentCategory::Directory => {
                let fallback_name = if latest_event.payload.content_category == ContentCategory::Image
                {
                    "clipboard_image.png"
                } else {
                    "downloaded_item"
                };
                let default_name = latest_event
                    .payload
                    .items_meta
                    .first()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| fallback_name.to_string());

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

    pub fn execute_alt_v(&self, latest_event: &CloudEventEnvelope) -> anyhow::Result<SmartPasteDecision> {
        let decision = self.route_alt_v(latest_event)?;
        if let SmartPasteDecision::PasteTextIntoEditor { ref text, .. } = decision {
            self.platform.write_text_to_clipboard_and_paste(text)?;
        }
        Ok(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::macos::MacOsPlatformAdapter;
    use mi_protocol::digest::DigestManifest;
    use mi_protocol::envelope::{
        EventPayload, EventType, ItemMeta, SourceDevice, TargetMode, TargetScope, UploadState,
        SPEC_VERSION_V6, TOPIC_CLIPBOARD_BROADCAST,
    };

    fn sample_envelope(category: ContentCategory, text: Option<&str>, item_name: &str) -> CloudEventEnvelope {
        let bytes = text.unwrap_or("file_bytes").as_bytes();
        let digest = DigestManifest::from_bytes(bytes);
        CloudEventEnvelope {
            spec_version: SPEC_VERSION_V6.to_string(),
            event_id: "evt_paste_01".to_string(),
            topic: TOPIC_CLIPBOARD_BROADCAST.to_string(),
            event_type: EventType::ClipboardBroadcast,
            timestamp_ms: 1700000000000,
            source_device: SourceDevice {
                device_id: "node_a".to_string(),
                device_name: "Node-A".to_string(),
                os: "windows".to_string(),
                lan_endpoint: Some("192.168.1.10:8443".to_string()),
            },
            target_scope: TargetScope {
                mode: TargetMode::Broadcast,
                target_device_ids: vec!["*".to_string()],
            },
            payload: EventPayload {
                content_category: category.clone(),
                digest,
                inline_data: text.map(|s| s.to_string()),
                file_cache_id: if text.is_none() {
                    Some("fc_paste_01".to_string())
                } else {
                    None
                },
                upload_state: UploadState::Completed,
                items_meta: vec![ItemMeta {
                    name: item_name.to_string(),
                    is_dir: category == ContentCategory::Directory,
                    size_bytes: bytes.len() as u64,
                    mtime: 1700000000,
                }],
                destination_override: None,
            },
        }
    }

    #[test]
    fn test_smart_paste_routing_all_three_modes() {
        // 1. Text -> PasteTextIntoEditor
        let text_env = sample_envelope(ContentCategory::Text, Some("Hello Multinteractor"), "clip.txt");
        let router_editor = SmartPasteRouter::new(MacOsPlatformAdapter {
            simulated_focus: Some(FocusedWindowContext::TextEditor {
                app_name: "Word".to_string(),
            }),
            ..Default::default()
        });
        assert!(matches!(
            router_editor.execute_alt_v(&text_env).unwrap(),
            SmartPasteDecision::PasteTextIntoEditor { .. }
        ));

        // 2. File in FileManager -> DirectSaveToFolder
        let file_env = sample_envelope(ContentCategory::File, None, "report.pdf");
        let router_fm = SmartPasteRouter::new(MacOsPlatformAdapter {
            simulated_focus: Some(FocusedWindowContext::FileManager {
                app_name: "Finder".to_string(),
                current_dir: PathBuf::from("/Users/test/Downloads"),
                selected_items: vec![],
            }),
            ..Default::default()
        });
        assert_eq!(
            router_fm.route_alt_v(&file_env).unwrap(),
            SmartPasteDecision::DirectSaveToFolder {
                event_id: "evt_paste_01".to_string(),
                target_dir: PathBuf::from("/Users/test/Downloads"),
                item_name: "report.pdf".to_string(),
            }
        );

        // 3. Directory in TextEditor -> ShowNativeSaveAsDialog
        let dir_env = sample_envelope(ContentCategory::Directory, None, "课程资料");
        let router_dlg = SmartPasteRouter::new(MacOsPlatformAdapter {
            simulated_focus: Some(FocusedWindowContext::TextEditor {
                app_name: "VSCode".to_string(),
            }),
            simulated_dialog_dir: Some(PathBuf::from("/Users/test/Desktop")),
            ..Default::default()
        });
        assert_eq!(
            router_dlg.route_alt_v(&dir_env).unwrap(),
            SmartPasteDecision::ShowNativeSaveAsDialog {
                event_id: "evt_paste_01".to_string(),
                default_item_name: "课程资料".to_string(),
                chosen_dir: Some(PathBuf::from("/Users/test/Desktop")),
            }
        );

        // 4. Image (file_cache_id, inline_data=None) in FileManager -> DirectSaveToFolder
        let img_env = sample_envelope(ContentCategory::Image, None, "screenshot.png");
        assert_eq!(
            router_fm.route_alt_v(&img_env).unwrap(),
            SmartPasteDecision::DirectSaveToFolder {
                event_id: "evt_paste_01".to_string(),
                target_dir: PathBuf::from("/Users/test/Downloads"),
                item_name: "screenshot.png".to_string(),
            }
        );

        // 5. Image (file_cache_id, inline_data=None) in TextEditor -> ShowNativeSaveAsDialog
        assert_eq!(
            router_dlg.route_alt_v(&img_env).unwrap(),
            SmartPasteDecision::ShowNativeSaveAsDialog {
                event_id: "evt_paste_01".to_string(),
                default_item_name: "screenshot.png".to_string(),
                chosen_dir: Some(PathBuf::from("/Users/test/Desktop")),
            }
        );
    }
}

