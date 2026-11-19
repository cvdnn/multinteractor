//! macOS 平台适配实现 (Accessibility API / Finder 路径探测与 Finder Quick Actions 右键扩展).

use std::path::PathBuf;
use std::process::Command;
use super::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, Default)]
pub struct MacOsPlatformAdapter {
    pub simulated_focus: Option<FocusedWindowContext>,
    pub simulated_selected_text: Option<String>,
    pub simulated_image_bytes: Option<Vec<u8>>,
    pub simulated_dialog_dir: Option<PathBuf>,
}

impl MacOsPlatformAdapter {
    fn utf8_command(program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env("LANG", "en_US.UTF-8").env("LC_CTYPE", "UTF-8");
        cmd
    }

    /// 通过 macOS AppleScript (`osascript`) 探测当前前台应用名称、Finder 当前打开目录及选中项路径.
    fn query_macos_frontmost_context() -> anyhow::Result<FocusedWindowContext> {
        let app_out = Self::utf8_command("osascript")
            .args([
                "-e",
                r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
            ])
            .output()?;
        let app_name = String::from_utf8_lossy(&app_out.stdout).trim().to_string();

        if app_name.eq_ignore_ascii_case("Finder") {
            let dir_out = Self::utf8_command("osascript")
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

            let sel_out = Self::utf8_command("osascript")
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
        if self.simulated_selected_text.is_some()
            || self.simulated_focus.is_some()
            || self.simulated_image_bytes.is_some()
            || self.simulated_dialog_dir.is_some()
        {
            return Ok(self.simulated_selected_text.clone());
        }
        if cfg!(target_os = "macos") {
            // Query AXSelectedText via Accessibility without overwriting system pasteboard
            let ax_out = Self::utf8_command("osascript")
                .args([
                    "-e",
                    r#"tell application "System Events"
                        set frontProc to first application process whose frontmost is true
                        try
                            set focusedElem to value of attribute "AXFocusedUIElement" of frontProc
                            return value of attribute "AXSelectedText" of focusedElem
                        end try
                    end tell"#,
                ])
                .output();
            if let Ok(out) = ax_out {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !s.is_empty() {
                        return Ok(Some(s));
                    }
                }
            }
            let output = Self::utf8_command("pbpaste").output()?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                if !text.is_empty() {
                    return Ok(Some(text));
                }
            }
        }
        Ok(None)
    }

    fn read_clipboard_image_bytes(&self) -> anyhow::Result<Option<Vec<u8>>> {
        if self.simulated_image_bytes.is_some() {
            return Ok(self.simulated_image_bytes.clone());
        }
        if cfg!(target_os = "macos") {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let pid = std::process::id();
            let png_path = std::env::temp_dir().join(format!("mi_clip_{}_{}.png", pid, unique));
            let tiff_path = std::env::temp_dir().join(format!("mi_clip_{}_{}.tiff", pid, unique));
            let png_escaped = png_path.to_string_lossy().replace('"', "\\\"");
            let tiff_escaped = tiff_path.to_string_lossy().replace('"', "\\\"");

            let script = format!(
                r#"try
    set imgData to the clipboard as «class PNGf»
    set fRef to open for access (POSIX file "{png}") with write permission
    set eof of fRef to 0
    write imgData to fRef
    close access fRef
    return "PNG"
on error
    try
        close access (POSIX file "{png}")
    end try
    try
        set imgData to the clipboard as TIFF picture
        set fRef to open for access (POSIX file "{tiff}") with write permission
        set eof of fRef to 0
        write imgData to fRef
        close access fRef
        return "TIFF"
    on error
        try
            close access (POSIX file "{tiff}")
        end try
        return "NONE"
    end try
end try"#,
                png = png_escaped,
                tiff = tiff_escaped
            );

            let output = Self::utf8_command("osascript").args(["-e", &script]).output();
            let result = if let Ok(out) = output {
                let kind = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if kind == "PNG" && png_path.exists() {
                    std::fs::read(&png_path).ok().filter(|b| !b.is_empty())
                } else if kind == "TIFF" && tiff_path.exists() {
                    let sips_ok = Self::utf8_command("sips")
                        .args([
                            "-s",
                            "format",
                            "png",
                            &tiff_path.to_string_lossy(),
                            "--out",
                            &png_path.to_string_lossy(),
                        ])
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false);
                    if sips_ok && png_path.exists() {
                        std::fs::read(&png_path).ok().filter(|b| !b.is_empty())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let _ = std::fs::remove_file(&png_path);
            let _ = std::fs::remove_file(&tiff_path);
            if result.is_some() {
                return Ok(result);
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
            let output = Self::utf8_command("osascript").args(["-e", &script]).output()?;
            if output.status.success() {
                let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !p.is_empty() {
                    return Ok(Some(PathBuf::from(p)));
                }
            }
        }
        Ok(None)
    }

    fn write_text_to_clipboard_and_paste(&self, text: &str) -> anyhow::Result<()> {
        if self.simulated_focus.is_some()
            || self.simulated_selected_text.is_some()
            || self.simulated_image_bytes.is_some()
            || self.simulated_dialog_dir.is_some()
        {
            return Ok(());
        }
        if cfg!(target_os = "macos") {
            use std::io::Write;
            use std::process::Stdio;
            let mut child = Self::utf8_command("pbcopy")
                .stdin(Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes())?;
            }
            let status = child.wait()?;
            if !status.success() {
                anyhow::bail!("pbcopy failed with status: {}", status);
            }
            let _ = Self::utf8_command("osascript")
                .args([
                    "-e",
                    r#"tell application "System Events" to keystroke "v" using command down"#,
                ])
                .output();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf8_command_sets_utf8_locale() {
        let cmd = MacOsPlatformAdapter::utf8_command("pbpaste");
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs
            .iter()
            .any(|(k, v)| k == &"LANG" && v == &Some("en_US.UTF-8".as_ref())));
        assert!(envs
            .iter()
            .any(|(k, v)| k == &"LC_CTYPE" && v == &Some("UTF-8".as_ref())));
    }
}

