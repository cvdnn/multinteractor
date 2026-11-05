//! `mi events list [--limit N] --json`：查看本机接收事件缓存列表中的待粘贴事件.

use mi_protocol::envelope::CloudEventEnvelope;

pub fn format_events_json(events: &[CloudEventEnvelope], limit: usize) -> anyhow::Result<String> {
    let slice: Vec<&CloudEventEnvelope> = events.iter().take(limit).collect();
    Ok(serde_json::to_string_pretty(&slice)?)
}
