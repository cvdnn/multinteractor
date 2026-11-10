import type {
  AuditLogsPagedResponse,
  AuthSessionResponse,
  CacheSessionDetail,
  ContentCategory,
  IssuedTokenResponse,
  OverviewPayload,
  RegisteredDevice,
  SecurityTokensResponse,
  SystemStatusResponse,
  TelemetryFrame,
  WalDbStats,
} from './types';

export class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

let onUnauthorizedHandler: (() => void) | null = null;

export function setOnUnauthorized(cb: () => void): void {
  onUnauthorizedHandler = cb;
}

async function requestJson<T>(url: string, init?: RequestInit, triggerAuthGate = true): Promise<T> {
  const resp = await fetch(url, {
    credentials: 'same-origin',
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  });

  if (!resp.ok) {
    let detail = `HTTP ${resp.status}`;
    try {
      const errBody = (await resp.json()) as { detail?: string };
      if (errBody.detail) detail = errBody.detail;
    } catch {
      // Ignore parse failure
    }
    if (triggerAuthGate && resp.status === 401 && onUnauthorizedHandler) {
      onUnauthorizedHandler();
    }
    throw new ApiError(resp.status, detail);
  }
  return (await resp.json()) as T;
}

export const api = {
  getSession(): Promise<AuthSessionResponse> {
    return requestJson<AuthSessionResponse>('/api/v1/auth/session', { method: 'GET' }, false);
  },

  login(token: string): Promise<AuthSessionResponse> {
    return requestJson<AuthSessionResponse>(
      '/api/v1/auth/login',
      { method: 'POST', body: JSON.stringify({ token }) },
      false
    );
  },

  logout(): Promise<{ authenticated: boolean }> {
    return requestJson<{ authenticated: boolean }>('/api/v1/auth/logout', { method: 'POST' }, false);
  },

  getOverview(): Promise<OverviewPayload> {
    return requestJson<OverviewPayload>('/api/v1/overview');
  },

  listDevices(onlineOnly = false): Promise<{ devices: RegisteredDevice[]; total: number }> {
    const q = onlineOnly ? '?online_only=true' : '';
    return requestJson<{ devices: RegisteredDevice[]; total: number }>(`/api/v1/devices${q}`);
  },

  kickDevice(deviceId: string): Promise<{ status: string; device_id: string }> {
    return requestJson(`/api/v1/devices/${encodeURIComponent(deviceId)}/kick`, { method: 'POST' });
  },

  disableDevice(deviceId: string): Promise<{ status: string; device_id: string }> {
    return requestJson(`/api/v1/devices/${encodeURIComponent(deviceId)}/disable`, { method: 'POST' });
  },

  enableDevice(deviceId: string): Promise<{ status: string; device_id: string }> {
    return requestJson(`/api/v1/devices/${encodeURIComponent(deviceId)}/enable`, { method: 'POST' });
  },

  removeDevice(deviceId: string): Promise<{ status: string; device_id: string }> {
    return requestJson(`/api/v1/devices/${encodeURIComponent(deviceId)}`, { method: 'DELETE' });
  },

  updateDeviceAcl(
    deviceId: string,
    allowedCategories: ContentCategory[],
    subscribedTopics?: string[]
  ): Promise<{ status: string; device: RegisteredDevice }> {
    return requestJson(`/api/v1/devices/${encodeURIComponent(deviceId)}/acl`, {
      method: 'PATCH',
      body: JSON.stringify({
        allowed_categories: allowedCategories,
        ...(subscribedTopics ? { subscribed_topics: subscribedTopics } : {}),
      }),
    });
  },

  getSecurityTokens(reveal = false): Promise<SecurityTokensResponse> {
    return requestJson<SecurityTokensResponse>(`/api/v1/security/tokens?reveal=${reveal ? 'true' : 'false'}`);
  },

  issueToken(label: string, expectedOs: string): Promise<IssuedTokenResponse> {
    return requestJson<IssuedTokenResponse>('/api/v1/devices/token/issue', {
      method: 'POST',
      body: JSON.stringify({ label, expected_os: expectedOs }),
    });
  },

  revokeToken(tokenId: string): Promise<{ status: string }> {
    return requestJson(`/api/v1/devices/token/${encodeURIComponent(tokenId)}`, { method: 'DELETE' });
  },

  rotateClusterToken(newToken?: string): Promise<{ status: string; token: string }> {
    return requestJson('/api/v1/devices/token/rotate', {
      method: 'POST',
      body: JSON.stringify(newToken ? { new_token: newToken } : {}),
    });
  },

  listCacheSessions(): Promise<{ quota: OverviewPayload['cache_quota']; sessions: OverviewPayload['recent_sessions'] }> {
    return requestJson('/api/v1/cache/sessions');
  },

  getCacheSessionDetail(fileCacheId: string): Promise<CacheSessionDetail> {
    return requestJson(`/api/v1/cache/sessions/${encodeURIComponent(fileCacheId)}`);
  },

  pruneCache(orphanPartsOnly: boolean): Promise<{
    removed_sessions: number;
    orphan_files_removed: number;
    freed_bytes: number;
    orphan_parts_only: boolean;
  }> {
    return requestJson('/api/v1/cache/prune', {
      method: 'POST',
      body: JSON.stringify({ orphan_parts_only: orphanPartsOnly }),
    });
  },

  abortCacheSession(fileCacheId: string): Promise<unknown> {
    return requestJson(`/api/v1/cache/${encodeURIComponent(fileCacheId)}/abort`, { method: 'POST' });
  },

  deleteCacheSession(fileCacheId: string): Promise<{ status: string; freed_bytes: number }> {
    return requestJson(`/api/v1/cache/${encodeURIComponent(fileCacheId)}`, { method: 'DELETE' });
  },

  queryAuditLogs(params: {
    log_type?: string;
    device_id?: string;
    status?: string;
    keyword?: string;
    limit?: number;
    offset?: number;
  }): Promise<AuditLogsPagedResponse> {
    const qs = new URLSearchParams();
    if (params.log_type && params.log_type !== 'ALL') qs.set('log_type', params.log_type.toLowerCase());
    if (params.device_id) qs.set('device_id', params.device_id);
    if (params.status) qs.set('status', params.status);
    if (params.keyword) qs.set('keyword', params.keyword);
    qs.set('limit', String(params.limit ?? 50));
    qs.set('offset', String(params.offset ?? 0));
    return requestJson<AuditLogsPagedResponse>(`/api/v1/audit/logs?${qs.toString()}`);
  },

  getAuditExportUrl(params: {
    log_type?: string;
    device_id?: string;
    status?: string;
    keyword?: string;
  }): string {
    const qs = new URLSearchParams();
    if (params.log_type && params.log_type !== 'ALL') qs.set('log_type', params.log_type.toLowerCase());
    if (params.device_id) qs.set('device_id', params.device_id);
    if (params.status) qs.set('status', params.status);
    if (params.keyword) qs.set('keyword', params.keyword);
    return `/api/v1/audit/export?${qs.toString()}`;
  },

  async exportAuditLogs(params: {
    log_type?: string;
    device_id?: string;
    status?: string;
    keyword?: string;
  }): Promise<Blob> {
    const url = this.getAuditExportUrl(params);
    const resp = await fetch(url, { credentials: 'same-origin' });
    if (!resp.ok) {
      let detail = `HTTP ${resp.status}`;
      try {
        const errBody = (await resp.json()) as { detail?: string };
        if (errBody.detail) detail = errBody.detail;
      } catch {
        // Ignore parse failure
      }
      if (resp.status === 401 && onUnauthorizedHandler) {
        onUnauthorizedHandler();
      }
      throw new ApiError(resp.status, detail);
    }
    return await resp.blob();
  },

  cleanupExpiredAudit(): Promise<{ deleted_rows: number }> {
    return requestJson('/api/v1/audit/logs/expired', { method: 'DELETE' });
  },

  getSystemStatus(): Promise<SystemStatusResponse> {
    return requestJson<SystemStatusResponse>('/api/v1/system/status');
  },

  runWalCheckpoint(): Promise<WalDbStats> {
    return requestJson<WalDbStats>('/api/v1/system/wal-checkpoint', { method: 'POST' });
  },
};

