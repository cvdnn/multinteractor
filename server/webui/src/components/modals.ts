import { api, ApiError } from '../api';
import type { CacheSessionDetail, ContentCategory, IssuedTokenResponse, RegisteredDevice } from '../types';
import { copyToClipboard, escapeHtml, formatBytes, formatTtlCountdown } from '../utils';
import { showToast } from './toast';

export function openRotateTokenModal(onSuccess: () => void): void {
  const existing = document.getElementById('mi-modal-overlay');
  existing?.remove();

  const overlay = document.createElement('div');
  overlay.id = 'mi-modal-overlay';
  overlay.className = 'fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 flex items-center justify-center p-4';
  overlay.innerHTML = `
    <div class="bg-slate-900 border border-slate-800 rounded-xl max-w-md w-full p-5 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <h3 class="text-sm font-bold text-white flex items-center space-x-2">
          <svg class="w-4 h-4 text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <span>轮转主集群预共享密钥 (MI_CLUSTER_TOKEN)</span>
        </h3>
        <button id="modal-close-x" class="text-slate-400 hover:text-slate-200 text-sm px-1">✕</button>
      </div>

      <div class="text-xs text-slate-300 leading-relaxed space-y-2">
        <p><strong class="text-amber-300">安全影响：</strong>轮转将会立即吊销全部旧数据面 Token（含历史签发的设备专属 Token）。所有客户端 <code class="font-mono text-sky-300">mi-daemon</code> 重连时需更新新 Token。</p>
        <p class="text-slate-400">当前 WebUI 管理员会话受独立 Session 保护，轮转不会使本管理后台掉线。</p>
      </div>

      <div class="space-y-1.5">
        <label class="text-xs text-slate-400 block">新集群密钥（留空将由服务端密码学随机生成）:</label>
        <input type="text" id="rotate-new-token-input" placeholder="留空自动生成高强随机密钥" class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-xs font-mono text-white placeholder-slate-600 focus:outline-none focus:border-amber-500" />
      </div>

      <div id="rotate-result-box" class="hidden rounded-lg bg-emerald-500/10 border border-emerald-500/30 p-3 space-y-2 text-xs">
        <div class="text-emerald-300 font-semibold">集群密钥轮转成功，新密钥已生效：</div>
        <div class="flex items-center space-x-2 bg-slate-950 rounded px-2.5 py-1.5 border border-slate-800">
          <code id="rotate-result-token" class="font-mono text-sky-300 break-all flex-1"></code>
          <button id="rotate-copy-btn" class="text-sky-400 hover:text-sky-300 shrink-0 font-medium">复制</button>
        </div>
      </div>

      <div class="flex items-center justify-end space-x-2 pt-2 border-t border-slate-800">
        <button id="modal-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200">关闭</button>
        <button id="rotate-submit-btn" class="px-3 py-1.5 rounded-lg text-xs font-semibold bg-amber-500 hover:bg-amber-400 text-slate-950">确认轮转并生效</button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);
  const close = () => overlay.remove();
  overlay.querySelector('#modal-close-x')?.addEventListener('click', close);
  overlay.querySelector('#modal-cancel-btn')?.addEventListener('click', close);

  const submitBtn = overlay.querySelector('#rotate-submit-btn') as HTMLButtonElement;
  const input = overlay.querySelector('#rotate-new-token-input') as HTMLInputElement;
  const resultBox = overlay.querySelector('#rotate-result-box') as HTMLDivElement;
  const resultTokenEl = overlay.querySelector('#rotate-result-token') as HTMLElement;
  const copyBtn = overlay.querySelector('#rotate-copy-btn') as HTMLButtonElement;

  copyBtn.addEventListener('click', async () => {
    const ok = await copyToClipboard(resultTokenEl.textContent || '');
    showToast(ok ? '新集群 Token 已复制到剪贴板' : '复制失败，请手动选中复制', ok ? 'success' : 'warn');
  });

  submitBtn.addEventListener('click', async () => {
    submitBtn.disabled = true;
    submitBtn.textContent = '正在轮转...';
    try {
      const res = await api.rotateClusterToken(input.value.trim() || undefined);
      resultTokenEl.textContent = res.token;
      resultBox.classList.remove('hidden');
      submitBtn.classList.add('hidden');
      showToast('主集群密钥轮转成功！所有旧客户端 Token 已吊销。', 'warn');
      onSuccess();
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '密钥轮转失败', 'error');
      submitBtn.disabled = false;
      submitBtn.textContent = '确认轮转并生效';
    }
  });
}

export function openIssueTokenModal(onIssued: (issued: IssuedTokenResponse) => void): void {
  const existing = document.getElementById('mi-modal-overlay');
  existing?.remove();

  const overlay = document.createElement('div');
  overlay.id = 'mi-modal-overlay';
  overlay.className = 'fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 flex items-center justify-center p-4';
  overlay.innerHTML = `
    <div class="bg-slate-900 border border-slate-800 rounded-xl max-w-md w-full p-5 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <h3 class="text-sm font-bold text-white">签发设备专属 Access Token</h3>
        <button id="modal-close-x" class="text-slate-400 hover:text-slate-200 text-sm px-1">✕</button>
      </div>

      <div class="space-y-3 text-xs">
        <div>
          <label class="text-slate-400 block mb-1">设备别名 / 用途说明:</label>
          <input type="text" id="issue-label-input" placeholder="例如：Handy-MacStudio" class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-sky-500" />
        </div>
        <div>
          <label class="text-slate-400 block mb-1">预期操作系统:</label>
          <select id="issue-os-select" class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none">
            <option value="windows">Windows</option>
            <option value="macos">macOS</option>
            <option value="linux">Linux</option>
          </select>
        </div>
      </div>

      <div id="issued-secret-box" class="hidden rounded-lg bg-emerald-500/10 border border-emerald-500/30 p-3 space-y-2 text-xs">
        <div class="text-emerald-300 font-semibold">专属 Token 已签发（明文仅展示这一次，请立即复制保存）：</div>
        <div class="flex items-center space-x-2 bg-slate-950 rounded px-2.5 py-2 border border-slate-800">
          <code id="issued-secret-value" class="font-mono text-sky-300 break-all flex-1"></code>
          <button id="issued-copy-btn" class="text-sky-400 hover:text-sky-300 shrink-0 font-medium">复制</button>
        </div>
      </div>

      <div class="flex items-center justify-end space-x-2 pt-2 border-t border-slate-800">
        <button id="modal-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200">关闭</button>
        <button id="issue-submit-btn" class="px-3 py-1.5 rounded-lg text-xs font-semibold bg-sky-500 hover:bg-sky-400 text-white">立即签发</button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);
  const close = () => overlay.remove();
  overlay.querySelector('#modal-close-x')?.addEventListener('click', close);
  overlay.querySelector('#modal-cancel-btn')?.addEventListener('click', close);

  const labelInput = overlay.querySelector('#issue-label-input') as HTMLInputElement;
  const osSelect = overlay.querySelector('#issue-os-select') as HTMLSelectElement;
  const submitBtn = overlay.querySelector('#issue-submit-btn') as HTMLButtonElement;
  const secretBox = overlay.querySelector('#issued-secret-box') as HTMLDivElement;
  const secretValEl = overlay.querySelector('#issued-secret-value') as HTMLElement;
  const copyBtn = overlay.querySelector('#issued-copy-btn') as HTMLButtonElement;

  copyBtn.addEventListener('click', async () => {
    const ok = await copyToClipboard(secretValEl.textContent || '');
    showToast(ok ? '设备 Token 已复制' : '复制失败', ok ? 'success' : 'warn');
  });

  submitBtn.addEventListener('click', async () => {
    submitBtn.disabled = true;
    submitBtn.textContent = '签发中...';
    try {
      const issued = await api.issueToken(labelInput.value.trim(), osSelect.value);
      secretValEl.textContent = issued.token;
      secretBox.classList.remove('hidden');
      submitBtn.classList.add('hidden');
      showToast(`已签发专属 Token (${issued.token_prefix})`, 'success');
      onIssued(issued);
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : '签发失败', 'error');
      submitBtn.disabled = false;
      submitBtn.textContent = '立即签发';
    }
  });
}

