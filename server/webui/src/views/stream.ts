import { api, ApiError } from '../api';
import { openCacheDetailModal } from '../components/modals';
import { showToast } from '../components/toast';
import type { CacheQuotaStats, CacheSession } from '../types';
import { escapeHtml, formatBytes, formatTtlCountdown } from '../utils';

export function renderStreamView(
  container: HTMLElement,
  quota: CacheQuotaStats,
  sessions: CacheSession[],
  onRefresh: () => void
): void {
  const usageBarColor =
    quota.usage_pct >= 90 ? 'bg-rose-500' : quota.usage_pct >= 70 ? 'bg-amber-400' : 'bg-sky-500';
  const completedBytes = sessions
    .filter((s) => s.state === 'COMPLETED')
    .reduce((acc, s) => acc + s.written_offset, 0);

  const rowsHtml =
    sessions.length === 0
      ? `<tr><td colspan="6" class="py-10 text-center text-slate-500 text-xs font-sans">当前缓存池暂无活跃或保留中的会话文件</td></tr>`
      : sessions
          .map((s) => {
            const readersHtml =
              s.active_readers.length > 0
                ? s.active_readers
                    .map(
                      (r) => `
                        <div class="text-[11px] bg-indigo-500/10 text-indigo-300 border border-indigo-500/20 rounded px-2 py-0.5 mb-1">
                          ${escapeHtml(r.reader_device_id)} (${r.progress_pct.toFixed(1)}%)
                        </div>
                      `
                    )
                    .join('')
                : `<span class="text-slate-500">${s.state === 'COMPLETED' ? `已完成 ${s.download_count} 次拉取` : '等待消费端接入'}</span>`;

            const stateBadge =
              s.state === 'STREAMING'
                ? '<span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-[11px]">STREAMING (边传边下)</span>'
                : s.state === 'COMPLETED'
                  ? '<span class="px-2 py-0.5 rounded bg-slate-800 text-slate-300 text-[11px]">COMPLETED</span>'
                  : '<span class="px-2 py-0.5 rounded bg-rose-500/10 text-rose-400 text-[11px]">ABORTED</span>';

            return `
              <tr class="hover:bg-slate-900/50 ${s.state === 'STREAMING' ? 'bg-indigo-950/10' : ''}">
                <td class="py-3 px-4">
                  <div class="font-bold text-sky-400">${escapeHtml(s.file_cache_id)}</div>
                  <div class="text-[10px] text-slate-500">${escapeHtml(s.file_name)}</div>
                </td>
                <td class="py-3 px-4">
                  <span class="text-white font-sans font-medium">${escapeHtml(s.source_device_id)}</span>
                  <div class="text-[11px] text-slate-400 font-sans">${escapeHtml(s.content_category)} • ${s.verified_chunks}/${s.total_chunks} 块</div>
                </td>
                <td class="py-3 px-4">
                  <div class="flex items-center space-x-2">
                    <span class="text-sky-300">${formatBytes(s.written_offset)}</span>
                    <span class="text-slate-500">/ ${formatBytes(s.total_size)}</span>
                  </div>
                  <div class="w-36 bg-slate-800 rounded-full h-1.5 mt-1 overflow-hidden">
                    <div class="bg-sky-500 h-1.5 rounded-full" style="width: ${s.progress_pct}%"></div>
                  </div>
                </td>
                <td class="py-3 px-4 font-sans">${readersHtml}</td>
                <td class="py-3 px-4">
                  ${stateBadge}
                  <div class="text-[10px] text-amber-300 mt-1 font-mono" data-ttl-seconds="${s.ttl_remaining_seconds}">24h TTL 剩余 ${formatTtlCountdown(s.ttl_remaining_seconds)}</div>
                </td>
                <td class="py-3 px-4 text-right space-x-2 whitespace-nowrap font-sans">
                  <button data-action="detail" data-id="${escapeHtml(s.file_cache_id)}" class="cache-act text-sky-400 hover:text-sky-300 text-xs">分块摘要</button>
                  ${
                    s.state === 'STREAMING'
                      ? `<button data-action="abort" data-id="${escapeHtml(s.file_cache_id)}" class="cache-act text-amber-400 hover:text-amber-300 text-xs">终止</button>`
                      : ''
                  }
                  <button data-action="delete" data-id="${escapeHtml(s.file_cache_id)}" class="cache-act text-rose-400 hover:text-rose-300 text-xs">清除</button>
                </td>
              </tr>
            `;
          })
          .join('');

  container.innerHTML = `
    <div class="space-y-6">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 class="text-lg font-bold text-white">边传边下流式缓存池 (Stream Teeing Pool)</h2>
          <p class="text-xs text-slate-400">一写多读实时分流、Upload-Offset / Range 断点续传与严格 24 小时超时后台自动清理</p>
        </div>
        <div class="flex flex-wrap items-center gap-2.5">
          <button id="cache-prune-parts-btn" class="px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-800 hover:bg-slate-700 text-slate-200 transition border border-slate-700">
            释放孤立 .part 碎片
          </button>
          <button id="cache-prune-all-btn" class="px-3 py-1.5 rounded-lg text-xs font-medium bg-amber-500/15 hover:bg-amber-500/25 text-amber-300 transition border border-amber-500/30 flex items-center space-x-1.5">
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" /></svg>
            <span>立即清理 24H 过期缓存</span>
          </button>
        </div>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4">
          <span class="text-slate-400 text-xs font-medium">当前缓存池占用率</span>
          <div class="mt-2 flex items-baseline justify-between">
            <span class="text-xl font-bold font-mono text-white">${formatBytes(quota.used_bytes)} <span class="text-xs font-normal text-slate-400">/ ${formatBytes(quota.quota_bytes)}</span></span>
            <span class="text-xs font-mono font-semibold text-sky-400">${quota.usage_pct.toFixed(1)}%</span>
          </div>
          <div class="mt-2 w-full bg-slate-800 rounded-full h-1.5 overflow-hidden">
            <div class="${usageBarColor} h-1.5 rounded-full" style="width: ${Math.max(1, quota.usage_pct)}%"></div>
          </div>
          <div class="mt-2 text-[11px] text-slate-500">剩余配额 ${formatBytes(quota.remaining_bytes)} • 超出配额按 LRU 淘汰</div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4">
          <span class="text-slate-400 text-xs font-medium">活跃流式分流管道</span>
          <div class="mt-2 flex items-baseline justify-between">
            <span class="text-xl font-bold font-mono text-indigo-300">${quota.streaming_count} 活跃会话</span>
            <span class="text-xs px-2 py-0.5 rounded bg-indigo-500/20 text-indigo-300 font-mono">${quota.active_reader_count} 消费者</span>
          </div>
          <div class="mt-2 text-[11px] text-slate-400">支持 1 源端边上传、N 台目标端实时 Range 拉取</div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4">
          <span class="text-slate-400 text-xs font-medium">24H 留存归档文件</span>
          <div class="mt-2 flex items-baseline justify-between">
            <span class="text-xl font-bold font-mono text-emerald-400">${quota.completed_count} 个归档</span>
            <span class="text-xs text-slate-400 font-mono">合计 ${formatBytes(completedBytes)}</span>
          </div>
          <div class="mt-2 text-[11px] text-slate-400">满 ${Math.round(quota.ttl_seconds / 3600)}h 后台巡检自动清理销毁</div>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <div class="p-4 border-b border-slate-800 flex flex-col sm:flex-row sm:items-center justify-between gap-2">
          <span class="text-xs font-bold uppercase tracking-wider text-slate-300">流式中转会话与落盘清单</span>
          <span class="text-xs text-slate-500 font-mono">目录: ${escapeHtml(quota.cache_dir)}</span>
        </div>
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">缓存标识 / 文件</th>
              <th class="py-3 px-4">源设备 / 分块校验</th>
              <th class="py-3 px-4">落盘水位线 (Upload-Offset)</th>
              <th class="py-3 px-4">边传边下消费者</th>
              <th class="py-3 px-4">状态 / 24h TTL 倒计时</th>
              <th class="py-3 px-4 text-right">操作</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">${rowsHtml}</tbody>
        </table>
      </div>
    </div>
  `;

  container.querySelector('#cache-prune-parts-btn')?.addEventListener('click', async () => {
    try {
      const res = await api.pruneCache(true);
      showToast(
        `孤立碎片清理完成：移除 ${res.removed_sessions} 个中止会话与 ${res.orphan_files_removed} 个文件，释放 ${formatBytes(res.freed_bytes)}`,
        'success'
      );
      onRefresh();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '碎片清理失败', 'error');
    }
  });

  container.querySelector('#cache-prune-all-btn')?.addEventListener('click', async () => {
    try {
      const res = await api.pruneCache(false);
      showToast(
        `超期与失效缓存清理完成：移除 ${res.removed_sessions} 个会话与 ${res.orphan_files_removed} 个孤立文件，释放 ${formatBytes(res.freed_bytes)}`,
        'success'
      );
      onRefresh();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '清理失败', 'error');
    }
  });

  container.querySelectorAll<HTMLButtonElement>('.cache-act').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const action = btn.dataset['action'];
      const id = btn.dataset['id'];
      if (!action || !id) return;
      try {
        if (action === 'detail') {
          await openCacheDetailModal(id);
          return;
        }
        if (action === 'abort') {
          await api.abortCacheSession(id);
          showToast(`已终止缓存会话 ${id}`, 'warn');
        } else if (action === 'delete') {
          const res = await api.deleteCacheSession(id);
          showToast(`已清除缓存 ${id}，释放 ${formatBytes(res.freed_bytes)}`, 'success');
        }
        onRefresh();
      } catch (err) {
        showToast(err instanceof ApiError ? err.message : '缓存操作失败', 'error');
      }
    });
  });
}
