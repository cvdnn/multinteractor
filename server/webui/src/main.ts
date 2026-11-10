import './styles/main.css';
import { AdminTelemetrySocket, api, setOnUnauthorized } from './api';
import { renderAuthGate } from './components/authGate';
import { openDeviceAclDrawer, openRotateTokenModal } from './components/modals';
import { showToast } from './components/toast';
import type { AuthSessionResponse, OverviewPayload, RegisteredDevice, TabId } from './types';
import { escapeHtml, formatTtlCountdown } from './utils';
import { renderAuditView, type AuditFilterState } from './views/audit';
import { renderDevicesView } from './views/devices';
import { renderOverviewView } from './views/overview';
import { renderSecurityView } from './views/security';
import { renderSettingsView } from './views/settings';
import { renderStreamView } from './views/stream';

class CloudHubWebConsole {
  private root: HTMLElement;
  private activeTab: TabId = 'overview';
  private overview: OverviewPayload | null = null;
  private adminTokenIsSeparate = false;
  private wsConnected = false;
  private socket: AdminTelemetrySocket | null = null;
  private deviceViewState = { searchQuery: '', onlineOnly: false };
  private auditFilter: AuditFilterState = {
    logType: 'ALL',
    deviceId: '',
    status: '',
    keyword: '',
    limit: 50,
    offset: 0,
  };
  private revealClusterToken = false;
  private ttlTickerId: number | null = null;
  private initRetryTimerId: number | null = null;

  constructor(root: HTMLElement) {
    this.root = root;
    setOnUnauthorized(() => {
      this.handleLoggedOut();
    });
  }

  async init(): Promise<void> {
    if (this.initRetryTimerId !== null) {
      window.clearTimeout(this.initRetryTimerId);
      this.initRetryTimerId = null;
    }
    this.renderLoading('正在验证控制台会话...');
    try {
      const session = await api.getSession();
      this.adminTokenIsSeparate = session.admin_token_is_separate;
      if (!session.authenticated) {
        renderAuthGate(this.root, session, (authRes) => this.handleAuthenticated(authRes));
        return;
      }
      await this.enterDashboard();
    } catch {
      this.renderLoading('无法连接 Cloud Hub 服务，3 秒后自动重试...', true);
      this.initRetryTimerId = window.setTimeout(() => {
        void this.init();
      }, 3000);
    }
  }

  private renderLoading(msg: string, showRetry = false): void {
    this.root.innerHTML = `
      <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-300 text-xs font-mono">
        <div class="flex flex-col items-center space-y-3">
          <div class="flex items-center space-x-3">
            <span class="w-2.5 h-2.5 rounded-full bg-sky-400 animate-ping"></span>
            <span>${escapeHtml(msg)}</span>
          </div>
          ${
            showRetry
              ? '<button id="init-retry-btn" class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-700 hover:bg-slate-800 text-sky-400 transition">立即重试连接</button>'
              : ''
          }
        </div>
      </div>
    `;
    if (showRetry) {
      this.root.querySelector('#init-retry-btn')?.addEventListener('click', () => {
        void this.init();
      });
    }
  }

  private async handleAuthenticated(res: AuthSessionResponse): Promise<void> {
    this.adminTokenIsSeparate = res.admin_token_is_separate;
    await this.enterDashboard();
  }

  private handleLoggedOut(): void {
    this.socket?.stop();
    this.socket = null;
    if (this.ttlTickerId !== null) {
      window.clearInterval(this.ttlTickerId);
      this.ttlTickerId = null;
    }
    void api.getSession().then((session) => {
      this.adminTokenIsSeparate = session.admin_token_is_separate;
      renderAuthGate(this.root, session, (authRes) => this.handleAuthenticated(authRes));
    });
  }

  private async enterDashboard(): Promise<void> {
    this.overview = await api.getOverview();
    this.renderShell();
    await this.renderActiveTab();

    this.socket?.stop();
    this.socket = new AdminTelemetrySocket(
      (frame) => {
        if (frame.type === 'telemetry' && frame.overview) {
          this.overview = frame.overview;
          this.updateShellBadges();
          if (this.activeTab === 'overview' || this.activeTab === 'stream' || this.activeTab === 'devices') {
            void this.renderActiveTab(true);
          }
        }
      },
      (connected) => {
        this.wsConnected = connected;
        this.updateShellBadges();
      }
    );
    this.socket.start();

    if (this.ttlTickerId !== null) {
      window.clearInterval(this.ttlTickerId);
    }
    this.ttlTickerId = window.setInterval(() => {
      document.querySelectorAll<HTMLElement>('[data-ttl-seconds]').forEach((el) => {
        const cur = Number(el.dataset['ttlSeconds'] ?? '0');
        const next = Math.max(0, cur - 1);
        el.dataset['ttlSeconds'] = String(next);
        const prefix = el.textContent?.includes('24h TTL') ? '24h TTL 剩余 ' : '';
        el.textContent = `${prefix}${formatTtlCountdown(next)}`;
      });
    }, 1000);
  }

