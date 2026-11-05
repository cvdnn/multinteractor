//! macOS 平台适配实现 (Accessibility API / Finder 路径探测与 Finder Quick Actions 右键扩展).

use std::path::PathBuf;
use std::process::Command;
use super::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, Default)]
pub struct MacOsPlatformAdapter {
    pub simulated_focus: Option<FocusedWindowContext>,
    pub simulated_selected_text: Option<String>,
    pub simulated_dialog_dir: Option<PathBuf>,
}

impl MacOsPlatformAdapter {
    /// 通过 macOS AppleScript (`osascript`) 探测当前前台应用名称、Finder 当前打开目录及选中项路径.
    fn query_macos_frontmost_context() -> anyhow::Result<FocusedWindowContext> {
        let app_out = Command::new("osascript")
            .args([
                "-e",
                r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
            ])
            .output()?;
        let app_name = String::from_utf8_lossy(&app_out.stdout).trim().to_string();

        if app_name.eq_ignore_ascii_case("Finder") {
            let dir_out = Command::new("osascript")
                .args([
                    "-e",
                    r#"tell application "Finder" to if (count of Finder windows) > 0 then get POSIX path of (target of front Finder window as alias)"#,
                ])
                .output()?;
            let current_dir_str = String::from_utf8_lossy(&dir_out.stdout).trim().to_string();
            let current_dir = if current_dir_str.is_empty() {
                std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
            } else {
                PathBuf::from(current_dir_str)
            };

            let sel_out = Command::new("osascript")
                .args([
                    "-e",
                    r#"tell application "Finder"
                        set selList to selection as alias list
                        set outPaths to ""
                        repeat with itemRef in selList
                            set outPaths to outPaths & (POSIX path of itemRef) & linefeed
                        end repeat
                        return outPaths
                    end tell"#,
                ])
                .output()?;
            let selected_items: Vec<PathBuf> = String::from_utf8_lossy(&sel_out.stdout)
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect();

            return Ok(FocusedWindowContext::FileManager {
                app_name: "Finder".to_string(),
                current_dir,
                selected_items,
            });
        }

        Ok(FocusedWindowContext::TextEditor {
            app_name: if app_name.is_empty() {
                "TextEdit".to_string()
            } else {
                app_name
            },
        })
    }
}

impl OsPlatformAdapter for MacOsPlatformAdapter {
    fn inspect_focused_window(&self) -> anyhow::Result<FocusedWindowContext> {
        if let Some(ref ctx) = self.simulated_focus {
            return Ok(ctx.clone());
        }
        if cfg!(target_os = "macos") {
            if let Ok(ctx) = Self::query_macos_frontmost_context() {
                return Ok(ctx);
            }
        }
        Ok(FocusedWindowContext::TextEditor {
            app_name: "TextEdit".to_string(),
        })
    }

    fn read_selected_text_without_clobber(&self) -> anyhow::Result<Option<String>> {
        if self.simulated_selected_text.is_some() {
            return Ok(self.simulated_selected_text.clone());
        }
        if cfg!(target_os = "macos") {
            let output = Command::new("pbpaste").output()?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                if !text.is_empty() {
                    return Ok(Some(text));
                }
            }
        }
        Ok(None)
    }

    fn prompt_native_save_dialog(&self, default_file_name: &str) -> anyhow::Result<Option<PathBuf>> {
        if self.simulated_dialog_dir.is_some() {
            return Ok(self.simulated_dialog_dir.clone());
        }
        if cfg!(target_os = "macos") {
            let script = format!(
                r#"POSIX path of (choose folder with prompt "选择保存 {} 的目标目录 (multinteractor):")"#,
                default_file_name.replace('"', "\\\"")
            );
            let output = Command::new("osascript").args(["-e", &script]).output()?;
            if output.status.success() {
                let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !p.is_empty() {
                    return Ok(Some(PathBuf::from(p)));
                }
            }
        }
        Ok(None)
    }
}
