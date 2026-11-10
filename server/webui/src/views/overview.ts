import type {
  AuditLogItem,
  CacheSession,
  OverviewPayload,
  RegisteredDevice,
  TabId,
} from '../types';
import {
  escapeHtml,
  formatBytes,
  formatRelativeMs,
  formatTimeShortMs,
  formatTtlCountdown,
  osShortLabel,
  shortHash,
} from '../utils';

export interface OverviewHandlers {
  onSwitchTab: (tab: TabId) => void;
  onSelectDevice: (device: RegisteredDevice) => void;
}

function buildTopologySvg(data: OverviewPayload): string {
  const cx = 310;
  const cy = 130;
  const rx = 205;
  const ry = 85;
  const devices = data.devices.slice(0, 12);

  if (devices.length === 0) {
    return `
      <div class="relative h-64 bg-slate-900/60 rounded-lg border border-slate-800/80 overflow-hidden flex items-center justify-center">
        <svg class="w-full h-full" viewBox="0 0 620 260">
          <defs>
            <linearGradient id="gradHub" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stop-color="#0284c7" />
              <stop offset="100%" stop-color="#4f46e5" />
            </linearGradient>
          </defs>
          <g transform="translate(${cx}, ${cy})">
            <circle r="38" fill="url(#gradHub)" />
            <circle r="38" fill="none" stroke="#38bdf8" stroke-width="1.5" class="animate-pulse-subtle" />
            <text y="-4" text-anchor="middle" fill="#ffffff" font-size="11" font-weight="bold">Cloud Hub</text>
            <text y="12" text-anchor="middle" fill="#bae6fd" font-size="9" font-family="monospace">FastAPI / WAL</text>
          </g>
          <text x="${cx}" y="220" text-anchor="middle" fill="#64748b" font-size="11">暂无已注册设备，等待 mi-daemon 接入信令网关 /ws/signaling ...</text>
        </svg>
      </div>
    `;
  }

  const coords = new Map<string, { x: number; y: number; dev: RegisteredDevice }>();
  const count = devices.length;
  devices.forEach((dev, idx) => {
    const angle = -Math.PI * 0.75 + (idx * 2 * Math.PI) / count;
    const x = Math.round(cx + rx * Math.cos(angle));
    const y = Math.round(cy + ry * Math.sin(angle));
    coords.set(dev.device_id, { x, y, dev });
  });

  // Base signaling lines (Cloud Hub <-> Device)
  const signalingLines = Array.from(coords.values())
    .map(({ x, y, dev }) => {
      if (!dev.enabled) {
        return `<line x1="${cx}" y1="${cy}" x2="${x}" y2="${y}" stroke="#f43f5e" stroke-width="1.2" stroke-dasharray="3 3" stroke-opacity="0.55" />`;
      }
      if (dev.online) {
        return `<line x1="${cx}" y1="${cy}" x2="${x}" y2="${y}" stroke="#0284c7" stroke-width="1.6" stroke-opacity="0.65" />`;
      }
      return `<line x1="${cx}" y1="${cy}" x2="${x}" y2="${y}" stroke="#475569" stroke-width="1" stroke-dasharray="4 4" stroke-opacity="0.45" />`;
    })
    .join('');

  // Same-subnet LAN links + objective recent LAN_DIRECT audit status (never fabricates 180s client countdown)
  const lanLinks: string[] = [];
  const drawnLanPairs = new Set<string>();
  const recentLanPairs = new Set<string>();
  const recentCloudFallbackPairs = new Set<string>();

  for (const ch of data.channel_activity) {
    const pairKey = [ch.source_device_id, ch.target_device_id].sort().join('|');
    if (ch.channel_used === 'LAN_DIRECT') {
      recentLanPairs.add(pairKey);
    } else if (ch.channel_used === 'CLOUD_CACHE') {
      recentCloudFallbackPairs.add(pairKey);
    }
  }

  for (const [subnet, devIds] of Object.entries(data.lan_subnets)) {
    for (let i = 0; i < devIds.length; i++) {
      for (let j = i + 1; j < devIds.length; j++) {
        const aId = devIds[i];
        const bId = devIds[j];
        if (!aId || !bId) continue;
        const a = coords.get(aId);
        const b = coords.get(bId);
        if (!a || !b) continue;
        const pairKey = [aId, bId].sort().join('|');
        if (drawnLanPairs.has(pairKey)) continue;
        drawnLanPairs.add(pairKey);
        const midX = Math.round((a.x + b.x) / 2);
        const midY = Math.round((a.y + b.y) / 2);
        const mx = Math.round(midX + (midX - cx) * 0.25);
        const my = Math.round(midY + (midY - cy) * 0.25);
        const isHot = recentLanPairs.has(pairKey);
        const isFallback = !isHot && recentCloudFallbackPairs.has(pairKey);
        const strokeColor = isFallback ? '#f59e0b' : '#10b981';
        const label = isHot ? 'LAN_DIRECT' : isFallback ? 'CLOUD FALLBACK' : `LAN ${subnet}`;
        lanLinks.push(
          `<path d="M ${a.x} ${a.y} Q ${mx} ${my} ${b.x} ${b.y}" fill="none" stroke="${strokeColor}" stroke-width="${isHot || isFallback ? '2.2' : '1.4'}" stroke-dasharray="5 3" stroke-opacity="${isHot || isFallback ? '0.9' : '0.5'}" />`,
          `<text x="${mx}" y="${my - 4}" text-anchor="middle" fill="${strokeColor}" font-size="9" font-family="monospace">${escapeHtml(label)}</text>`
        );
      }
    }
  }

  // Active Stream Teeing curves (Source -> Hub -> Consumers)
  const streamLines: string[] = [];
  for (const stream of data.active_streams) {
    const src = coords.get(stream.source_device_id);
    if (src) {
      const qx = Math.round((src.x + cx) / 2);
      const qy = Math.round((src.y + cy) / 2) - 18;
      streamLines.push(
        `<path d="M ${src.x} ${src.y} Q ${qx} ${qy} ${cx} ${cy}" fill="none" stroke="#818cf8" stroke-width="2.6" class="stream-flow-active" />`
      );
    }
    for (const reader of stream.active_readers) {
      const dst = coords.get(reader.reader_device_id);
      if (dst) {
        const qx = Math.round((cx + dst.x) / 2);
        const qy = Math.round((cy + dst.y) / 2) - 18;
        streamLines.push(
          `<path d="M ${cx} ${cy} Q ${qx} ${qy} ${dst.x} ${dst.y}" fill="none" stroke="#38bdf8" stroke-width="2.4" class="stream-flow-active" />`
        );
      }
    }
  }

  const deviceNodes = Array.from(coords.values())
    .map(({ x, y, dev }) => {
      const stroke = !dev.enabled
        ? '#f43f5e'
        : dev.online
          ? '#38bdf8'
          : '#475569';
      const dotColor = !dev.enabled
        ? '#f43f5e'
        : dev.online
          ? '#10b981'
          : '#64748b';
      const opacity = dev.online && dev.enabled ? '1' : '0.65';
      const nameTrim = dev.device_name.length > 11 ? `${dev.device_name.slice(0, 10)}…` : dev.device_name;
      const endpointText = dev.lan_endpoint ? dev.lan_endpoint.split(':')[0] ?? dev.lan_endpoint : dev.online ? 'WSS Only' : '离线';
      const subLine = `${osShortLabel(dev.os)} • ${endpointText}`;
      return `
        <g transform="translate(${x}, ${y})" class="cursor-pointer topo-device-node" data-device-id="${escapeHtml(dev.device_id)}" opacity="${opacity}">
          <rect x="-54" y="-22" width="108" height="44" rx="8" fill="#0f172a" stroke="${stroke}" stroke-width="1.5" />
          <circle cx="-40" cy="0" r="4" fill="${dotColor}" />
          <text x="-30" y="-3" fill="#e2e8f0" font-size="10" font-weight="600">${escapeHtml(nameTrim)}</text>
          <text x="-30" y="11" fill="#94a3b8" font-size="8" font-family="monospace">${escapeHtml(subLine)}</text>
        </g>
      `;
    })
    .join('');

  return `
    <div class="relative h-64 bg-slate-900/60 rounded-lg border border-slate-800/80 overflow-hidden flex items-center justify-center">
      <svg class="w-full h-full" viewBox="0 0 620 260">
        <defs>
          <linearGradient id="gradHub" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#0284c7" />
            <stop offset="100%" stop-color="#4f46e5" />
          </linearGradient>
        </defs>
        ${signalingLines}
        ${lanLinks.join('')}
        ${streamLines.join('')}
        <g transform="translate(${cx}, ${cy})">
          <circle r="36" fill="url(#gradHub)" filter="drop-shadow(0 0 12px rgba(2, 132, 199, 0.38))" />
          <circle r="36" fill="none" stroke="#38bdf8" stroke-width="1.5" class="animate-pulse-subtle" />
          <text y="-4" text-anchor="middle" fill="#ffffff" font-size="11" font-weight="bold">Cloud Hub</text>
          <text y="12" text-anchor="middle" fill="#bae6fd" font-size="9" font-family="monospace">${escapeHtml(data.system.journal_mode)} / v6</text>
        </g>
        ${deviceNodes}
      </svg>
    </div>
  `;
}

