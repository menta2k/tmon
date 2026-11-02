/**
 * Hand-written API types derived from specs/001-mikrotik-flow-monitor/contracts/http-api.md
 * and data-model.md. `pnpm gen:api` produces `schema.d.ts` from the live OpenAPI document
 * for cross-checking; the app compiles against this file only.
 */

import type { DpiStatus, HostTopApp } from './dpiTypes'
import type { RouterKind } from '@/constants'
export * from './dpiTypes'

export type Direction = 'down' | 'up' | 'internal' | 'transit'
export type CountDirection = 'ingress' | 'egress' | 'both'
export type ExporterProtocol = 'netflow5' | 'netflow9' | 'ipfix' | 'sflow5'
export type DataSource = 'rollup_1m' | 'rollup_1h' | 'raw'

/** Timestamps may be RFC3339 strings or epoch milliseconds; normalise with `toMillis`. */
export type Timestamp = string | number

export interface ApiMeta {
  source?: DataSource
  sampled?: boolean
  page?: number
  limit?: number
  total?: number
}

export interface ApiErrorBody {
  code: string
  message: string
  fields?: Record<string, string>
}

export interface Envelope<T> {
  ok: boolean
  data: T | null
  error: ApiErrorBody | null
  meta?: ApiMeta | null
}

export interface ApiResult<T> {
  data: T
  meta: ApiMeta
}

// ---------- system ----------

export interface ExporterStatus {
  id: number
  address: string
  domain_id: number
  protocol: ExporterProtocol
  name: string | null
  sampling_override: number | null
  count_direction: CountDirection
  first_seen: Timestamp
  last_seen: Timestamp
  state: 'active' | 'stalled' | 'unknown'
  flows_per_sec: number
  sampling_rate: number
  received: number
  decoded: number
  dropped: Record<string, number>
  template_missing: number
  parse_errors: number
}

export interface CollectorStatus {
  proto: string
  port: number
  listening: boolean
}

export interface Warning {
  code: string
  message: string
}

/** `/status.capture` (feature 002). */
export interface CaptureEnabled {
  enabled: true
  listening: boolean
  address: string
  error: string | null
  last_datagram_at: Timestamp | null
  datagrams: {
    received: number
    rejected_source: number
    dropped_backpressure: number
    unsupported: number
    malformed: number
    max_len_15m: number
  }
  connections: { tracked: number; evicted: number; reassembly_bytes: number }
  bindings: { tracked: number; evicted: number }
  names: { tls: number; quic: number; http: number; dns: number; ech: number }
  errors: {
    tls_incomplete: number
    tls_parse: number
    quic_unsupported_version: number
    quic_decrypt: number
    quic_incomplete: number
    dns_parse: number
    name_invalid: number
  }
  named_share_15m: number | null
}

export type CaptureStatus = CaptureEnabled | { enabled: false }

export interface StatusResponse {
  exporters: ExporterStatus[]
  collectors: CollectorStatus[]
  ingest: { flows_per_sec: number; dropped_total: Record<string, number> }
  storage: { disk_bytes: number; cap_bytes: number; oldest_raw: Timestamp | null }
  enrichment: {
    geoip_db_date: string | null
    router_integration: { enabled: boolean; last_ok: Timestamp | null; error?: string | null }
  }
  /** Absent on servers older than feature 002. */
  capture?: CaptureStatus
  /** Feature 003; absent on older servers. */
  dpi?: DpiStatus
  warnings: Warning[]
}

export interface LiveTick {
  ts: Timestamp
  bytes_per_sec: { down: number; up: number }
  by_category: { id: number; bps: number }[]
}

// ---------- analytics ----------

export type GroupBy = 'category' | 'direction' | 'proto' | 'country' | 'exporter'
export type BreakdownBy = 'category' | 'proto' | 'direction' | 'country' | 'asn' | 'rule' | 'app'
/** How sure a categorization is (backend `by=rule` breakdowns). */
export type Confidence = 'detected' | 'inferred' | 'guessed' | 'none'
export type TopKeyedKind = 'asns' | 'countries' | 'ports'

export interface TimeseriesSeries {
  key: string | number
  label: string
  points: [Timestamp, number][]
}

export interface TimeseriesResponse {
  /** Bucket width: seconds (number) or a duration string such as "1m". */
  step: number | string
  series: TimeseriesSeries[]
}

export interface BreakdownItem {
  key: string | number
  label: string
  bytes: number
  packets: number
  flows: number
  share: number
  /** Only for `by=rule`. */
  confidence?: Confidence
  /** Only for `by=app`: share (0..1) of the row's flows classified with confidence. */
  confident_share?: number
}

export interface TopHost {
  ip: string
  name: string | null
  bytes_down: number
  bytes_up: number
  flows: number
  top_category: number | null
}

