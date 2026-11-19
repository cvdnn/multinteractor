//! Linux 平台适配实现 (X11/Wayland 焦点探测、PRIMARY/CLIPBOARD 选区读取、文件管理器与原生保存对话框).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, Default)]
pub struct LinuxPlatformAdapter {
    pub simulated_focus: Option<FocusedWindowContext>,
    pub simulated_selected_text: Option<String>,
    pub simulated_image_bytes: Option<Vec<u8>>,
    pub simulated_dialog_dir: Option<PathBuf>,
}

impl LinuxPlatformAdapter {
    fn is_wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
    }

    fn run_capture_cmd(program: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(program).args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        if s.trim().is_empty() {
            None
        } else {
            Some(s)
        }
    }

    fn detect_active_window_class() -> Option<String> {
        let win_id = Self::run_capture_cmd("xdotool", &["getactivewindow"])?;
        let win_id = win_id.trim();
        let xprop = Self::run_capture_cmd("xprop", &["-id", win_id, "WM_CLASS"])?;
        Some(xprop.trim().to_string())
    }

    fn is_linux_file_manager(wm_class: &str) -> Option<&'static str> {
        let lower = wm_class.to_ascii_lowercase();
        for fm in ["nautilus", "dolphin", "thunar", "nemo", "pcmanfm", "caja"] {
            if lower.contains(fm) {
                return Some(fm);
            }
        }
        None
    }

    fn default_home_dir() -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"))
    }
}

impl OsPlatformAdapter for LinuxPlatformAdapter {
    fn inspect_focused_window(&self) -> anyhow::Result<FocusedWindowContext> {
        if let Some(ref ctx) = self.simulated_focus {
            return Ok(ctx.clone());
        }
        if cfg!(target_os = "linux") {
            if let Some(wm_class) = Self::detect_active_window_class() {
                if let Some(fm) = Self::is_linux_file_manager(&wm_class) {
                    return Ok(FocusedWindowContext::FileManager {
                        app_name: fm.to_string(),
                        current_dir: Self::default_home_dir(),
                        selected_items: vec![],
                    });
                }
                return Ok(FocusedWindowContext::TextEditor { app_name: wm_class });
            }
        }
        Ok(FocusedWindowContext::TextEditor {
            app_name: "linux-editor".to_string(),
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
        if cfg!(target_os = "linux") {
            // 1. 优先读取 Linux PRIMARY 选区（鼠标/键盘选中即生效，不污染 CLIPBOARD）
            if Self::is_wayland() {
                if let Some(txt) = Self::run_capture_cmd("wl-paste", &["--primary", "--no-newline"]) {
                    return Ok(Some(txt));
                }
            }
            if let Some(txt) = Self::run_capture_cmd("xclip", &["-o", "-selection", "primary"]) {
                return Ok(Some(txt));
            }
            if let Some(txt) = Self::run_capture_cmd("xsel", &["-p", "-o"]) {
                return Ok(Some(txt));
            }

            // 2. 回退读取 CLIPBOARD
            if Self::is_wayland() {
                if let Some(txt) = Self::run_capture_cmd("wl-paste", &["--no-newline"]) {
                    return Ok(Some(txt));
                }
            }
            if let Some(txt) = Self::run_capture_cmd("xclip", &["-o", "-selection", "clipboard"]) {
                return Ok(Some(txt));
            }
            if let Some(txt) = Self::run_capture_cmd("xsel", &["-b", "-o"]) {
                return Ok(Some(txt));
            }
        }
        Ok(None)
    }

    fn read_clipboard_image_bytes(&self) -> anyhow::Result<Option<Vec<u8>>> {
        if self.simulated_image_bytes.is_some() {
            return Ok(self.simulated_image_bytes.clone());
        }
        if cfg!(target_os = "linux") {
            let candidates: &[(&str, &[&str])] = if Self::is_wayland() {
                &[
                    ("wl-paste", &["--no-newline", "--type", "image/png"]),
                    ("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"]),
                ]
            } else {
                &[
                    ("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"]),
                    ("wl-paste", &["--no-newline", "--type", "image/png"]),
                ]
            };
            for (prog, args) in candidates {
                if let Ok(out) = Command::new(prog).args(*args).output() {
                    if out.status.success() && !out.stdout.is_empty() {
                        return Ok(Some(out.stdout));
                    }
                }
            }
        }
        Ok(None)
    }

    fn prompt_native_save_dialog(&self, default_file_name: &str) -> anyhow::Result<Option<PathBuf>> {
        if self.simulated_dialog_dir.is_some() {
            return Ok(self.simulated_dialog_dir.clone());
        }
        if cfg!(target_os = "linux") {
            let title = format!("选择保存 {} 的目标目录 (multinteractor)", default_file_name);
            if let Some(out) = Self::run_capture_cmd(
                "zenity",
                &["--file-selection", "--directory", "--title", &title],
            ) {
                return Ok(Some(PathBuf::from(out.trim())));
            }
            if let Some(out) = Self::run_capture_cmd(
                "kdialog",
                &["--getexistingdirectory", ".", "--title", &title],
            ) {
                return Ok(Some(PathBuf::from(out.trim())));
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
        if cfg!(target_os = "linux") {
            let mut copied = false;
            let candidates: &[(&str, &[&str])] = if Self::is_wayland() {
                &[
                    ("wl-copy", &[]),
                    ("xclip", &["-selection", "clipboard"]),
                    ("xsel", &["-b", "-i"]),
                ]
            } else {
                &[
                    ("xclip", &["-selection", "clipboard"]),
                    ("xsel", &["-b", "-i"]),
                    ("wl-copy", &[]),
                ]
            };

            for (bin, args) in candidates {
                if let Ok(mut child) = Command::new(bin).args(*args).stdin(Stdio::piped()).spawn() {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    if let Ok(status) = child.wait() {
                        if status.success() {
                            copied = true;
                            break;
                        }
                    }
                }
            }

            if copied {
                if Self::is_wayland() {
                    let _ = Command::new("wtype")
                        .args(["-M", "ctrl", "-P", "v", "-p", "v", "-m", "ctrl"])
                        .output();
                } else {
                    let _ = Command::new("xdotool")
                        .args(["key", "--clearmodifiers", "ctrl+v"])
                        .output();
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linux_file_manager_detection_and_simulated_capture() {
        assert_eq!(
            LinuxPlatformAdapter::is_linux_file_manager("\"org.gnome.Nautilus\""),
            Some("nautilus")
        );
        assert_eq!(
            LinuxPlatformAdapter::is_linux_file_manager("\"dolphin\""),
            Some("dolphin")
        );
        assert_eq!(
            LinuxPlatformAdapter::is_linux_file_manager("\"code\""),
            None
        );

        let adapter = LinuxPlatformAdapter {
            simulated_selected_text: Some("linux clipboard text".to_string()),
            ..Default::default()
        };
        assert_eq!(
            adapter.read_selected_text_without_clobber().unwrap(),
            Some("linux clipboard text".to_string())
        );
    }
}