function renderStreamCard(session: CacheSession | undefined, lanIdleTimeoutSeconds: number): string {
  if (!session) {
    return `
      <div class="bg-slate-900/80 border border-slate-800 rounded-lg p-4 text-xs text-slate-400 space-y-2">
        <div class="font-semibold text-slate-200">当前无活跃流式中转会话</div>
        <p class="leading-relaxed text-slate-400">
          局域网同网段默认由 Rust 客户端开启即用即连内网直传（空闲 ${lanIdleTimeoutSeconds} 秒客户端自主关闭）；跨网段或内网异常时自动无缝切换云端 Stream Teeing 边传边下缓存（24h 超时自动清理）。
        </p>
      </div>
    `;
  }

  const primaryReader = session.active_readers[0];
  const readPct = primaryReader ? primaryReader.progress_pct : session.state === 'COMPLETED' ? 100 : 0;
  const readBytes = primaryReader
    ? primaryReader.read_offset
    : session.state === 'COMPLETED'
      ? session.total_size
      : 0;
  const readerLabel =
    session.active_readers.length > 0
      ? session.active_readers.map((r) => escapeHtml(r.reader_device_id)).join(', ')
      : session.state === 'COMPLETED'
        ? `已落盘 (${session.download_count} 次拉取)`
        : '等待接收端接入';

  const statusBadge =
    session.state === 'STREAMING'
      ? `<span class="text-xs px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 font-mono">边传边下中</span>`
      : session.state === 'COMPLETED'
        ? `<span class="text-xs px-2 py-0.5 rounded bg-sky-500/10 text-sky-400 border border-sky-500/20 font-mono">已落盘归档</span>`
        : `<span class="text-xs px-2 py-0.5 rounded bg-rose-500/10 text-rose-400 border border-rose-500/20 font-mono">ABORTED</span>`;

  return `
    <div class="bg-slate-900 border border-slate-800 rounded-lg p-3.5 space-y-3">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <div class="text-xs font-semibold text-slate-200 truncate">${escapeHtml(session.file_name)}</div>
          <div class="text-[11px] text-slate-400 mt-0.5 truncate">
            源端: <span class="text-sky-300 font-mono">${escapeHtml(session.source_device_id)}</span>
            → 接收端: <span class="text-indigo-300 font-mono">${readerLabel}</span>
          </div>
        </div>
        ${statusBadge}
      </div>

      <div>
        <div class="flex items-center justify-between text-xs mb-1">
          <span class="text-slate-400">已上传落盘 (Upload-Offset)</span>
          <span class="font-mono text-sky-400">${formatBytes(session.written_offset)} / ${formatBytes(session.total_size)} (${session.progress_pct.toFixed(1)}%)</span>
        </div>
        <div class="w-full bg-slate-800 rounded-full h-2 overflow-hidden">
          <div class="bg-sky-500 h-2 rounded-full transition-all duration-300" style="width: ${session.progress_pct}%"></div>
        </div>

        <div class="flex items-center justify-between text-xs mt-2.5 mb-1">
          <span class="text-slate-400">接收端消费水位 (Range Read)</span>
          <span class="font-mono text-indigo-300">${formatBytes(readBytes)} / ${formatBytes(session.total_size)} (${readPct.toFixed(1)}%)</span>
        </div>
        <div class="w-full bg-slate-800 rounded-full h-2 overflow-hidden">
          <div class="bg-indigo-400 h-2 rounded-full transition-all duration-300" style="width: ${readPct}%"></div>
        </div>
      </div>

      <div class="pt-2 border-t border-slate-800/80 grid grid-cols-2 gap-2 text-[11px]">
        <div>
          <span class="text-slate-500">4MB 分块校验:</span>
          <div class="text-slate-300 font-mono">${session.verified_chunks} / ${session.total_chunks} 块已验</div>
        </div>
        <div>
          <span class="text-slate-500">24H 缓存剩余 TTL:</span>
          <div class="text-amber-300 font-mono" data-ttl-seconds="${session.ttl_remaining_seconds}">${formatTtlCountdown(session.ttl_remaining_seconds)}</div>
        </div>
      </div>
    </div>
  `;
}

