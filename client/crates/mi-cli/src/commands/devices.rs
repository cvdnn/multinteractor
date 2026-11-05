//! `mi devices list --json`：查询已注册设备列表、在线状态及内网可达性.

use mi_protocol::ipc::DeviceStatusDto;

pub fn format_devices_json(devices: &[DeviceStatusDto]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(devices)?)
}
