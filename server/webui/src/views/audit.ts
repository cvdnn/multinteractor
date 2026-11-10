import { api, ApiError } from '../api';
import { showToast } from '../components/toast';
import type { AuditLogItem, AuditLogsPagedResponse, RegisteredDevice } from '../types';
import { escapeHtml, formatBytes, formatTimestampMs, shortHash } from '../utils';

export interface AuditFilterState {
  logType: 'ALL' | 'TRANSFER' | 'EVENT' | 'CONNECTION';
  deviceId: string;
  status: string;
  keyword: string;
  limit: number;
  offset: number;
}

function renderAuditTableRow(item: AuditLogItem): string {
  const ts = formatTimestampMs(item.timestamp_ms);
  if (item.record_type === 'TRANSFER') {
    const ok = item.verification_status === 'VERIFIED_OK';
    return `
      <tr class="hover:bg-slate-900/50">
        <td class="py-3 px-4 text-slate-400">
          <div class="text-slate-200">#${item.id}</div>
          <div class="text-[10px] text-slate-500">${escapeHtml(ts)}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-400 border border-indigo-500/20 text-[10px]">audit_transfer_log</span>
        </td>
        <td class="py-3 px-4 font-sans">
          <span class="font-medium text-white">${escapeHtml(item.source_device_id)}</span> → <span class="font-medium text-slate-300">${escapeHtml(item.target_device_id)}</span>
        </td>
        <td class="py-3 px-4">
          <div class="text-sky-300">${escapeHtml(item.channel_used)}</div>
          <div class="text-[10px] ${item.resumed ? 'text-amber-400' : 'text-slate-500'}">
            ${item.resumed ? `已续传 ${item.resume_count ?? 1} 次 • 偏移 ${item.resume_offset ?? 0}` : `cache: ${escapeHtml(item.file_cache_id || '-')}`}
          </div>
        </td>
        <td class="py-3 px-4">
          <div class="text-slate-200">${formatBytes(item.bytes_transferred ?? 0)}</div>
          <div class="text-[10px] text-slate-400">b3: ${escapeHtml(shortHash(item.content_hash))}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded ${ok ? 'bg-emerald-500/10 text-emerald-400' : 'bg-rose-500/10 text-rose-400'} text-[11px] font-sans">${escapeHtml(item.verification_status)}</span>
        </td>
      </tr>
    `;
  }

  if (item.record_type === 'EVENT') {
    const targets = item.target_device_ids ?? [];
    return `
      <tr class="hover:bg-slate-900/50">
        <td class="py-3 px-4 text-slate-400">
          <div class="text-slate-200">#${item.id}</div>
          <div class="text-[10px] text-slate-500">${escapeHtml(ts)}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded bg-sky-500/10 text-sky-400 border border-sky-500/20 text-[10px]">audit_event_log</span>
        </td>
        <td class="py-3 px-4 font-sans">
          <span class="font-medium text-white">${escapeHtml(item.source_device_id)}</span> → <span class="text-slate-400">${escapeHtml(targets.join(', ') || '0 台')}</span>
        </td>
        <td class="py-3 px-4">
          <div class="text-slate-300">${escapeHtml(item.topic)}</div>
          <div class="text-[10px] text-slate-500">${escapeHtml(item.event_id)} (${escapeHtml(item.content_category)})</div>
        </td>
        <td class="py-3 px-4">
          <div class="text-slate-200">${formatBytes(item.size_bytes ?? 0)}</div>
          <div class="text-[10px] text-slate-400">b3: ${escapeHtml(shortHash(item.content_hash))}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-[11px] font-sans">DELIVERED</span>
        </td>
      </tr>
    `;
  }

  const ok = item.status === 'OK';
  return `
    <tr class="hover:bg-slate-900/50">
      <td class="py-3 px-4 text-slate-400">
        <div class="text-slate-200">#${item.id}</div>
        <div class="text-[10px] text-slate-500">${escapeHtml(ts)}</div>
      </td>
      <td class="py-3 px-4">
        <span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 text-[10px]">audit_connection_log</span>
      </td>
      <td class="py-3 px-4 font-sans">
        <span class="font-medium text-white">${escapeHtml(item.device_name || item.device_id)}</span>
        <div class="text-[10px] text-slate-500 font-mono">${escapeHtml(item.device_id)}</div>
      </td>
      <td class="py-3 px-4">
        <div class="text-slate-200">${escapeHtml(item.action)}</div>
        <div class="text-[10px] text-slate-400">${escapeHtml(item.lan_endpoint || '-')}</div>
      </td>
      <td class="py-3 px-4 text-slate-400 max-w-xs truncate">${escapeHtml(item.detail || '-')}</td>
      <td class="py-3 px-4">
        <span class="px-2 py-0.5 rounded ${ok ? 'bg-emerald-500/10 text-emerald-400' : 'bg-rose-500/10 text-rose-400'} text-[11px] font-sans">${escapeHtml(item.status)}</span>
      </td>
    </tr>
  `;
}

