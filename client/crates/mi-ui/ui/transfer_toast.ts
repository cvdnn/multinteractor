// 大文件传输进度悬浮条与断点续传状态提示组件 (FR-TRANS-03)

export interface TransferProgressState {
  task_id: string;
  file_name: string;
  verified_offset: number;
  total_size: number;
  channel_used: "LAN_DIRECT" | "CLOUD_CACHE";
  digest_verified: boolean;
  status: "STREAMING" | "INTERRUPTED" | "COMPLETED";
}

export function formatTransferToastText(state: TransferProgressState): string {
  const pct =
    state.total_size > 0
      ? Math.min(100, Math.round((state.verified_offset / state.total_size) * 100))
      : 100;
  const channelLabel = state.channel_used === "LAN_DIRECT" ? "内网即用即连" : "云端边传边下缓存";
  if (state.status === "COMPLETED" && state.digest_verified) {
    return `[已完成] ${state.file_name} (100% | BLAKE3 摘要校验通过 | ${channelLabel})`;
  }
  if (state.status === "INTERRUPTED") {
    return `[断点保护] ${state.file_name} (已校验落盘 ${pct}%，等待续传...)`;
  }
  return `[传输中] ${state.file_name} (${pct}% | ${channelLabel})`;
}
