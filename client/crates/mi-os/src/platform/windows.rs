//! Windows 平台适配实现 (Win32 / UIAutomation / COM 获取 Explorer 路径与注册表右键菜单扩展).

use std::path::PathBuf;
use std::process::Command;
use super::{FocusedWindowContext, OsPlatformAdapter};

#[derive(Debug, Clone, Default)]
pub struct WindowsPlatformAdapter {
    pub simulated_focus: Option<FocusedWindowContext>,
    pub simulated_selected_text: Option<String>,
    pub simulated_image_bytes: Option<Vec<u8>>,
    pub simulated_dialog_dir: Option<PathBuf>,
}

impl WindowsPlatformAdapter {
    const PS_UTF8_PREFIX: &'static str =
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8; ";

    pub const EXPLORER_COM_PS_SCRIPT: &'static str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
$fg = (Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();' -Name Win32 -Namespace Native -PassThru)::GetForegroundWindow()
$shell = New-Object -ComObject Shell.Application
foreach ($win in $shell.Windows()) {
    if ($win.HWND -eq $fg) {
        $folderPath = $win.Document.Folder.Self.Path
        $selected = @()
        foreach ($item in $win.Document.SelectedItems()) {
            $selected += $item.Path
        }
        @{
            kind = "FileManager"
            app_name = "explorer.exe"
            current_dir = $folderPath
            selected_items = $selected
        } | ConvertTo-Json -Compress
        exit 0
    }
}
"#;

    pub const FOLDER_DIALOG_PS_SCRIPT: &'static str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.FolderBrowserDialog
$dlg.Description = "选择跨设备文件保存目录 (multinteractor)"
if ($dlg.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    Write-Output $dlg.SelectedPath
}
"#;

    pub const UIA_SELECTED_TEXT_PS_SCRIPT: &'static str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient
$elem = [System.Windows.Automation.AutomationElement]::FocusedElement
if ($null -ne $elem) {
    $patObj = $null
    if ($elem.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$patObj)) {
        $sel = $patObj.GetSelection()
        if ($sel.Length -gt 0) {
            $txt = $sel[0].GetText(-1)
            if (-not [string]::IsNullOrEmpty($txt)) {
                Write-Output $txt
                exit 0
            }
        }
    }
}
"#;

    pub const CLIPBOARD_PASTE_PS_SCRIPT: &'static str = r#"
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
$reader = New-Object System.IO.StreamReader([Console]::OpenStandardInput(), [System.Text.Encoding]::UTF8)
$txt = $reader.ReadToEnd()
Add-Type -AssemblyName System.Windows.Forms
if (-not [string]::IsNullOrEmpty($txt)) {
    [System.Windows.Forms.Clipboard]::SetText($txt)
}
[System.Windows.Forms.SendKeys]::SendWait('^v')
"#;