/** Where a remote host name came from. */
export const NAME_SOURCES = ['tls', 'quic', 'http', 'dns', 'router', 'dpi'] as const
export type NameSource = (typeof NAME_SOURCES)[number]

export interface TopRemote {
  ip: string
  hostname?: string | null
  hostname_source?: NameSource | null
  country?: string | null
  asn?: number | null
  as_name?: string | null
  bytes: number
  flows: number
}

export interface TopKeyedItem {
  key: string | number
  label: string
  bytes: number
  flows: number
}

export interface LocalHost {
  ip: string
  mac: string | null
  friendly_name: string | null
  dhcp_hostname: string | null
  first_seen: Timestamp
  last_seen: Timestamp
}

export interface HostTotals {
  bytes_down: number
  bytes_up: number
  flows: number
  packets?: number
}

export interface HostDetail {
  host: LocalHost
  totals: HostTotals
  categories: BreakdownItem[]
  protocols: BreakdownItem[]
  top_remotes: TopRemote[]
  /** Feature 003: ≤ 10 rows; empty for non-raw ranges or without classification. */
  top_apps?: HostTopApp[]
}

// ---------- raw flows ----------

export interface FlowRecord {
  ts: Timestamp
  ts_start: Timestamp
  exporter_id: number
  in_if: number
  out_if: number
  ip_ver: 4 | 6
  local_ip: string
  remote_ip: string
  local_port: number
  remote_port: number
  proto: number
  tcp_flags: number
  bytes: number
  packets: number
  sampled: boolean
  direction: Direction
  nat_ip: string | null
  nat_port: number | null
  category_id: number
  rule_id: number | null
  country: string | null
  asn: number | null
  as_name: string | null
  remote_host: string | null
  /** How `remote_host` was learned (feature 002); null for older or unnamed flows. */
  remote_host_source?: NameSource | null
  /** Classifier application (feature 003); null when unclassified. */
  app?: string | null
  /** Whether the classifier was confident; null when unclassified. */
  app_confident?: boolean | null
}

/** Flow fields accepted by `POST /rules/test`. */
export type RuleTestFlow = Partial<FlowRecord> & { app_category?: string }

// ---------- configuration ----------

export interface Category {
  id: number
  name: string
  color: string
  description: string
  builtin: boolean
}

export type CategoryInput = Pick<Category, 'name' | 'color' | 'description'>

export type PortSpec = number | string

export interface RuleMatch {
  proto?: number[]
  remote_port?: PortSpec[]
  local_port?: PortSpec[]
  remote_cidr?: string[]
  local_host?: string[]
  asn?: number[]
  country?: string[]
  interface?: number[]
  hostname?: string[]
  direction?: Direction[]
  /** Classifier application names (feature 003). */
  app?: string[]
  /** Classifier application categories (feature 003). */
  app_category?: string[]
}

export interface Rule {
  id: number
  category_id: number
  priority: number
  enabled: boolean
  origin: 'builtin' | 'user'
  name: string
  match: RuleMatch
  hits_24h: number
}

export interface RuleInput {
  name: string
  category_id: number
  enabled: boolean
  match: RuleMatch
}

export interface RuleTestResult {
  category_id: number
  rule_id: number | null
  /** How sure the match is (feature 003); absent on older servers. */
  confidence?: Confidence
}

export type JobId = string | number

export interface Job {
  state: 'queued' | 'running' | 'done' | 'failed'
  progress: number
  error?: string | null
}

export interface HostPatch {
  friendly_name?: string | null
  mac?: string | null
}

export interface LocalNetwork {
  cidr: string
  label: string
}

export interface Exporter {
  id: number
  address: string
  domain_id: number
  protocol: ExporterProtocol
  name: string | null
  sampling_override: number | null
  count_direction: CountDirection
  first_seen: Timestamp
  last_seen: Timestamp
}

export interface ExporterPatch {
  name?: string | null
  sampling_override?: number | null
  count_direction?: CountDirection
}

export interface RetentionSettings {
  raw_days: number
  rollup_1m_days: number
  rollup_1h_days: number
}

export interface RouterSettings {
  /** `routeros` (MikroTik REST) or `openwrt` (rpcd/ubus); older servers omit it. */
  kind?: RouterKind
  enabled: boolean
  url: string
  username: string
  verify_tls: boolean
  lease_poll_secs: number
  dns_poll_secs: number
  /** Read-only: whether a password is stored (the password itself is never returned). */
  password_set?: boolean
}

export interface Settings {
  retention: RetentionSettings
  storage: { disk_cap_gb: number }
  router: RouterSettings
}

export type RouterSettingsPatch = Partial<Omit<RouterSettings, 'password_set'>> & { password?: string }

export interface SettingsPatch {
  retention?: Partial<RetentionSettings>
  storage?: { disk_cap_gb?: number }
  router?: RouterSettingsPatch
}

export interface RouterTestResult {
  reachable: boolean
  error?: string | null
}

export type SettingsBundle = Record<string, unknown>
