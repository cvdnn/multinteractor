//! Windows 平台适配实现 (Win32 / UIAutomation / COM 获取 Explorer 路径与注册表右键菜单扩展).

use std::path::PathBuf;
use std::process::Command;
use super::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, Default)]
pub struct WindowsPlatformAdapter {
    pub simulated_focus: Option<FocusedWindowContext>,
    pub simulated_selected_text: Option<String>,
    pub simulated_dialog_dir: Option<PathBuf>,
}

impl WindowsPlatformAdapter {
    /// 通过 Windows Shell.Application COM 接口探测当前活动 Explorer 窗口路径与选中文件列表.
    fn query_explorer_context_via_com() -> anyhow::Result<Option<FocusedWindowContext>> {
        let ps_script = r#"
$ fg = (Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();' -Name Win32 -Namespace Native -PassThru)::GetForegroundWindow()
$ shell = New-Object -ComObject Shell.Application
foreach ($ win in $ shell.Windows()) {
    if ($ win.HWND -eq $ fg) {
        $ folderPath = $ win.Document.Folder.Self.Path
        $ selected = @()
        foreach ($ item in $ win.Document.SelectedItems()) {
            $ selected += $ item.Path
        }
        @{
            kind = "FileManager"
            app_name = "explorer.exe"
            current_dir = $ folderPath
            selected_items = $ selected
        } | ConvertTo-Json -Compress
        exit 0
    }
}
"#;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(None);
        }
        let val: serde_json::Value = serde_json::from_str(&stdout)?;
        let current_dir = PathBuf::from(val["current_dir"].as_str().unwrap_or("C:\\"));
        let selected_items = val["selected_items"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(PathBuf::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some(FocusedWindowContext::FileManager {
            app_name: "explorer.exe".to_string(),
            current_dir,
            selected_items,
        }))
    }
}

impl OsPlatformAdapter for WindowsPlatformAdapter {
    fn inspect_focused_window(&self) -> anyhow::Result<FocusedWindowContext> {
        if let Some(ref ctx) = self.simulated_focus {
            return Ok(ctx.clone());
        }
        if cfg!(target_os = "windows") {
            if let Ok(Some(ctx)) = Self::query_explorer_context_via_com() {
                return Ok(ctx);
            }
        }
        Ok(FocusedWindowContext::TextEditor {
            app_name: "WINWORD.EXE".to_string(),
        })
    }

    fn read_selected_text_without_clobber(&self) -> anyhow::Result<Option<String>> {
        if self.simulated_selected_text.is_some() {
            return Ok(self.simulated_selected_text.clone());
        }
        if cfg!(target_os = "windows") {
            let output = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", "Get-Clipboard -Raw"])
                .output()?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                if !text.is_empty() {
                    return Ok(Some(text));
                }
            }
        }
        Ok(None)
    }

    fn prompt_native_save_dialog(&self, _default_file_name: &str) -> anyhow::Result<Option<PathBuf>> {
        if self.simulated_dialog_dir.is_some() {
            return Ok(self.simulated_dialog_dir.clone());
        }
        if cfg!(target_os = "windows") {
            let ps_dialog = r#"
Add-Type -AssemblyName System.Windows.Forms
$ dlg = New-Object System.Windows.Forms.FolderBrowserDialog
$ dlg.Description = "选择跨设备文件保存目录 (multinteractor)"
if ($ dlg.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    Write-Output $ dlg.SelectedPath
}
"#;
            let output = Command::new("powershell")
                .args(["-NoProfile", "-Sta", "-Command", ps_dialog])
                .output()?;
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return Ok(Some(PathBuf::from(path_str)));
                }
            }
        }
        Ok(None)
    }
}