    /// 通过 Windows Shell.Application COM 接口探测当前活动 Explorer 窗口路径与选中文件列表.
    fn query_explorer_context_via_com() -> anyhow::Result<Option<FocusedWindowContext>> {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                Self::EXPLORER_COM_PS_SCRIPT,
            ])
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
        if self.simulated_selected_text.is_some()
            || self.simulated_focus.is_some()
            || self.simulated_image_bytes.is_some()
            || self.simulated_dialog_dir.is_some()
        {
            return Ok(self.simulated_selected_text.clone());
        }
        if cfg!(target_os = "windows") {
            // 1. 优先通过 UIAutomation TextPattern 无损获取当前焦点控件选中文本
            if let Ok(uia_out) = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", Self::UIA_SELECTED_TEXT_PS_SCRIPT])
                .output()
            {
                if uia_out.status.success() {
                    let sel = String::from_utf8_lossy(&uia_out.stdout)
                        .trim_end_matches(&['\r', '\n'][..])
                        .to_string();
                    if !sel.is_empty() {
                        return Ok(Some(sel));
                    }
                }
            }

            // 2. 回退读取系统剪贴板（强制 UTF-8 输出避免中文 Windows CP936/GBK 乱码）
            let clip_cmd = format!("{}Get-Clipboard -Raw", Self::PS_UTF8_PREFIX);
            let output = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &clip_cmd])
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

    fn read_clipboard_image_bytes(&self) -> anyhow::Result<Option<Vec<u8>>> {
        if self.simulated_image_bytes.is_some() {
            return Ok(self.simulated_image_bytes.clone());
        }
        if cfg!(target_os = "windows") {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let tmp_path = std::env::temp_dir()
                .join(format!("mi_clip_{}_{}.png", std::process::id(), unique));
            let escaped = tmp_path.to_string_lossy().replace('\'', "''");
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $img = [System.Windows.Forms.Clipboard]::GetImage(); if ($img -ne $null) {{ $img.Save('{escaped}', [System.Drawing.Imaging.ImageFormat]::Png); 'OK' }}"
            );
            let output = Command::new("powershell")
                .args(["-NoProfile", "-Sta", "-Command", &script])
                .output();
            let result = if let Ok(out) = output {
                if out.status.success() && tmp_path.exists() {
                    std::fs::read(&tmp_path).ok().filter(|b| !b.is_empty())
                } else {
                    None
                }
            } else {
                None
            };
            let _ = std::fs::remove_file(&tmp_path);
            if result.is_some() {
                return Ok(result);
            }
        }
        Ok(None)
    }

    fn prompt_native_save_dialog(&self, _default_file_name: &str) -> anyhow::Result<Option<PathBuf>> {
        if self.simulated_dialog_dir.is_some() {
            return Ok(self.simulated_dialog_dir.clone());
        }
        if cfg!(target_os = "windows") {
            let output = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Sta",
                    "-Command",
                    Self::FOLDER_DIALOG_PS_SCRIPT,
                ])
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

    fn write_text_to_clipboard_and_paste(&self, text: &str) -> anyhow::Result<()> {
        if self.simulated_focus.is_some()
            || self.simulated_selected_text.is_some()
            || self.simulated_image_bytes.is_some()
            || self.simulated_dialog_dir.is_some()
        {
            return Ok(());
        }
        if cfg!(target_os = "windows") {
            use std::io::Write;
            use std::process::Stdio;
            let mut child = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Sta",
                    "-Command",
                    Self::CLIPBOARD_PASTE_PS_SCRIPT,
                ])
                .stdin(Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes())?;
            }
            let status = child.wait()?;
            if !status.success() {
                anyhow::bail!("powershell clipboard paste failed with status: {}", status);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_powershell_scripts_have_valid_variable_syntax() {
        assert!(!WindowsPlatformAdapter::EXPLORER_COM_PS_SCRIPT.contains("$ "));
        assert!(!WindowsPlatformAdapter::FOLDER_DIALOG_PS_SCRIPT.contains("$ "));
        assert!(!WindowsPlatformAdapter::UIA_SELECTED_TEXT_PS_SCRIPT.contains("$ "));
        assert!(!WindowsPlatformAdapter::CLIPBOARD_PASTE_PS_SCRIPT.contains("$ "));
        assert!(WindowsPlatformAdapter::EXPLORER_COM_PS_SCRIPT.contains("$fg"));
        assert!(WindowsPlatformAdapter::EXPLORER_COM_PS_SCRIPT.contains("$shell"));
        assert!(WindowsPlatformAdapter::FOLDER_DIALOG_PS_SCRIPT.contains("$dlg"));
        assert!(WindowsPlatformAdapter::EXPLORER_COM_PS_SCRIPT.contains("Encoding]::UTF8"));
        assert!(WindowsPlatformAdapter::FOLDER_DIALOG_PS_SCRIPT.contains("Encoding]::UTF8"));
        assert!(WindowsPlatformAdapter::UIA_SELECTED_TEXT_PS_SCRIPT.contains("Encoding]::UTF8"));
        assert!(WindowsPlatformAdapter::CLIPBOARD_PASTE_PS_SCRIPT.contains("Encoding]::UTF8"));
    }
}