export class AdminTelemetrySocket {
  private ws: WebSocket | null = null;
  private stopped = false;
  private reconnectDelayMs = 1500;
  private timer: number | null = null;
  private onMessage: (frame: TelemetryFrame) => void;
  private onStateChange: (connected: boolean) => void;

  constructor(
    onMessage: (frame: TelemetryFrame) => void,
    onStateChange: (connected: boolean) => void
  ) {
    this.onMessage = onMessage;
    this.onStateChange = onStateChange;
  }

  start(): void {
    this.stopped = false;
    this.connect();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer !== null) {
      window.clearTimeout(this.timer);
      this.timer = null;
    }
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.onStateChange(false);
  }

  private connect(): void {
    if (this.stopped) return;
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${proto}//${window.location.host}/ws/admin`;
    const ws = new WebSocket(url);
    this.ws = ws;

    ws.onopen = () => {
      this.reconnectDelayMs = 1500;
      this.onStateChange(true);
    };

    ws.onmessage = (ev) => {
      try {
        const frame = JSON.parse(String(ev.data)) as TelemetryFrame;
        this.onMessage(frame);
      } catch {
        // Ignore invalid frame
      }
    };

    ws.onclose = (ev) => {
      this.onStateChange(false);
      this.ws = null;
      if (ev.code === 4001 || ev.code === 4003) {
        if (onUnauthorizedHandler) onUnauthorizedHandler();
        return;
      }
      if (!this.stopped) {
        this.timer = window.setTimeout(() => this.connect(), this.reconnectDelayMs);
        this.reconnectDelayMs = Math.min(15000, Math.floor(this.reconnectDelayMs * 1.5));
      }
    };

    ws.onerror = () => {
      ws.close();
    };
  }
}