export function renderAuditView(
  container: HTMLElement,
  data: AuditLogsPagedResponse,
  devices: RegisteredDevice[],
  filter: AuditFilterState,
  onFilterChange: () => void
): void {
  const tabs: Array<{ id: AuditFilterState['logType']; label: string }> = [
    { id: 'ALL', label: `全部审计日志 (${data.total_counts.connection + data.total_counts.event + data.total_counts.transfer})` },
    { id: 'TRANSFER', label: `传输与校验 (${data.total_counts.transfer})` },
    { id: 'EVENT', label: `事件路由 (${data.total_counts.event})` },
    { id: 'CONNECTION', label: `连接准入 (${data.total_counts.connection})` },
  ];

  const tabBtns = tabs
    .map((t) => {
      const active = filter.logType === t.id;
      const cls = active
        ? 'bg-sky-500/20 text-sky-400 border-sky-500/30'
        : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900 border-transparent';
      return `<button data-log-type="${t.id}" class="audit-tab-btn px-3 py-1.5 rounded-lg text-xs font-medium border transition ${cls}">${escapeHtml(t.label)}</button>`;
    })
    .join('');

  const devOptions = [
    `<option value="">全部设备</option>`,
    ...devices.map(
      (d) =>
        `<option value="${escapeHtml(d.device_id)}" ${filter.deviceId === d.device_id ? 'selected' : ''}>${escapeHtml(d.device_name)} (${escapeHtml(d.device_id)})</option>`
    ),
  ].join('');

  const pageStart = data.total === 0 ? 0 : filter.offset + 1;
  const pageEnd = Math.min(data.total, filter.offset + data.items.length);

  container.innerHTML = `
    <div class="space-y-6">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 class="text-lg font-bold text-white">全链路日志审计与取证 (SQLite WAL Audit)</h2>
          <p class="text-xs text-slate-400">记录连接鉴权、事件广播路由、文件流式传输与 BLAKE3 校验回执，支持多维过滤与 JSON 导出</p>
        </div>
        <div class="flex items-center space-x-2">
          <button
            id="audit-export-btn"
            type="button"
            class="px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-800 hover:bg-slate-700 text-slate-200 transition border border-slate-700"
          >导出审计 JSON</button>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2 border-b border-slate-800 pb-3">${tabBtns}</div>

      <div class="flex flex-wrap items-center gap-3 bg-slate-950/60 border border-slate-800 p-3 rounded-xl text-xs">
        <div class="flex items-center space-x-2">
          <span class="text-slate-400">设备:</span>
          <select id="audit-device-select" class="bg-slate-900 border border-slate-800 rounded px-2.5 py-1.5 text-slate-200 focus:outline-none">
            ${devOptions}
          </select>
        </div>

        <div class="flex items-center space-x-2">
          <span class="text-slate-400">校验/状态:</span>
          <select id="audit-status-select" class="bg-slate-900 border border-slate-800 rounded px-2.5 py-1.5 text-slate-200 focus:outline-none">
            <option value="" ${filter.status === '' ? 'selected' : ''}>全部状态</option>
            <option value="VERIFIED_OK" ${filter.status === 'VERIFIED_OK' ? 'selected' : ''}>正常 / VERIFIED_OK</option>
            <option value="RESUMED" ${filter.status === 'RESUMED' ? 'selected' : ''}>断点续传 (Resumed)</option>
            <option value="FAILED" ${filter.status === 'FAILED' ? 'selected' : ''}>异常 / 鉴权或校验失败</option>
          </select>
        </div>

        <div class="flex items-center space-x-2 flex-1 min-w-[200px] max-w-sm">
          <input
            type="text"
            id="audit-keyword-input"
            value="${escapeHtml(filter.keyword)}"
            placeholder="搜索 event_id / file_cache_id / hash / IP..."
            class="w-full bg-slate-900 border border-slate-800 rounded px-2.5 py-1.5 text-slate-200 placeholder-slate-500 focus:outline-none focus:border-sky-500"
          />
          <button id="audit-search-btn" class="px-3 py-1.5 rounded bg-sky-500 hover:bg-sky-400 text-white font-medium shrink-0">检索</button>
        </div>

        <div class="ml-auto flex items-center space-x-3 text-slate-500 font-mono">
          <span>SQLite WAL 查询: ${data.query_ms}ms</span>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">序号 / 时间戳</th>
              <th class="py-3 px-4">审计表类型</th>
              <th class="py-3 px-4">源设备 / 目标</th>
              <th class="py-3 px-4">通道 / 行为 / 主题</th>
              <th class="py-3 px-4">BLAKE3 摘要 / 详情</th>
              <th class="py-3 px-4">状态回执</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800/80 font-mono">
            ${
              data.items.length === 0
                ? '<tr><td colspan="6" class="py-10 text-center text-slate-500 font-sans">当前筛选条件下无匹配审计记录</td></tr>'
                : data.items.map(renderAuditTableRow).join('')
            }
          </tbody>
        </table>
      </div>

      <div class="flex items-center justify-between text-xs text-slate-400">
        <span class="font-mono">显示 ${pageStart} - ${pageEnd} / 共 ${data.total} 条</span>
        <div class="flex items-center space-x-2">
          <button id="audit-prev-btn" ${filter.offset <= 0 ? 'disabled' : ''} class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 disabled:opacity-40">上一页</button>
          <button id="audit-next-btn" ${pageEnd >= data.total ? 'disabled' : ''} class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 disabled:opacity-40">下一页</button>
        </div>
      </div>
    </div>
  `;

  container.querySelectorAll<HTMLButtonElement>('.audit-tab-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      filter.logType = (btn.dataset['logType'] as AuditFilterState['logType']) ?? 'ALL';
      filter.offset = 0;
      onFilterChange();
    });
  });

  const devSel = container.querySelector('#audit-device-select') as HTMLSelectElement;
  devSel.addEventListener('change', () => {
    filter.deviceId = devSel.value;
    filter.offset = 0;
    onFilterChange();
  });

  const statusSel = container.querySelector('#audit-status-select') as HTMLSelectElement;
  statusSel.addEventListener('change', () => {
    filter.status = statusSel.value;
    filter.offset = 0;
    onFilterChange();
  });

  const kwInput = container.querySelector('#audit-keyword-input') as HTMLInputElement;
  const triggerKeywordSearch = () => {
    filter.keyword = kwInput.value.trim();
    filter.offset = 0;
    onFilterChange();
  };
  container.querySelector('#audit-search-btn')?.addEventListener('click', triggerKeywordSearch);
  kwInput.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter') triggerKeywordSearch();
  });

  container.querySelector('#audit-prev-btn')?.addEventListener('click', () => {
    filter.offset = Math.max(0, filter.offset - filter.limit);
    onFilterChange();
  });
  container.querySelector('#audit-next-btn')?.addEventListener('click', () => {
    filter.offset += filter.limit;
    onFilterChange();
  });

  const exportBtn = container.querySelector('#audit-export-btn') as HTMLButtonElement | null;
  exportBtn?.addEventListener('click', async () => {
    exportBtn.disabled = true;
    exportBtn.textContent = '导出中...';
    try {
      const blob = await api.exportAuditLogs({
        log_type: filter.logType,
        device_id: filter.deviceId,
        status: filter.status,
        keyword: filter.keyword,
      });
      const blobUrl = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = blobUrl;
      a.download = 'multinteractor-audit-export.json';
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(blobUrl);
      showToast('已导出审计日志 JSON 文件', 'success');
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '审计日志导出失败', 'error');
    } finally {
      exportBtn.disabled = false;
      exportBtn.textContent = '导出审计 JSON';
    }
  });
}
