import { api, ApiError } from '../api';
import { showToast } from '../components/toast';
import type { SystemStatusResponse } from '../types';
import { escapeHtml, formatBytes, formatDuration } from '../utils';

export function renderSettingsView(
  container: HTMLElement,
  data: SystemStatusResponse,
  onRefresh: () => void
): void {
  container.innerHTML = `
    <div class="space-y-6">
      <div>
        <h2 class="text-lg font-bold text-white">云端中枢运行参数与系统健康 (System Health & Config)</h2>
        <p class="text-xs text-slate-400">查看 CloudHubService 运行环境、SQLite WAL 存储参数、安全平面配置及快捷运维操作</p>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-3">
          <h3 class="text-sm font-semibold text-white">基础服务运行环境</h3>
          <div class="space-y-2 text-xs divide-y divide-slate-800/80">
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">服务引擎:</span>
              <span class="text-slate-200 font-mono">Python ${escapeHtml(data.runtime.python_version)} + FastAPI ${escapeHtml(data.runtime.fastapi_version)}</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">监听绑定地址:</span>
              <span class="text-slate-200 font-mono">${escapeHtml(data.server.host)}:${data.server.port}</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">持续运行时长:</span>
              <span class="text-emerald-400 font-mono">${escapeHtml(formatDuration(data.uptime_seconds))}</span>
            </div>
            <div class="flex items-center justify-between py-1.5 gap-3">
              <span class="text-slate-400 shrink-0">数据目录 (MI_SERVER_DATA_DIR):</span>
              <span class="text-slate-300 font-mono text-[11px] truncate">${escapeHtml(data.server.data_dir)}</span>
            </div>
            <div class="flex items-center justify-between py-1.5 gap-3">
              <span class="text-slate-400 shrink-0">SQLite WAL 状态:</span>
              <span class="text-slate-300 font-mono text-[11px]">${escapeHtml(data.wal.journal_mode)} / SYNC=${escapeHtml(data.wal.synchronous)} (DB ${formatBytes(data.wal.db_size_bytes)} • WAL ${formatBytes(data.wal.wal_size_bytes)})</span>
            </div>
            <div class="flex items-center justify-between py-1.5 gap-3">
              <span class="text-slate-400 shrink-0">流式缓存池目录:</span>
              <span class="text-slate-300 font-mono text-[11px] truncate">${escapeHtml(data.server.cache_dir)}</span>
            </div>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-3">
          <h3 class="text-sm font-semibold text-white">协同、缓存与安全配额规则</h3>
          <div class="space-y-2 text-xs divide-y divide-slate-800/80">
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">云端数据缓存 TTL:</span>
              <span class="text-sky-400 font-mono">${data.policies.cache_ttl_seconds}s (${Math.round(data.policies.cache_ttl_seconds / 3600)}h 超时自动清理)</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">缓存巡检周期:</span>
              <span class="text-slate-300 font-mono">每 ${data.policies.cache_cleanup_interval_seconds}s 后台扫描</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">缓存配额上限:</span>
              <span class="text-sky-400 font-mono">${formatBytes(data.policies.cache_quota_bytes)}</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">内网通道空闲释放 (客户端侧计时):</span>
              <span class="text-emerald-400 font-mono">${data.policies.lan_idle_timeout_seconds}s (Rust 客户端自主控制)</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">审计日志保留期:</span>
              <span class="text-slate-200 font-mono">${data.policies.audit_retention_days} 天</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">分块 BLAKE3 块大小:</span>
              <span class="text-slate-200 font-mono">${formatBytes(data.policies.chunk_size_bytes)}</span>
            </div>
          </div>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-3">
        <h3 class="text-sm font-semibold text-white">运维与诊断快捷操作</h3>
        <div class="flex flex-wrap items-center gap-3">
          <button id="settings-wal-ckpt-btn" class="px-3 py-2 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 text-xs text-slate-200 transition">
            执行 WAL Checkpoint (TRUNCATE)
          </button>
          <button id="settings-cleanup-audit-btn" class="px-3 py-2 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 text-xs text-slate-200 transition">
            清理超期 (${data.policies.audit_retention_days} 天前) 审计日志
          </button>
          <a href="/docs" target="_blank" rel="noopener noreferrer" class="px-3 py-2 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 text-xs text-sky-400 transition inline-flex items-center space-x-1">
            <span>打开 FastAPI OpenAPI (Swagger) 文档</span>
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" /></svg>
          </a>
        </div>
      </div>
    </div>
  `;

  container.querySelector('#settings-wal-ckpt-btn')?.addEventListener('click', async () => {
    try {
      const res = await api.runWalCheckpoint();
      showToast(`WAL 检查点完成！当前 WAL 体积 ${formatBytes(res.wal_size_bytes)}`, 'success');
      onRefresh();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : 'WAL Checkpoint 执行失败', 'error');
    }
  });

  container.querySelector('#settings-cleanup-audit-btn')?.addEventListener('click', async () => {
    try {
      const res = await api.cleanupExpiredAudit();
      showToast(`超期审计日志清理完成：已清理 ${res.deleted_rows} 条记录`, 'info');
      onRefresh();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '清理失败', 'error');
    }
  });
}