  private renderShell(): void {
    const sys = this.overview?.system;
    const hostPort = sys ? `${sys.host}:${sys.port}` : window.location.host;
    const devCount = this.overview?.device_stats.total ?? 0;
    const streamingActive = (this.overview?.active_streams.length ?? 0) > 0;

    const navItems: Array<{ id: TabId; label: string; icon: string; extra?: string }> = [
      {
        id: 'overview',
        label: '实时总览大屏',
        icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" />',
      },
      {
        id: 'devices',
        label: '设备台账与准入',
        icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />',
        extra: `<span id="nav-device-count" class="ml-auto text-xs px-1.5 py-0.5 rounded-full bg-slate-800 text-slate-300 font-mono">${devCount}</span>`,
      },
      {
        id: 'stream',
        label: '边传边下流式缓存',
        icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M9 19l3 3m0 0l3-3m-3 3V10" />',
        extra: `<span id="nav-stream-dot" class="ml-auto flex h-2 w-2 relative ${streamingActive ? '' : 'hidden'}"><span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-sky-400 opacity-75"></span><span class="relative inline-flex rounded-full h-2 w-2 bg-sky-500"></span></span>`,
      },
      {
        id: 'audit',
        label: '全链路日志审计',
        icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />',
      },
      {
        id: 'security',
        label: '安全凭证与 Token',
        icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />',
      },
      {
        id: 'settings',
        label: '系统健康与参数',
        icon: '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />',
      },
    ];

    this.root.innerHTML = `
      <header class="h-14 border-b border-slate-800 bg-slate-950/85 backdrop-blur px-5 flex items-center justify-between sticky top-0 z-40">
        <div class="flex items-center space-x-3">
          <div class="w-8 h-8 rounded-lg bg-gradient-to-tr from-sky-500 to-indigo-600 flex items-center justify-center shadow-lg shadow-sky-500/20">
            <svg class="w-5 h-5 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
            </svg>
          </div>
          <div class="flex items-center space-x-2">
            <span class="font-bold text-base tracking-wide text-white">multinteractor</span>
            <span class="text-xs px-2 py-0.5 rounded-full bg-sky-500/10 text-sky-400 font-mono border border-sky-500/20">Cloud Hub v6</span>
          </div>
          <span class="text-slate-600 text-xs hidden md:inline">|</span>
          <span class="text-slate-400 text-xs hidden md:inline">跨设备协同与流式中转中枢</span>
        </div>

        <div class="flex items-center space-x-3">
          <div class="flex items-center space-x-2 bg-slate-900 border border-slate-800 rounded-lg px-3 py-1.5 text-xs">
            <span id="hdr-ws-dot" class="w-2 h-2 rounded-full ${this.wsConnected ? 'bg-emerald-500' : 'bg-amber-400'}"></span>
            <span class="text-slate-300 font-mono">${escapeHtml(hostPort)}</span>
            <span class="text-slate-600">|</span>
            <span id="hdr-ws-label" class="font-medium ${this.wsConnected ? 'text-emerald-400' : 'text-amber-300'}">${this.wsConnected ? 'TELEMETRY LIVE' : 'POLLING / CONNECTING'}</span>
          </div>

          <button id="hdr-rotate-btn" class="hidden sm:flex items-center space-x-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-amber-500/10 text-amber-300 border border-amber-500/30 hover:bg-amber-500/20 transition">
            <span>轮转集群密钥</span>
          </button>

          <button id="hdr-logout-btn" class="px-2.5 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200 border border-slate-800 hover:bg-slate-900 transition">
            退出登录
          </button>
        </div>
      </header>

      <div class="flex-1 flex overflow-hidden">
        <aside class="w-60 border-r border-slate-800 bg-slate-950/60 p-4 flex flex-col justify-between shrink-0 hidden md:flex">
          <nav class="space-y-1.5" id="sidebar-nav">
            ${navItems
              .map(
                (item) => `
              <button data-tab="${item.id}" class="nav-tab-btn w-full flex items-center space-x-3 px-3 py-2.5 rounded-lg text-sm font-medium transition border ${
                this.activeTab === item.id
                  ? 'bg-sky-500/15 text-sky-400 border-sky-500/20 shadow-sm'
                  : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900 border-transparent'
              }">
                <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">${item.icon}</svg>
                <span>${item.label}</span>
                ${item.extra ?? ''}
              </button>
            `
              )
              .join('')}
          </nav>

          <div class="border border-slate-800 rounded-xl p-3 bg-slate-900/60 space-y-2 text-xs">
            <div class="flex items-center justify-between text-slate-400">
              <span>SQLite WAL</span>
              <span class="text-emerald-400 font-mono">${escapeHtml(sys?.synchronous ?? 'NORMAL')}</span>
            </div>
            <div class="flex items-center justify-between text-slate-400">
              <span>客户端内网闲断</span>
              <span class="text-slate-300 font-mono">${sys?.lan_idle_timeout_seconds ?? 180}s 策略</span>
            </div>
            <div class="flex items-center justify-between text-slate-400">
              <span>云端缓存 TTL</span>
              <span class="text-sky-400 font-mono">${Math.round((sys?.cache_ttl_seconds ?? 86400) / 3600)}h 超时清理</span>
            </div>
            <div class="pt-2 border-t border-slate-800/80 flex items-center justify-between">
              <span class="text-slate-500">管理平面隔离</span>
              <span class="font-mono font-semibold ${this.adminTokenIsSeparate ? 'text-emerald-400' : 'text-amber-300'}">${this.adminTokenIsSeparate ? 'ADMIN_TOKEN' : 'FALLBACK'}</span>
            </div>
          </div>
        </aside>

        <div class="flex-1 flex flex-col overflow-hidden">
          <!-- Mobile Tab Strip -->
          <div class="md:hidden flex items-center space-x-1 overflow-x-auto px-3 py-2 border-b border-slate-800 bg-slate-950/80">
            ${navItems
              .map(
                (item) => `
              <button data-tab="${item.id}" class="nav-tab-btn px-2.5 py-1.5 rounded-lg text-xs whitespace-nowrap border ${
                this.activeTab === item.id
                  ? 'bg-sky-500/15 text-sky-400 border-sky-500/20'
                  : 'text-slate-400 border-transparent'
              }">${item.label}</button>
            `
              )
              .join('')}
          </div>

          <main id="tab-content-container" class="flex-1 overflow-y-auto p-6 bg-slate-900/40"></main>
        </div>
      </div>
    `;

    this.root.querySelectorAll<HTMLButtonElement>('.nav-tab-btn').forEach((btn) => {
      btn.addEventListener('click', () => {
        const next = btn.dataset['tab'] as TabId | undefined;
        if (next) void this.switchTab(next);
      });
    });

    this.root.querySelector('#hdr-rotate-btn')?.addEventListener('click', () => {
      openRotateTokenModal(() => {
        void this.refreshCurrentView();
      });
    });

    this.root.querySelector('#hdr-logout-btn')?.addEventListener('click', async () => {
      await api.logout();
      this.handleLoggedOut();
    });
  }

  private updateShellBadges(): void {
    const devBadge = document.getElementById('nav-device-count');
    if (devBadge && this.overview) {
      devBadge.textContent = String(this.overview.device_stats.total);
    }
    const streamDot = document.getElementById('nav-stream-dot');
    if (streamDot && this.overview) {
      streamDot.classList.toggle('hidden', this.overview.active_streams.length === 0);
    }
    const wsDot = document.getElementById('hdr-ws-dot');
    const wsLabel = document.getElementById('hdr-ws-label');
    if (wsDot && wsLabel) {
      wsDot.className = `w-2 h-2 rounded-full ${this.wsConnected ? 'bg-emerald-500' : 'bg-amber-400'}`;
      wsLabel.className = `font-medium ${this.wsConnected ? 'text-emerald-400' : 'text-amber-300'}`;
      wsLabel.textContent = this.wsConnected ? 'WS TELEMETRY LIVE' : 'RECONNECTING';
    }
  }

  private async switchTab(tab: TabId): Promise<void> {
    this.activeTab = tab;
    this.root.querySelectorAll<HTMLButtonElement>('.nav-tab-btn').forEach((btn) => {
      const isActive = btn.dataset['tab'] === tab;
      if (btn.closest('#sidebar-nav')) {
        btn.className = isActive
          ? 'nav-tab-btn w-full flex items-center space-x-3 px-3 py-2.5 rounded-lg text-sm font-medium transition border bg-sky-500/15 text-sky-400 border-sky-500/20 shadow-sm'
          : 'nav-tab-btn w-full flex items-center space-x-3 px-3 py-2.5 rounded-lg text-sm font-medium transition border text-slate-400 hover:text-slate-200 hover:bg-slate-900 border-transparent';
      } else {
        btn.className = isActive
          ? 'nav-tab-btn px-2.5 py-1.5 rounded-lg text-xs whitespace-nowrap border bg-sky-500/15 text-sky-400 border-sky-500/20'
          : 'nav-tab-btn px-2.5 py-1.5 rounded-lg text-xs whitespace-nowrap border text-slate-400 border-transparent';
      }
    });
    await this.renderActiveTab();
  }

  private async refreshCurrentView(): Promise<void> {
    this.overview = await api.getOverview();
    this.updateShellBadges();
    await this.renderActiveTab();
  }

  private async renderActiveTab(fromTelemetry = false): Promise<void> {
    const container = document.getElementById('tab-content-container');
    if (!container) return;

    // 保留当前聚焦输入框的焦点与光标位置，避免后台遥测推送打断输入
    const activeEl = document.activeElement;
    const focusedInputId =
      fromTelemetry && activeEl instanceof HTMLInputElement && container.contains(activeEl) && activeEl.id
        ? activeEl.id
        : null;
    const selStart = focusedInputId && activeEl instanceof HTMLInputElement ? activeEl.selectionStart : null;
    const selEnd = focusedInputId && activeEl instanceof HTMLInputElement ? activeEl.selectionEnd : null;

    try {
      if (this.activeTab === 'overview') {
        const data = this.overview ?? (await api.getOverview());
        this.overview = data;
        renderOverviewView(container, data, {
          onSwitchTab: (tab) => void this.switchTab(tab),
          onSelectDevice: (dev: RegisteredDevice) =>
            openDeviceAclDrawer(dev, () => void this.refreshCurrentView()),
        });
      } else if (this.activeTab === 'devices') {
        const res = await api.listDevices(false);
        renderDevicesView(container, res.devices, this.deviceViewState, () => {
          void this.refreshCurrentView();
        });
      } else if (this.activeTab === 'stream') {
        const res = await api.listCacheSessions();
        renderStreamView(container, res.quota, res.sessions, () => {
          void this.refreshCurrentView();
        });
      } else if (this.activeTab === 'audit') {
        const [auditRes, devRes] = await Promise.all([
          api.queryAuditLogs({
            log_type: this.auditFilter.logType,
            device_id: this.auditFilter.deviceId || undefined,
            status: this.auditFilter.status || undefined,
            keyword: this.auditFilter.keyword || undefined,
            limit: this.auditFilter.limit,
            offset: this.auditFilter.offset,
          }),
          api.listDevices(false),
        ]);
        renderAuditView(container, auditRes, devRes.devices, this.auditFilter, () => {
          void this.renderActiveTab();
        });
      } else if (this.activeTab === 'security') {
        const [secRes, ov] = await Promise.all([
          api.getSecurityTokens(this.revealClusterToken),
          api.getOverview(),
        ]);
        this.overview = ov;
        renderSecurityView(
          container,
          secRes,
          ov.security_warnings,
          this.adminTokenIsSeparate,
          this.revealClusterToken,
          (nextReveal) => {
            this.revealClusterToken = nextReveal;
            void this.renderActiveTab();
          },
          () => void this.refreshCurrentView()
        );
      } else if (this.activeTab === 'settings') {
        const status = await api.getSystemStatus();
        renderSettingsView(container, status, () => void this.renderActiveTab());
      }

      if (focusedInputId) {
        const restored = container.querySelector<HTMLInputElement>(`#${CSS.escape(focusedInputId)}`);
        if (restored) {
          restored.focus();
          if (selStart !== null && selEnd !== null) {
            restored.setSelectionRange(selStart, selEnd);
          }
        }
      }
    } catch (err) {
      if (!fromTelemetry) {
        showToast(err instanceof Error ? err.message : '视图加载失败', 'error');
      }
    }
  }
}

function ensureFavicon(): void {
  if (document.querySelector('link[rel="icon"]')) return;
  const ns = ['http', '://www.w3.org/2000/svg'].join('');
  const svg = `<svg xmlns="${ns}" viewBox="0 0 32 32"><rect width="32" height="32" rx="8" fill="#0284c7"/><path d="M9 12h14m0 0l-4-4m4 4l-4 4M23 20H9m0 0l4 4m-4-4l4-4" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/></svg>`;
  const blob = new Blob([svg], { type: 'image/svg+xml' });
  const link = document.createElement('link');
  link.rel = 'icon';
  link.type = 'image/svg+xml';
  link.href = URL.createObjectURL(blob);
  document.head.appendChild(link);
}

ensureFavicon();

const appRoot = document.getElementById('app');
if (appRoot) {
  const consoleApp = new CloudHubWebConsole(appRoot);
  void consoleApp.init();
}
