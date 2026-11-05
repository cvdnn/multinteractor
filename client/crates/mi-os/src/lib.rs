//! 操作系统原生深度交互集成层：全局热键 (`Alt+C`/`Alt+V`)、上下文抓取、焦点窗口智能粘贴与系统右键菜单扩展.

pub mod capture;
pub mod context_menu;
pub mod hotkey;
pub mod platform;
pub mod power;
pub mod smart_paste;
