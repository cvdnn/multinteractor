//! 操作系统鼠标右键【同步到... >】扩展菜单注册与在线设备列表动态更新 (FR-CTX-01, TASK-09).

use serde::{Deserialize, Serialize};
use mi_protocol::ipc::DeviceStatusDto;

pub const CONTEXT_MENU_ROOT_LABEL: &str = "同步到... >";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextMenuTargetItem {
    pub device_id: String,
    pub display_label: String,
    pub command_line: String,
}

#[derive(Debug, Clone, Default)]
pub struct ContextMenuRegistry {
    online_targets: Vec<ContextMenuTargetItem>,
}

impl ContextMenuRegistry {
    pub fn new() -> Self {
        Self {
            online_targets: Vec::new(),
        }
    }

    pub fn refresh_online_devices(&mut self, devices: &[DeviceStatusDto], local_device_id: &str) {
        self.online_targets = devices
            .iter()
            .filter(|d| d.online && d.device_id != local_device_id)
            .map(|d| ContextMenuTargetItem {
                device_id: d.device_id.clone(),
                display_label: format!("{} ({})", d.name, d.os),
                command_line: format!("mi-ui --pick-remote-dir --device \"{}\" --source \"%1\"", d.device_id),
            })
            .collect();
    }

    pub fn menu_items(&self) -> &[ContextMenuTargetItem] {
        &self.online_targets
    }

    /// 生成 Windows 文件 (`*`) 与文件夹 (`Directory`) 右键菜单注册表脚本.
    pub fn render_windows_reg_script(&self) -> String {
        let mut lines = vec!["Windows Registry Editor Version 5.00".to_string()];
        for scope in ["*", "Directory"] {
            lines.push(format!(
                r"[HKEY_CURRENT_USER\Software\Classes\{}\shell\MultinteractorSync]",
                scope
            ));
            lines.push(format!(r#""MUIVerb"="{}""#, CONTEXT_MENU_ROOT_LABEL));
            lines.push(r#""SubCommands"="""#.to_string());
            for item in &self.online_targets {
                lines.push(format!(
                    r"[HKEY_CURRENT_USER\Software\Classes\{}\shell\MultinteractorSync\shell\{}]",
                    scope, item.device_id
                ));
                lines.push(format!(r#""MUIVerb"="{}""#, item.display_label));
                lines.push(format!(
                    r"[HKEY_CURRENT_USER\Software\Classes\{}\shell\MultinteractorSync\shell\{}\command]",
                    scope, item.device_id
                ));
                lines.push(format!(r#"@="{}""#, item.command_line.replace('"', "\\\"")));
            }
        }
        lines.join("\n")
    }

    /// 生成 macOS Finder Quick Action (`NSServices` Info.plist) 服务配置.
    pub fn render_macos_quick_action_plist(&self) -> String {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSServices</key>
    <array>
        <dict>
            <key>NSMenuItem</key>
            <dict>
                <key>default</key>
                <string>同步到... (multinteractor)</string>
            </dict>
            <key>NSMessage</key>
            <string>runWorkflowAsService</string>
            <key>NSSendFileTypes</key>
            <array>
                <string>public.item</string>
                <string>public.folder</string>
            </array>
        </dict>
    </array>
</dict>
</plist>"#
            .to_string()
    }

    /// 将右键菜单扩展配置文件导出至指定目录 (`.reg` + macOS `Info.plist`).
    pub fn write_install_files(&self, out_dir: &std::path::Path) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
        std::fs::create_dir_all(out_dir)?;
        let reg_path = out_dir.join("multinteractor_context_menu.reg");
        let plist_path = out_dir.join("Info.plist");
        std::fs::write(&reg_path, self.render_windows_reg_script())?;
        std::fs::write(&plist_path, self.render_macos_quick_action_plist())?;
        Ok((reg_path, plist_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_menu_refresh_and_script_generation() {
        let mut reg = ContextMenuRegistry::new();
        let devices = vec![
            DeviceStatusDto {
                device_id: "node_a".to_string(),
                name: "Node-A".to_string(),
                os: "windows".to_string(),
                online: true,
                lan_endpoint: Some("192.168.1.10:8443".to_string()),
                lan_reachable: true,
                cert_sha256: None,
            },
            DeviceStatusDto {
                device_id: "node_c".to_string(),
                name: "Office-Mac".to_string(),
                os: "macos".to_string(),
                online: true,
                lan_endpoint: Some("192.168.1.12:8443".to_string()),
                lan_reachable: true,
                cert_sha256: None,
            },
        ];
        reg.refresh_online_devices(&devices, "node_a");
        assert_eq!(reg.menu_items().len(), 1);
        assert_eq!(reg.menu_items()[0].device_id, "node_c");
        assert!(reg.render_windows_reg_script().contains("Office-Mac (macos)"));
        assert!(reg.render_macos_quick_action_plist().contains("同步到... (multinteractor)"));
    }
}

