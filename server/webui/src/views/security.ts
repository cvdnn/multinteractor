import { api, ApiError } from '../api';
import { openIssueTokenModal, openRotateTokenModal } from '../components/modals';
import { showToast } from '../components/toast';
import type { SecurityTokensResponse, SecurityWarning } from '../types';
import { copyToClipboard, escapeHtml, formatTimestampMs } from '../utils';

export function renderSecurityView(
  container: HTMLElement,
  data: SecurityTokensResponse,
  warnings: SecurityWarning[],
  adminTokenIsSeparate: boolean,
  revealed: boolean,
  onToggleReveal: (revealNext: boolean) => void,
  onRefresh: () => void
): void {
  const tokenText = revealed && data.cluster_token.plaintext ? data.cluster_token.plaintext : data.cluster_token.masked;

  const warningCards = warnings
    .map(
      (w) => `
      <div class="rounded-xl border ${w.severity === 'HIGH' ? 'bg-rose-500/10 border-rose-500/35 text-rose-200' : 'bg-amber-500/10 border-amber-500/35 text-amber-200'} px-4 py-3 text-xs">
        <div class="font-semibold flex items-center space-x-2">
          <span class="px-1.5 py-0.5 rounded font-mono text-[10px] ${w.severity === 'HIGH' ? 'bg-rose-500/25 text-rose-300' : 'bg-amber-500/25 text-amber-300'}">${escapeHtml(w.severity)}</span>
          <span>${escapeHtml(w.title)}</span>
        </div>
        <div class="mt-1 opacity-85">${escapeHtml(w.message)}</div>
      </div>
    `
    )
    .join('');

  const tokenRows =
    data.device_tokens.length === 0
      ? `<tr><td colspan="5" class="py-8 text-center text-slate-500 font-sans text-xs">尚未签发设备专属 Token</td></tr>`
      : data.device_tokens
          .map(
            (t) => `
          <tr class="hover:bg-slate-900/50">
            <td class="py-3 px-4">
              <div class="text-sky-300 font-mono">${escapeHtml(t.token_prefix)}</div>
              <div class="text-[10px] text-slate-500 font-mono">${escapeHtml(t.token_id)}</div>
            </td>
            <td class="py-3 px-4 text-slate-400 font-mono">${escapeHtml(formatTimestampMs(t.issued_at_ms))}</td>
            <td class="py-3 px-4 font-sans">
              <div class="text-white">${escapeHtml(t.label || '未命名设备')}</div>
              ${t.expected_os ? `<div class="text-[10px] text-slate-500 font-mono">${escapeHtml(t.expected_os)}</div>` : ''}
            </td>
            <td class="py-3 px-4">
              ${
                t.revoked
                  ? `<span class="px-2 py-0.5 rounded bg-rose-500/10 text-rose-400 text-[10px]">已吊销 (${escapeHtml(formatTimestampMs(t.revoked_at_ms ?? 0))})</span>`
                  : `<span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-[10px]">有效</span>`
              }
            </td>
            <td class="py-3 px-4 text-right">
              ${
                t.revoked
                  ? '<span class="text-slate-600 text-xs">已入黑名单</span>'
                  : `<button data-token-id="${escapeHtml(t.token_id)}" class="sec-revoke-btn text-rose-400 hover:text-rose-300 text-xs">吊销</button>`
              }
            </td>
          </tr>
        `
          )
          .join('');

  container.innerHTML = `
    <div class="space-y-6">
      <div>
        <h2 class="text-lg font-bold text-white">安全中心与访问凭据管控 (Security & Credentials)</h2>
        <p class="text-xs text-slate-400">管理双平面密钥隔离（MI_ADMIN_TOKEN vs MI_CLUSTER_TOKEN）、主集群 PSK 轮转与设备专属 Token 吊销黑名单</p>
      </div>

      ${warningCards}

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-4">
        <div class="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
          <div>
            <div class="flex items-center space-x-2">
              <h3 class="text-sm font-semibold text-white">主集群预共享密钥 (MI_CLUSTER_TOKEN)</h3>
              <span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-xs font-mono border border-emerald-500/20">Active</span>
              <span class="px-2 py-0.5 rounded ${adminTokenIsSeparate ? 'bg-sky-500/10 text-sky-400 border-sky-500/20' : 'bg-amber-500/10 text-amber-300 border-amber-500/20'} text-xs font-mono border">
                ${adminTokenIsSeparate ? 'MI_ADMIN_TOKEN 已隔离' : '未配置 MI_ADMIN_TOKEN'}
              </span>
            </div>
            <p class="text-xs text-slate-400 mt-1">仅供客户端守护进程 (mi-daemon) 接入 WebSocket 信令网关与数据缓存通道使用；轮转此密钥不会注销管理控制台会话</p>
          </div>
          <button id="sec-rotate-btn" class="px-3 py-1.5 rounded-lg text-xs font-bold bg-amber-500 hover:bg-amber-400 text-slate-950 transition shrink-0">
            立即轮转集群密钥
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-3 bg-slate-900 border border-slate-800 rounded-lg p-3">
          <span class="text-xs text-slate-500 font-mono">集群密钥:</span>
          <span class="font-mono text-sm text-slate-200 break-all">${escapeHtml(tokenText)}</span>
          <div class="ml-auto flex items-center space-x-3">
            <button id="sec-toggle-reveal" class="text-xs text-sky-400 hover:text-sky-300">${revealed ? '隐藏明文' : '显示明文'}</button>
            <button id="sec-copy-cluster" class="text-xs text-slate-400 hover:text-slate-200">复制</button>
          </div>
        </div>

        <div class="text-[11px] text-slate-400 flex items-center space-x-2">
          <svg class="w-4 h-4 text-emerald-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" /></svg>
          <span>恒定时间验证守卫：${escapeHtml(data.constant_time_guard)} • SQLite WAL 凭据库仅存储设备 Token SHA-256 摘要</span>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <div class="p-4 border-b border-slate-800 flex items-center justify-between">
          <h3 class="text-sm font-semibold text-white">设备专属 Access Token 记录与黑名单</h3>
          <button id="sec-issue-btn" class="text-xs text-sky-400 hover:underline font-medium">+ 签发设备专属 Token</button>
        </div>
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">Token 摘要 / ID</th>
              <th class="py-3 px-4">签发时间</th>
              <th class="py-3 px-4">授权设备绑定</th>
              <th class="py-3 px-4">状态</th>
              <th class="py-3 px-4 text-right">操作</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">${tokenRows}</tbody>
        </table>
      </div>
    </div>
  `;

  container.querySelector('#sec-rotate-btn')?.addEventListener('click', () => {
    openRotateTokenModal(() => onRefresh());
  });
  container.querySelector('#sec-issue-btn')?.addEventListener('click', () => {
    openIssueTokenModal(() => onRefresh());
  });
  container.querySelector('#sec-toggle-reveal')?.addEventListener('click', () => {
    onToggleReveal(!revealed);
  });
  container.querySelector('#sec-copy-cluster')?.addEventListener('click', async () => {
    try {
      const full = revealed && data.cluster_token.plaintext
        ? data.cluster_token.plaintext
        : (await api.getSecurityTokens(true)).cluster_token.plaintext ?? '';
      const ok = await copyToClipboard(full);
      showToast(ok ? '已复制当前集群 MI_CLUSTER_TOKEN' : '复制失败', ok ? 'success' : 'warn');
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '读取密钥失败', 'error');
    }
  });
  container.querySelectorAll<HTMLButtonElement>('.sec-revoke-btn').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const tokenId = btn.dataset['tokenId'];
      if (!tokenId) return;
      try {
        await api.revokeToken(tokenId);
        showToast(`已吊销设备 Token ${tokenId}`, 'warn');
        onRefresh();
      } catch (err) {
        showToast(err instanceof ApiError ? err.message : '吊销失败', 'error');
      }
    });
  });
}
