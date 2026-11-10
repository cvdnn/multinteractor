import { api, ApiError } from '../api';
import type { AuthSessionResponse } from '../types';
import { escapeHtml } from '../utils';

export function renderAuthGate(
  root: HTMLElement,
  sessionInfo: AuthSessionResponse,
  onAuthenticated: (res: AuthSessionResponse) => void
): void {
  const modeNote = sessionInfo.admin_token_is_separate
    ? '控制面已启用独立 MI_ADMIN_TOKEN 认证（数据面 MI_CLUSTER_TOKEN 不可用于登录管理台）。'
    : '服务端未单独配置 MI_ADMIN_TOKEN，当前临时以 MI_CLUSTER_TOKEN 作为管理员登录凭据；建议生产部署时配置 MI_ADMIN_TOKEN。';

  const modeBadgeClass = sessionInfo.admin_token_is_separate
    ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/30'
    : 'bg-amber-500/10 text-amber-300 border-amber-500/30';

  root.innerHTML = `
    <div class="min-h-screen flex items-center justify-center bg-slate-950 px-4 py-12">
      <div class="w-full max-w-md bg-slate-900 border border-slate-800 rounded-2xl p-7 shadow-2xl space-y-6">
        <div class="flex items-center space-x-3">
          <div class="w-10 h-10 rounded-xl bg-gradient-to-tr from-sky-500 to-indigo-600 flex items-center justify-center shadow-lg shadow-sky-500/25">
            <svg class="w-5 h-5 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
          </div>
          <div>
            <div class="flex items-center space-x-2">
              <h1 class="text-lg font-bold text-white tracking-wide">multinteractor</h1>
              <span class="text-xs px-2 py-0.5 rounded-full bg-sky-500/10 text-sky-400 font-mono border border-sky-500/20">Cloud Hub</span>
            </div>
            <p class="text-xs text-slate-400 mt-0.5">云端中枢管理控制台 • 强制鉴权门禁</p>
          </div>
        </div>

        <div class="text-xs rounded-xl border p-3 leading-relaxed ${modeBadgeClass}">
          ${escapeHtml(modeNote)}
        </div>

        <form id="auth-login-form" class="space-y-4">
          <div>
            <label for="auth-token-input" class="block text-xs font-medium text-slate-300 mb-1.5">
              ${sessionInfo.admin_token_is_separate ? '管理员密钥 (MI_ADMIN_TOKEN)' : '管理员 / 集群访问密钥'}
            </label>
            <div class="relative">
              <input
                id="auth-token-input"
                type="password"
                autocomplete="current-password"
                required
                placeholder="请输入访问凭据..."
                class="w-full bg-slate-950 border border-slate-700 rounded-lg pl-3 pr-14 py-2.5 text-sm font-mono text-white placeholder-slate-500 focus:outline-none focus:border-sky-500"
              />
              <button
                type="button"
                id="auth-toggle-vis"
                class="absolute right-2.5 top-1/2 -translate-y-1/2 text-xs text-slate-400 hover:text-slate-200 px-1.5 py-0.5"
              >显示</button>
            </div>
          </div>

          <div id="auth-error-box" class="hidden rounded-lg bg-rose-500/10 border border-rose-500/30 px-3 py-2 text-xs text-rose-300"></div>

          <button
            type="submit"
            id="auth-submit-btn"
            class="w-full py-2.5 px-4 rounded-lg bg-sky-500 hover:bg-sky-400 text-white text-sm font-semibold transition shadow-lg shadow-sky-500/20"
          >
            登录管理控制台
          </button>
        </form>

        <div class="pt-3 border-t border-slate-800/80 flex items-center justify-between text-[11px] text-slate-500">
          <span>Session Cookie (HttpOnly / SameSite=Strict)</span>
          <span>Constant-Time Guard</span>
        </div>
      </div>
    </div>
  `;

  const form = document.getElementById('auth-login-form') as HTMLFormElement;
  const input = document.getElementById('auth-token-input') as HTMLInputElement;
  const toggleBtn = document.getElementById('auth-toggle-vis') as HTMLButtonElement;
  const errBox = document.getElementById('auth-error-box') as HTMLDivElement;
  const submitBtn = document.getElementById('auth-submit-btn') as HTMLButtonElement;

  toggleBtn.addEventListener('click', () => {
    const isPw = input.type === 'password';
    input.type = isPw ? 'text' : 'password';
    toggleBtn.textContent = isPw ? '隐藏' : '显示';
  });

  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    errBox.classList.add('hidden');
    submitBtn.disabled = true;
    submitBtn.textContent = '验证凭据中...';
    try {
      const res = await api.login(input.value);
      onAuthenticated(res);
    } catch (err) {
      const msg =
        err instanceof ApiError
          ? err.message
          : '登录失败，请检查网络连接或凭据。';
      errBox.textContent = msg;
      errBox.classList.remove('hidden');
    } finally {
      submitBtn.disabled = false;
      submitBtn.textContent = '登录管理控制台';
    }
  });
}
