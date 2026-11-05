//! 全局快捷键监听引擎 (`Alt+C` 复制广播 / `Alt+V` 智能粘贴) (FR-COPY-01, FR-PASTE-01).

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY_COPY: &str = "Alt+C";
pub const DEFAULT_HOTKEY_PASTE: &str = "Alt+V";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotkeyAction {
    BroadcastCopy,
    SmartPaste,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalHotkeyBinding {
    pub copy_chord: String,
    pub paste_chord: String,
}

impl Default for GlobalHotkeyBinding {
    fn default() -> Self {
        Self {
            copy_chord: DEFAULT_HOTKEY_COPY.to_string(),
            paste_chord: DEFAULT_HOTKEY_PASTE.to_string(),
        }
    }
}

impl GlobalHotkeyBinding {
    pub fn resolve_chord(&self, chord: &str) -> Option<HotkeyAction> {
        if chord.eq_ignore_ascii_case(&self.copy_chord) {
            Some(HotkeyAction::BroadcastCopy)
        } else if chord.eq_ignore_ascii_case(&self.paste_chord) {
            Some(HotkeyAction::SmartPaste)
        } else {
            None
        }
    }
}