function renderActivityRow(item: AuditLogItem): string {
  const timeStr = formatTimeShortMs(item.timestamp_ms);
  if (item.record_type === 'EVENT') {
    return `
      <tr class="hover:bg-slate-900/40 cursor-pointer audit-jump-row">
        <td class="py-2 px-3 text-slate-400">${escapeHtml(timeStr)}</td>
        <td class="py-2 px-3"><span class="px-2 py-0.5 rounded bg-sky-500/10 text-sky-400">EVENT</span></td>
        <td class="py-2 px-3 text-white">${escapeHtml(item.source_device_id)}</td>
        <td class="py-2 px-3 text-slate-300">${escapeHtml(item.topic)}</td>
        <td class="py-2 px-3 text-emerald-400">${escapeHtml(item.content_category)} (${formatBytes(item.size_bytes ?? 0)}) • ${escapeHtml(shortHash(item.content_hash))}</td>
        <td class="py-2 px-3 text-slate-400">投递 ${(item.target_device_ids ?? []).length} 台设备</td>
      </tr>
    `;
  }
  if (item.record_type === 'TRANSFER') {
    const isOk = item.verification_status === 'VERIFIED_OK';
    const isCloud = item.channel_used === 'CLOUD_CACHE';
    return `
      <tr class="hover:bg-slate-900/40 cursor-pointer audit-jump-row">
        <td class="py-2 px-3 text-slate-400">${escapeHtml(timeStr)}</td>
        <td class="py-2 px-3"><span class="px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-400">TRANSFER</span></td>
        <td class="py-2 px-3 text-white">${escapeHtml(item.source_device_id)} → ${escapeHtml(item.target_device_id)}</td>
        <td class="py-2 px-3 text-slate-300">${escapeHtml(item.file_cache_id || item.transfer_id)}</td>
        <td class="py-2 px-3 ${isOk ? 'text-emerald-400' : 'text-rose-400'}">${escapeHtml(item.verification_status)} (${formatBytes(item.bytes_transferred ?? 0)})</td>
        <td class="py-2 px-3 ${isCloud ? 'text-indigo-400' : 'text-emerald-400'}">${escapeHtml(item.channel_used)}${item.resumed ? ` • 续传#${item.resume_count}` : ''}</td>
      </tr>
    `;
  }
  const ok = item.status === 'OK';
  return `
    <tr class="hover:bg-slate-900/40 cursor-pointer audit-jump-row">
      <td class="py-2 px-3 text-slate-400">${escapeHtml(timeStr)}</td>
      <td class="py-2 px-3"><span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400">CONN</span></td>
      <td class="py-2 px-3 text-white">${escapeHtml(item.device_name || item.device_id)}</td>
      <td class="py-2 px-3 text-slate-300">${escapeHtml(item.action)}</td>
      <td class="py-2 px-3 ${ok ? 'text-emerald-400' : 'text-rose-400'}">${escapeHtml(item.status)}</td>
      <td class="py-2 px-3 text-slate-400 truncate max-w-xs">${escapeHtml(item.lan_endpoint || item.detail || '-')}</td>
    </tr>
  `;
}

export function renderOverviewView(
  container: HTMLElement,
  data: OverviewPayload,
  handlers: OverviewHandlers
): void {
  const usagePct = data.cache_quota.usage_pct;
  const quotaBarColor =
    usagePct >= 90
      ? 'from-rose-500 to-red-600'
      : usagePct >= 70
        ? 'from-amber-400 to-orange-500'
        : 'from-sky-500 to-indigo-500';

  const featuredSession = data.active_streams[0] ?? data.recent_sessions[0];
  const totalReaders = data.cache_quota.active_reader_count;
  const osSummary = `Win ${data.device_stats.online_by_os.windows} • Mac ${data.device_stats.online_by_os.macos} • Lnx ${data.device_stats.online_by_os.linux}`;

  const warningBanners = data.security_warnings
    .map(
      (w) => `
      <div class="rounded-xl border ${w.severity === 'HIGH' ? 'bg-rose-500/10 border-rose-500/35 text-rose-200' : 'bg-amber-500/10 border-amber-500/35 text-amber-200'} px-4 py-3 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs">
        <div class="flex items-start space-x-2.5">
          <span class="px-1.5 py-0.5 rounded font-mono text-[10px] font-bold shrink-0 ${w.severity === 'HIGH' ? 'bg-rose-500/20 text-rose-300' : 'bg-amber-500/20 text-amber-300'}">${escapeHtml(w.severity)}</span>
          <div>
            <div class="font-semibold">${escapeHtml(w.title)}</div>
            <div class="opacity-85 mt-0.5">${escapeHtml(w.message)}</div>
          </div>
        </div>
        <button class="goto-security-btn shrink-0 px-3 py-1 rounded-lg bg-slate-950/60 hover:bg-slate-900 border border-current/20 font-medium">
          前往安全中心处理 →
        </button>
      </div>
    `
    )
    .join('');

  container.innerHTML = `
    <div class="space-y-6">
      ${warningBanners}

      <!-- 4 KPI Cards -->
      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>在线终端设备</span>
            <span class="p-1 rounded bg-sky-500/10 text-sky-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-white">${data.device_stats.online}</span>
            <span class="text-xs text-slate-400">/ ${data.device_stats.total} 台账 ${data.device_stats.disabled > 0 ? `(禁用 ${data.device_stats.disabled})` : ''}</span>
          </div>
          <div class="mt-2 flex items-center text-xs text-emerald-400 space-x-1.5 font-mono">
            <span class="w-1.5 h-1.5 rounded-full bg-emerald-400"></span>
            <span>${escapeHtml(osSummary)}</span>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>边传边下分流会话</span>
            <span class="p-1 rounded bg-indigo-500/10 text-indigo-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-indigo-300">${data.cache_quota.streaming_count}</span>
            <span class="text-xs text-slate-400">活跃中 / ${data.cache_quota.completed_count} 归档</span>
          </div>
          <div class="mt-2 text-xs text-indigo-400/90 flex items-center justify-between">
            <span>并发消费者: <strong class="text-indigo-200 font-mono">${totalReaders}</strong></span>
            <span class="text-[11px] px-1.5 py-0.5 rounded bg-indigo-500/15 text-indigo-300 font-mono">24h 自动清理</span>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>云端流式缓存池</span>
            <span class="p-1 rounded bg-amber-500/10 text-amber-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2 1.5 3 3.5 3h9c2 0 3.5-1 3.5-3V7M4 7c0-2 1.5-3 3.5-3h9c2 0 3.5 1 3.5 3M4 7h16" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-white">${formatBytes(data.cache_quota.used_bytes)}</span>
            <span class="text-xs text-slate-400">/ ${formatBytes(data.cache_quota.quota_bytes)} (${usagePct.toFixed(1)}%)</span>
          </div>
          <div class="mt-2 w-full bg-slate-800 rounded-full h-1.5 overflow-hidden">
            <div class="bg-gradient-to-r ${quotaBarColor} h-1.5 rounded-full transition-all" style="width: ${Math.max(2, usagePct)}%"></div>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>摘要完整性与续传</span>
            <span class="p-1 rounded bg-emerald-500/10 text-emerald-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-emerald-400">${data.audit_summary.blake3_pass_rate}%</span>
            <span class="text-xs text-slate-400">BLAKE3 校验</span>
          </div>
          <div class="mt-2 text-xs text-slate-400 flex items-center justify-between">
            <span>断点续传: <strong class="text-slate-200 font-mono">${data.audit_summary.resumed_transfer_count} 次</strong></span>
            <span class="${data.audit_summary.hash_mismatch_count === 0 ? 'text-emerald-400' : 'text-rose-400'}">${data.audit_summary.hash_mismatch_count} 损坏</span>
          </div>
        </div>
      </div>

      <!-- Topology & Stream Teeing Tracker -->
      <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div class="lg:col-span-2 bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm">
          <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-2 mb-4">
            <div>
              <h3 class="text-sm font-semibold text-white flex items-center space-x-2">
                <span>多机协同与通道拓扑</span>
                <span class="text-xs px-2 py-0.5 rounded bg-sky-500/10 text-sky-400 font-normal">实时信令中枢</span>
              </h3>
              <p class="text-xs text-slate-400 mt-0.5">展示真实 WSS 信令连接、同网段内网直连候选分组（客户端自主控制 180s 空闲断开）及云端 Stream Teeing 兜底中转</p>
            </div>
            <div class="flex items-center space-x-3 text-xs shrink-0">
              <span class="flex items-center space-x-1.5"><span class="w-2.5 h-0.5 bg-sky-400"></span><span class="text-slate-400">WSS 信令</span></span>
              <span class="flex items-center space-x-1.5"><span class="w-2.5 h-0.5 bg-emerald-400"></span><span class="text-slate-400">同网段 LAN</span></span>
              <span class="flex items-center space-x-1.5"><span class="w-2.5 h-0.5 bg-indigo-400"></span><span class="text-slate-400">云端流中转</span></span>
            </div>
          </div>
          ${buildTopologySvg(data)}
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm flex flex-col justify-between">
          <div>
            <div class="flex items-center justify-between mb-3">
              <h3 class="text-sm font-semibold text-white flex items-center space-x-2">
                ${
                  data.active_streams.length > 0
                    ? '<span class="w-2 h-2 rounded-full bg-indigo-400 animate-ping"></span><span>正在流式传输 (Stream Teeing)</span>'
                    : '<span class="w-2 h-2 rounded-full bg-sky-400"></span><span>最近流式缓存会话</span>'
                }
              </h3>
              ${featuredSession ? `<span class="text-xs px-2 py-0.5 rounded bg-indigo-500/20 text-indigo-300 font-mono">${escapeHtml(featuredSession.file_cache_id)}</span>` : ''}
            </div>
            ${renderStreamCard(featuredSession, data.system.lan_idle_timeout_seconds)}
          </div>
          <div class="mt-4 pt-3 border-t border-slate-800/80 flex items-center justify-between text-xs">
            <span class="text-slate-500 font-mono">同步更新时间: ${formatRelativeMs(data.timestamp_ms)}</span>
            <button id="overview-goto-stream" class="text-sky-400 hover:text-sky-300 font-medium">查看完整缓存池 →</button>
          </div>
        </div>
      </div>

      <!-- Recent Activity Feed -->
      <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm">
        <div class="flex items-center justify-between mb-4">
          <h3 class="text-sm font-semibold text-white flex items-center space-x-2">
            <svg class="w-4 h-4 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
            <span>实时全链路审计动态流 (SQLite WAL)</span>
          </h3>
          <button id="overview-goto-audit" class="text-xs text-sky-400 hover:underline">查看全部日志</button>
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-left text-xs text-slate-300">
            <thead class="bg-slate-900/80 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
              <tr>
                <th class="py-2.5 px-3">时间</th>
                <th class="py-2.5 px-3">类别</th>
                <th class="py-2.5 px-3">设备</th>
                <th class="py-2.5 px-3">主题 / 行为</th>
                <th class="py-2.5 px-3">摘要 / 状态</th>
                <th class="py-2.5 px-3">通道 / 详情</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-slate-800/60 font-mono">
              ${
                data.recent_activity.length === 0
                  ? '<tr><td colspan="6" class="py-6 text-center text-slate-500 font-sans">暂无审计日志记录</td></tr>'
                  : data.recent_activity.map(renderActivityRow).join('')
              }
            </tbody>
          </table>
        </div>
      </div>
    </div>
  `;

  container.querySelectorAll('.goto-security-btn').forEach((btn) => {
    btn.addEventListener('click', () => handlers.onSwitchTab('security'));
  });
  container.querySelector('#overview-goto-stream')?.addEventListener('click', () => handlers.onSwitchTab('stream'));
  container.querySelector('#overview-goto-audit')?.addEventListener('click', () => handlers.onSwitchTab('audit'));
  container.querySelectorAll('.audit-jump-row').forEach((row) => {
    row.addEventListener('click', () => handlers.onSwitchTab('audit'));
  });
  container.querySelectorAll<HTMLElement>('.topo-device-node').forEach((node) => {
    node.addEventListener('click', () => {
      const id = node.dataset['deviceId'];
      const dev = data.devices.find((d) => d.device_id === id);
      if (dev) handlers.onSelectDevice(dev);
    });
  });
}
