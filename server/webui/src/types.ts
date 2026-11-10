export type TabId = 'overview' | 'devices' | 'stream' | 'audit' | 'security' | 'settings';

export type ContentCategory = 'TEXT' | 'IMAGE' | 'FILE' | 'DIRECTORY';

export interface SecurityWarning {
  code: string;
  severity: 'HIGH' | 'MEDIUM' | 'LOW';
  title: string;
  message: string;
}

export interface AuthSessionResponse {
  authenticated: boolean;
  admin_token_is_separate: boolean;
  is_default_cluster_token?: boolean;
  security_warnings?: SecurityWarning[];
  expires_at?: number;
}

export interface RegisteredDevice {
  device_id: string;
  name: string;
  device_name: string;
  os: string;
  lan_endpoint: string | null;
  online: boolean;
  enabled: boolean;
  first_seen_ms: number;
  last_seen_ms: number;
  subscribed_topics: string[];
  allowed_categories: ContentCategory[];
  acl_restricted: boolean;
}

export interface ReaderProgress {
  reader_id: string;
  reader_device_id: string;
  start_offset: number;
  read_offset: number;
  progress_pct: number;
  started_at: number;
  last_read_at: number;
}

export interface CacheSession {
  file_cache_id: string;
  source_device_id: string;
  file_name: string;
  content_category: string;
  total_size: number;
  written_offset: number;
  progress_pct: number;
  state: 'STREAMING' | 'COMPLETED' | 'ABORTED';
  download_count: number;
  created_at: number;
  last_accessed_at: number;
  expires_at: number;
  ttl_remaining_seconds: number;
  chunk_size_bytes: number;
  verified_chunks: number;
  total_chunks: number;
  full_hash: string;
  active_readers: ReaderProgress[];
}

export interface CacheSessionDetail extends CacheSession {
  digest: {
    algorithm: string;
    full_hash: string;
    chunk_size_bytes: number;
    chunk_hashes: string[];
  };
}

export interface CacheQuotaStats {
  cache_dir: string;
  quota_bytes: number;
  used_bytes: number;
  remaining_bytes: number;
  usage_pct: number;
  ttl_seconds: number;
  total_sessions: number;
  streaming_count: number;
  completed_count: number;
  aborted_count: number;
  active_reader_count: number;
}

export interface AuditSummary {
  total_connections: number;
  total_events: number;
  total_transfers: number;
  verified_ok_count: number;
  hash_mismatch_count: number;
  blake3_pass_rate: number;
  resumed_transfer_count: number;
  resumed_bytes_saved: number;
  lan_transfer_count: number;
  cloud_transfer_count: number;
  total_transfer_bytes: number;
}

export interface TransferChannelActivity {
  timestamp_ms: number;
  transfer_id: string;
  file_cache_id: string;
  source_device_id: string;
  target_device_id: string;
  channel_used: string;
  resumed: boolean;
  resume_offset: number;
  resume_count: number;
  bytes_transferred: number;
  verification_status: string;
}

export interface AuditLogItem {
  id: number;
  record_type: 'CONNECTION' | 'EVENT' | 'TRANSFER';
  timestamp_ms: number;
  device_id?: string;
  device_name?: string;
  action?: string;
  lan_endpoint?: string | null;
  status?: string;
  detail?: string | null;
  event_id?: string;
  topic?: string;
  event_type?: string;
  source_device_id?: string;
  target_device_ids?: string[];
  content_category?: string;
  content_hash?: string;
  size_bytes?: number;
  transfer_id?: string;
  file_cache_id?: string;
  target_device_id?: string;
  channel_used?: string;
  resumed?: boolean;
  resume_offset?: number;
  resume_count?: number;
  bytes_transferred?: number;
  verification_status?: string;
}

export interface AuditLogsPagedResponse {
  items: AuditLogItem[];
  total: number;
  total_counts: {
    connection: number;
    event: number;
    transfer: number;
  };
  limit: number;
  offset: number;
  query_ms: number;
}

export interface AccessTokenRecord {
  token_id: string;
  token_prefix: string;
  label: string;
  expected_os: string;
  issued_at_ms: number;
  revoked: boolean;
  revoked_at_ms: number | null;
}

export interface SecurityTokensResponse {
  cluster_token: {
    masked: string;
    length: number;
    plaintext?: string;
  };
  device_tokens: AccessTokenRecord[];
  constant_time_guard: string;
}

export interface IssuedTokenResponse extends AccessTokenRecord {
  token: string;
}

export interface WalDbStats {
  db_path: string;
  journal_mode: string;
  synchronous: string;
  db_size_bytes: number;
  wal_size_bytes: number;
  retention_days: number;
  row_counts: {
    audit_connection_log: number;
    audit_event_log: number;
    audit_transfer_log: number;
  };
}

export interface SystemStatusResponse {
  status: string;
  uptime_seconds: number;
  runtime: {
    python_version: string;
    platform: string;
    fastapi_version: string;
  };
  server: {
    host: string;
    port: number;
    ui_enabled: boolean;
    data_dir: string;
    cache_dir: string;
    state_db_path: string;
  };
  policies: {
    cache_ttl_seconds: number;
    cache_quota_bytes: number;
    cache_cleanup_interval_seconds: number;
    lan_idle_timeout_seconds: number;
    audit_retention_days: number;
    chunk_size_bytes: number;
  };
  wal: WalDbStats;
  security: {
    admin_token_is_separate: boolean;
    is_default_cluster_token: boolean;
    constant_time_compare: boolean;
    warnings: SecurityWarning[];
  };
}

export interface OverviewPayload {
  timestamp_ms: number;
  devices: RegisteredDevice[];
  device_stats: {
    total: number;
    online: number;
    disabled: number;
    online_by_os: {
      windows: number;
      macos: number;
      linux: number;
      other: number;
    };
  };
  lan_subnets: Record<string, string[]>;
  cache_quota: CacheQuotaStats;
  active_streams: CacheSession[];
  recent_sessions: CacheSession[];
  audit_summary: AuditSummary;
  recent_activity: AuditLogItem[];
  channel_activity: TransferChannelActivity[];
  security_warnings: SecurityWarning[];
  system: {
    host: string;
    port: number;
    uptime_seconds: number;
    cache_ttl_seconds: number;
    lan_idle_timeout_seconds: number;
    journal_mode: string;
    synchronous: string;
  };
}

export interface TelemetryFrame {
  type: 'telemetry' | 'ping';
  event?: string;
  payload?: unknown;
  overview?: OverviewPayload;
  ts_ms?: number;
}