export function openDeviceAclDrawer(
  device: RegisteredDevice,
  onSaved: (updated: RegisteredDevice) => void
): void {
  const existing = document.getElementById('mi-drawer-overlay');
  existing?.remove();

  const cats = new Set<ContentCategory>(device.allowed_categories);

  const overlay = document.createElement('div');
  overlay.id = 'mi-drawer-overlay';
  overlay.className = 'fixed inset-0 bg-slate-950/60 backdrop-blur-[1px] z-50 flex justify-end';
  overlay.innerHTML = `
    <div class="w-96 max-w-full bg-slate-950 border-l border-slate-800 shadow-2xl h-full p-6 flex flex-col justify-between">
      <div class="overflow-y-auto pr-1">
        <div class="flex items-center justify-between border-b border-slate-800 pb-3">
          <h3 class="font-bold text-white text-sm">设备配置与 ACL: ${escapeHtml(device.device_name)}</h3>
          <button id="drawer-close-x" class="text-slate-400 hover:text-slate-200 px-1">✕</button>
        </div>

        <div class="mt-4 space-y-4 text-xs">
          <div class="grid grid-cols-2 gap-3">
            <div>
              <span class="text-slate-500">设备 ID:</span>
              <div class="font-mono text-slate-200 font-semibold mt-0.5 break-all">${escapeHtml(device.device_id)}</div>
            </div>
            <div>
              <span class="text-slate-500">操作系统:</span>
              <div class="font-mono text-slate-300 mt-0.5">${escapeHtml(device.os)}</div>
            </div>
          </div>

          <div>
            <span class="text-slate-500">内网即用即连端点 (Client Reported):</span>
            <div class="font-mono text-sky-400 mt-0.5">${escapeHtml(device.lan_endpoint || '未上报')}</div>
          </div>

          <div class="pt-3 border-t border-slate-800/80">
            <span class="text-slate-300 font-semibold block mb-2">已订阅 Topic 权限列表 (防跨机越权):</span>
            <div class="space-y-1.5 font-mono text-[11px] text-slate-400 bg-slate-900/70 rounded-lg p-2.5 border border-slate-800">
              ${device.subscribed_topics
                .map(
                  (t) => `<div class="flex items-center space-x-2">
                    <span class="w-1.5 h-1.5 rounded-full bg-sky-400 shrink-0"></span>
                    <span class="break-all">${escapeHtml(t)}</span>
                  </div>`
                )
                .join('')}
            </div>
          </div>

          <div class="pt-3 border-t border-slate-800/80">
            <span class="text-slate-300 font-semibold block mb-1">内容分类白名单过滤 (allowed_categories):</span>
            <p class="text-[11px] text-slate-500 mb-2.5">取消勾选将在 Cloud Hub 路由投递阶段拦截对应类型事件，防止敏感文件进入受限设备。</p>
            <div class="space-y-2.5 text-slate-200">
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="TEXT" ${cats.has('TEXT') ? 'checked' : ''} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>TEXT (剪贴板文本)</span>
              </label>
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="IMAGE" ${cats.has('IMAGE') ? 'checked' : ''} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>IMAGE (剪贴板图片)</span>
              </label>
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="FILE" ${cats.has('FILE') ? 'checked' : ''} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>FILE (单个大文件传输)</span>
              </label>
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="DIRECTORY" ${cats.has('DIRECTORY') ? 'checked' : ''} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>DIRECTORY (文件夹 .tar.zst)</span>
              </label>
            </div>
          </div>
        </div>
      </div>

      <div class="pt-4 border-t border-slate-800 flex items-center justify-end space-x-2">
        <button id="drawer-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200">关闭</button>
        <button id="drawer-save-btn" class="px-3.5 py-1.5 rounded-lg text-xs font-semibold bg-sky-500 hover:bg-sky-400 text-white">保存 ACL 策略</button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);
  const close = () => overlay.remove();
  overlay.addEventListener('click', (ev) => {
    if (ev.target === overlay) close();
  });
  overlay.querySelector('#drawer-close-x')?.addEventListener('click', close);
  overlay.querySelector('#drawer-cancel-btn')?.addEventListener('click', close);

  const saveBtn = overlay.querySelector('#drawer-save-btn') as HTMLButtonElement;
  saveBtn.addEventListener('click', async () => {
    const selected: ContentCategory[] = [];
    overlay.querySelectorAll<HTMLInputElement>('.acl-cat-checkbox').forEach((el) => {
      if (el.checked && el.dataset['cat']) {
        selected.push(el.dataset['cat'] as ContentCategory);
      }
    });
    saveBtn.disabled = true;
    saveBtn.textContent = '保存中...';
    try {
      const res = await api.updateDeviceAcl(device.device_id, selected);
      showToast(`设备 ${device.device_name} 的 ACL 白名单已更新`, 'success');
      close();
      onSaved(res.device);
    } catch (err) {
      showToast(err instanceof ApiError ? err.message : 'ACL 保存失败', 'error');
      saveBtn.disabled = false;
      saveBtn.textContent = '保存 ACL 策略';
    }
  });
}

export async function openCacheDetailModal(fileCacheId: string): Promise<void> {
  const existing = document.getElementById('mi-modal-overlay');
  existing?.remove();

  let detail: CacheSessionDetail;
  try {
    detail = await api.getCacheSessionDetail(fileCacheId);
  } catch (err) {
    showToast(err instanceof ApiError ? err.message : '获取缓存分块详情失败', 'error');
    return;
  }

  const overlay = document.createElement('div');
  overlay.id = 'mi-modal-overlay';
  overlay.className = 'fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 flex items-center justify-center p-4';
  overlay.innerHTML = `
    <div class="bg-slate-900 border border-slate-800 rounded-xl max-w-2xl w-full p-5 shadow-2xl space-y-4 max-h-[85vh] flex flex-col">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3 shrink-0">
        <div>
          <h3 class="text-sm font-bold text-white font-mono">缓存分块清单与摘要追溯: ${escapeHtml(detail.file_cache_id)}</h3>
          <p class="text-xs text-slate-400 mt-0.5">${escapeHtml(detail.file_name)} • ${formatBytes(detail.total_size)} • 24h TTL 剩余 ${formatTtlCountdown(detail.ttl_remaining_seconds)}</p>
        </div>
        <button id="modal-close-x" class="text-slate-400 hover:text-slate-200 text-sm px-1">✕</button>
      </div>

      <div class="space-y-3 text-xs overflow-y-auto pr-1">
        <div class="bg-slate-950 border border-slate-800 rounded-lg p-3 space-y-1.5 font-mono">
          <div class="text-slate-400">全文 BLAKE3 摘要 (${escapeHtml(detail.digest.algorithm)}):</div>
          <div class="text-emerald-400 break-all">${escapeHtml(detail.digest.full_hash)}</div>
          <div class="text-slate-500 pt-1">分块尺寸: ${formatBytes(detail.digest.chunk_size_bytes)} • 分块数: ${detail.digest.chunk_hashes.length}</div>
        </div>

        <div class="space-y-1.5">
          <div class="text-slate-400 font-semibold">4MB 固定分块 BLAKE3 清单 (${detail.verified_chunks} / ${detail.total_chunks} 已验):</div>
          <div class="bg-slate-950 border border-slate-800 rounded-lg divide-y divide-slate-800/70 max-h-60 overflow-y-auto font-mono text-[11px]">
            ${
              detail.digest.chunk_hashes.length === 0
                ? '<div class="p-3 text-slate-500">空文件 (0 分块)</div>'
                : detail.digest.chunk_hashes
                    .map(
                      (h, idx) => `<div class="px-3 py-1.5 flex items-center justify-between gap-3">
                        <span class="text-slate-400 shrink-0">#${String(idx).padStart(4, '0')}</span>
                        <span class="text-slate-200 break-all">${escapeHtml(h)}</span>
                        <span class="shrink-0 ${idx < detail.verified_chunks ? 'text-emerald-400' : 'text-slate-500'}">
                          ${idx < detail.verified_chunks ? 'VERIFIED' : 'PENDING'}
                        </span>
                      </div>`
                    )
                    .join('')
            }
          </div>
        </div>
      </div>

      <div class="flex justify-end pt-2 border-t border-slate-800 shrink-0">
        <button id="modal-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs bg-slate-800 hover:bg-slate-700 text-slate-200">关闭</button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);
  const close = () => overlay.remove();
  overlay.addEventListener('click', (ev) => {
    if (ev.target === overlay) close();
  });
  overlay.querySelector('#modal-close-x')?.addEventListener('click', close);
  overlay.querySelector('#modal-cancel-btn')?.addEventListener('click', close);
}
