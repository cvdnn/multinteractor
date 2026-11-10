import { api, ApiError } from '../api';
import { openDeviceAclDrawer, openIssueTokenModal } from '../components/modals';
import { showToast } from '../components/toast';
import type { RegisteredDevice } from '../types';
import { escapeHtml, formatRelativeMs, formatTimestampMs, osIconSvg } from '../utils';

export function renderDevicesView(
  container: HTMLElement,
  devices: RegisteredDevice[],
  state: { searchQuery: string; onlineOnly: boolean },
  onRefresh: () => void
): void {
  const q = state.searchQuery.toLowerCase();
  const filtered = devices.filter((d) => {
    if (state.onlineOnly && !d.online) return false;
    if (!q) return true;
    return (
      d.device_id.toLowerCase().includes(q) ||
      d.device_name.toLowerCase().includes(q) ||
      d.os.toLowerCase().includes(q) ||
      (d.lan_endpoint ?? '').toLowerCase().includes(q)
    );
  });

  const rowsHtml =
    filtered.length === 0
      ? `<tr><td colspan="6" class="py-10 text-center text-slate-500 text-xs">未匹配到任何已注册设备</td></tr>`
      : filtered
          .map((d) => {
            const avatarBg = !d.enabled
              ? 'bg-rose-500/20 text-rose-400'
              : d.online
                ? 'bg-sky-500/20 text-sky-400'
                : 'bg-slate-800 text-slate-400';
            const initial = (d.device_name || d.device_id || '?').slice(0, 1).toUpperCase();
            const aclLabel = d.acl_restricted
              ? `ACL 限制 (${d.allowed_categories.join(',')})`
              : `Sub: ${d.subscribed_topics.length} 主题 • 全类型允许`;

            const statusDot = !d.enabled
              ? '<span class="w-2 h-2 rounded-full bg-rose-500"></span><span class="text-rose-400 font-medium">已禁用</span>'
              : d.online
                ? `<span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span><span class="text-emerald-400 font-medium">在线 (${escapeHtml(formatRelativeMs(d.last_seen_ms))})</span>`
                : `<span class="w-2 h-2 rounded-full bg-slate-600"></span><span class="text-slate-400">离线 (${escapeHtml(formatRelativeMs(d.last_seen_ms))})</span>`;

            const enableBadge = d.enabled
              ? '<span class="px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 text-[11px]">准入已启用</span>'
              : '<span class="px-2 py-0.5 rounded-full bg-rose-500/10 text-rose-400 border border-rose-500/20 text-[11px]">已禁止接入</span>';

            return `
              <tr class="hover:bg-slate-900/50 transition ${!d.online ? 'opacity-80' : ''}">
                <td class="py-3.5 px-4">
                  <div class="flex items-center space-x-2.5">
                    <div class="w-7 h-7 rounded-lg ${avatarBg} flex items-center justify-center font-bold text-xs shrink-0">${escapeHtml(initial)}</div>
                    <div>
                      <div class="font-semibold text-white flex items-center space-x-1.5">
                        <span>${escapeHtml(d.device_name)}</span>
                        <span class="text-[10px] px-1.5 py-0.5 rounded bg-slate-800 text-slate-400 font-mono">${escapeHtml(d.device_id)}</span>
                      </div>
                      <div class="text-[11px] ${d.acl_restricted ? 'text-amber-300' : 'text-slate-400'} font-mono mt-0.5">${escapeHtml(aclLabel)}</div>
                    </div>
                  </div>
                </td>
                <td class="py-3.5 px-4 font-mono">
                  <span class="inline-flex items-center space-x-1.5 text-slate-300">${osIconSvg(d.os)}<span>${escapeHtml(d.os)}</span></span>
                </td>
                <td class="py-3.5 px-4 font-mono text-slate-300">
                  ${
                    d.lan_endpoint
                      ? `<span class="px-2 py-1 rounded bg-slate-900 border border-slate-800 text-sky-300">${escapeHtml(d.lan_endpoint)}</span>`
                      : '<span class="text-slate-500">未上报</span>'
                  }
                </td>
                <td class="py-3.5 px-4">
                  <div class="flex items-center space-x-1.5">${statusDot}</div>
                  <div class="text-[10px] text-slate-500 font-mono mt-0.5">首次: ${escapeHtml(formatTimestampMs(d.first_seen_ms))}</div>
                </td>
                <td class="py-3.5 px-4">${enableBadge}</td>
                <td class="py-3.5 px-4 text-right space-x-2 whitespace-nowrap">
                  <button data-action="acl" data-id="${escapeHtml(d.device_id)}" class="dev-act text-sky-400 hover:text-sky-300 text-xs font-medium">配置 ACL</button>
                  ${
                    d.online
                      ? `<button data-action="kick" data-id="${escapeHtml(d.device_id)}" class="dev-act text-amber-400 hover:text-amber-300 text-xs font-medium">踢出</button>`
                      : ''
                  }
                  ${
                    d.enabled
                      ? `<button data-action="disable" data-id="${escapeHtml(d.device_id)}" class="dev-act text-rose-400 hover:text-rose-300 text-xs font-medium">禁用</button>`
                      : `<button data-action="enable" data-id="${escapeHtml(d.device_id)}" class="dev-act text-emerald-400 hover:text-emerald-300 text-xs font-medium">启用</button>`
                  }
                  <button data-action="remove" data-id="${escapeHtml(d.device_id)}" class="dev-act text-slate-500 hover:text-rose-400 text-xs font-medium">注销</button>
                </td>
              </tr>
            `;
          })
          .join('');

  container.innerHTML = `
    <div class="space-y-6">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 class="text-lg font-bold text-white">已注册设备台账 (Device Registry)</h2>
          <p class="text-xs text-slate-400">SQLite 持久化设备台账、内网通信端点映射、在线连接踢出与内容分类 ACL 控制</p>
        </div>
        <div class="flex flex-wrap items-center gap-3">
          <label class="inline-flex items-center space-x-1.5 text-xs text-slate-300 cursor-pointer">
            <input type="checkbox" id="devices-online-only" ${state.onlineOnly ? 'checked' : ''} class="rounded bg-slate-900 border-slate-700 text-sky-500" />
            <span>仅显示在线</span>
          </label>
          <input type="text" id="devices-search-input" value="${escapeHtml(state.searchQuery)}" placeholder="搜索设备名称 / ID / IP..." class="bg-slate-950 border border-slate-800 rounded-lg px-3 py-1.5 text-xs text-slate-200 placeholder-slate-500 focus:outline-none focus:border-sky-500 w-52" />
          <button id="devices-issue-btn" class="px-3 py-1.5 rounded-lg text-xs font-medium bg-sky-500 hover:bg-sky-400 text-white transition flex items-center space-x-1.5 shadow-sm">
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" /></svg>
            <span>准入新设备 (签发 Token)</span>
          </button>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">设备信息 (ID / Name)</th>
              <th class="py-3 px-4">操作系统</th>
              <th class="py-3 px-4">内网通信端点 (LAN)</th>
              <th class="py-3 px-4">在线状态 / 首次上线</th>
              <th class="py-3 px-4">准入状态</th>
              <th class="py-3 px-4 text-right">管理操作</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-sans">${rowsHtml}</tbody>
        </table>
      </div>
    </div>
  `;

  const searchInput = container.querySelector('#devices-search-input') as HTMLInputElement;
  searchInput.addEventListener('input', () => {
    state.searchQuery = searchInput.value;
    renderDevicesView(container, devices, state, onRefresh);
    const nextInput = container.querySelector('#devices-search-input') as HTMLInputElement | null;
    if (nextInput) {
      nextInput.focus();
      nextInput.setSelectionRange(nextInput.value.length, nextInput.value.length);
    }
  });

  const onlineChk = container.querySelector('#devices-online-only') as HTMLInputElement;
  onlineChk.addEventListener('change', () => {
    state.onlineOnly = onlineChk.checked;
    renderDevicesView(container, devices, state, onRefresh);
  });

  container.querySelector('#devices-issue-btn')?.addEventListener('click', () => {
    openIssueTokenModal(() => onRefresh());
  });

  container.querySelectorAll<HTMLButtonElement>('.dev-act').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const action = btn.dataset['action'];
      const id = btn.dataset['id'];
      if (!action || !id) return;
      const dev = devices.find((d) => d.device_id === id);
      if (!dev) return;

      if (action === 'acl') {
        openDeviceAclDrawer(dev, () => onRefresh());
        return;
      }

      try {
        if (action === 'kick') {
          await api.kickDevice(id);
          showToast(`已踢出设备 ${dev.device_name} 的当前连接`, 'info');
        } else if (action === 'disable') {
          await api.disableDevice(id);
          showToast(`已禁用设备 ${dev.device_name}`, 'warn');
        } else if (action === 'enable') {
          await api.enableDevice(id);
          showToast(`已恢复启用设备 ${dev.device_name}`, 'success');
        } else if (action === 'remove') {
          if (!window.confirm(`确认从 SQLite 台账中彻底注销设备「${dev.device_name} (${id})」？`)) {
            return;
          }
          await api.removeDevice(id);
          showToast(`已彻底移除设备 ${dev.device_name}`, 'warn');
        }
        onRefresh();
      } catch (err) {
        showToast(err instanceof ApiError ? err.message : '设备操作失败', 'error');
      }
    });
  });
}
