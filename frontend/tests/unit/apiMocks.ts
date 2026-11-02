/** Shared vi.fn() API doubles plus realistic fixtures for view tests. */
import { vi } from 'vitest'
import type {
  AppCatalogEntry,
  Category,
  DpiEnabled,
  ExporterStatus,
  FlowRecord,
  HostDetail,
  LocalHost,
  Rule,
  Settings,
  StatusResponse,
  TopHost,
  TopKeyedItem,
  TopRemote,
} from '@/api/types'

export const categories: Category[] = [
  { id: 0, name: 'Other', color: '#888888', description: 'fallback', builtin: true },
  { id: 1, name: 'Streaming', color: '#ff0000', description: '', builtin: true },
  { id: 7, name: 'Gaming', color: '#00ff00', description: 'games', builtin: false },
]

export const rules: Rule[] = [
  {
    id: 1,
    category_id: 1,
    priority: 100,
    enabled: true,
    origin: 'builtin',
    name: 'Netflix',
    match: { asn: [2906] },
    hits_24h: 12,
  },
  {
    id: 10,
    category_id: 7,
    priority: 1_000_002,
    enabled: true,
    origin: 'user',
    name: 'Steam',
    match: { proto: [17], remote_port: ['27015-27030'] },
    hits_24h: 3,
  },
  {
    id: 11,
    category_id: 7,
    priority: 1_000_001,
    enabled: false,
    origin: 'user',
    name: 'Other games',
    match: { remote_port: [3074] },
    hits_24h: 0,
  },
]

export const topHosts: TopHost[] = [
  { ip: '192.168.88.10', name: 'laptop', bytes_down: 5000, bytes_up: 100, flows: 10, top_category: 1 },
  { ip: '192.168.88.20', name: null, bytes_down: 9000, bytes_up: 900, flows: 4, top_category: null },
]

export const remotes: TopRemote[] = [
  {
    ip: '8.8.8.8',
    hostname: 'dns.google',
    hostname_source: 'router',
    country: 'US',
    asn: 15169,
    as_name: 'GOOGLE',
    bytes: 3000,
    flows: 3,
  },
  { ip: '192.168.88.1', hostname: null, country: null, asn: null, as_name: null, bytes: 100, flows: 1 },
]

export const countries: TopKeyedItem[] = [{ key: 'US', label: 'United States', bytes: 3000, flows: 3 }]

export const localHost: LocalHost = {
  ip: '192.168.88.10',
  mac: 'aa:bb:cc:dd:ee:ff',
  friendly_name: 'laptop',
  dhcp_hostname: 'johns-laptop',
  first_seen: '2024-01-01T00:00:00Z',
  last_seen: '2024-01-02T00:00:00Z',
}

export const hostDetail: HostDetail = {
  host: localHost,
  totals: { bytes_down: 5000, bytes_up: 100, flows: 10 },
  categories: [{ key: 1, label: 'Streaming', bytes: 5000, packets: 5, flows: 5, share: 1 }],
  protocols: [{ key: 6, label: 'TCP', bytes: 5000, packets: 5, flows: 5, share: 1 }],
  top_remotes: remotes,
  top_apps: [
    { app: 'YouTube', bytes: 4000, flows: 4 },
    { app: 'BitTorrent', bytes: 1000, flows: 1 },
  ],
}

export const catalog: AppCatalogEntry[] = [
  { app: 'BitTorrent', category: 'Download' },
  { app: 'Steam', category: 'Game' },
  { app: 'YouTube', category: 'Media' },
]

export const dpiRunning: DpiEnabled = {
  enabled: true,
  state: 'running',
  engine: 'nDPI 4.10',
  impl_version: '0.3.0',
  proto_version: 1,
  uptime_secs: 3725,
  restarts_10m: 0,
  restarts_total: 1,
  error: null,
  queue: { sent: 10_000, dropped_queue: 10, dropped_down: 0, skipped_final: 5, drop_share_15m: 0.01 },
  classifier: {
    packets_in: 123_456,
    packets_invalid: 2,
    flows_active: 321,
    classified_confident: 900,
    classified_guess: 100,
    evicted_lru: 1,
    evicted_idle: 2,
    evicted_memory: 0,
    evicted: 3,
    mem_bytes: 1_048_576,
    mem_limit_bytes: 67_108_864,
  },
  ipc_invalid: 0,
  app_invalid: 0,
  top_apps: [
    { app: 'YouTube', flows: 40 },
    { app: 'BitTorrent', flows: 12 },
  ],
}

export const flow: FlowRecord = {
  ts: '2024-01-01T00:00:05Z',
  ts_start: '2024-01-01T00:00:00Z',
  exporter_id: 1,
  in_if: 1,
  out_if: 2,
  ip_ver: 4,
  local_ip: '192.168.88.10',
  remote_ip: '8.8.8.8',
  local_port: 50000,
  remote_port: 443,
  proto: 6,
  tcp_flags: 0,
  bytes: 1500,
  packets: 2,
  sampled: false,
  direction: 'down',
  nat_ip: null,
  nat_port: null,
  category_id: 1,
  rule_id: 1,
  country: 'US',
  asn: 15169,
  as_name: 'GOOGLE',
  remote_host: 'dns.google',
  remote_host_source: 'tls',
  app: 'YouTube',
  app_confident: true,
}

export const exporter: ExporterStatus = {
  id: 1,
  address: '192.168.88.1',
  domain_id: 0,
  protocol: 'ipfix',
  name: null,
  sampling_override: null,
  count_direction: 'both',
  first_seen: '2024-01-01T00:00:00Z',
  last_seen: '2024-01-01T00:00:00Z',
  state: 'active',
  flows_per_sec: 12.5,
  sampling_rate: 1,
  received: 1000,
  decoded: 1000,
  dropped: { not_allowed: 2 },
  template_missing: 3,
  parse_errors: 0,
}

export const status: StatusResponse = {
  exporters: [
    exporter,
    { ...exporter, id: 2, address: '10.0.0.1', state: 'stalled', sampling_override: 512, name: 'switch' },
  ],
  collectors: [
    { proto: 'netflow', port: 2055, listening: true },
    { proto: 'sflow', port: 6343, listening: false },
  ],
  ingest: { flows_per_sec: 12.5, dropped_total: { backpressure: 0, not_allowed: 2 } },
  storage: { disk_bytes: 9e9, cap_bytes: 10e9, oldest_raw: '2024-01-01T00:00:00Z' },
  enrichment: {
    geoip_db_date: '2024-01-01',
    router_integration: { enabled: true, last_ok: null, error: 'timeout' },
  },
  capture: { enabled: false },
  dpi: { enabled: false },
  warnings: [{ code: 'stalled', message: 'No flows received from 10.0.0.1 for 5 min' }],
}

export const settings: Settings = {
  retention: { raw_days: 7, rollup_1m_days: 30, rollup_1h_days: 365 },
  storage: { disk_cap_gb: 20 },
  router: {
    kind: 'routeros',
    enabled: false,
    url: 'https://192.168.88.1',
    username: 'tmon-ro',
    verify_tls: true,
    lease_poll_secs: 60,
    dns_poll_secs: 30,
    password_set: true,
  },
}

export const analytics = {
  fetchTimeseries: vi.fn(),
  fetchBreakdown: vi.fn(),
  fetchTopHosts: vi.fn(),
  fetchTopRemotes: vi.fn(),
  fetchTopKeyed: vi.fn(),
  fetchHostDetail: vi.fn(),
  fetchFlows: vi.fn(),
  flowsExportUrl: vi.fn(() => '/api/v1/flows/export.csv'),
}

export const config = {
  fetchCategories: vi.fn(),
  createCategory: vi.fn(),
  updateCategory: vi.fn(),
  deleteCategory: vi.fn(),
  fetchRules: vi.fn(),
  createRule: vi.fn(),
  updateRule: vi.fn(),
  deleteRule: vi.fn(),
  reorderRules: vi.fn(),
  testRule: vi.fn(),
  reapplyRules: vi.fn(),
  fetchJob: vi.fn(),
  fetchHosts: vi.fn(),
  updateHost: vi.fn(),
  fetchNetworks: vi.fn(),
  saveNetworks: vi.fn(),
  fetchExporters: vi.fn(),
  updateExporter: vi.fn(),
  fetchSettings: vi.fn(),
  updateSettings: vi.fn(),
  exportSettings: vi.fn(),
  importSettings: vi.fn(),
  testRouterConnection: vi.fn(),
}

export const applications = {
  fetchAppCatalog: vi.fn(),
}

export const system = {
  login: vi.fn(),
  logout: vi.fn(),
  fetchStatus: vi.fn(),
  fetchStatusWithMeta: vi.fn(),
}

/** Resets every double to a sensible "happy path" default. */
export function resetApiMocks(): void {
  for (const fn of [
    ...Object.values(analytics),
    ...Object.values(config),
    ...Object.values(system),
    ...Object.values(applications),
  ])
    fn.mockReset()
  const res = <T>(data: T, meta = {}) => Promise.resolve({ data, meta })
  analytics.fetchTimeseries.mockImplementation(() => res({ step: 60, series: [] }))
  analytics.fetchBreakdown.mockImplementation(() => res([]))
  analytics.fetchTopHosts.mockImplementation(() => res(topHosts, { sampled: true }))
  analytics.fetchTopRemotes.mockImplementation(() => res(remotes))
  analytics.fetchTopKeyed.mockImplementation(() => res(countries))
  analytics.fetchHostDetail.mockImplementation(() => res(hostDetail))
  analytics.fetchFlows.mockImplementation(() => res([flow], { total: 120, page: 1, limit: 50 }))
  analytics.flowsExportUrl.mockImplementation(() => '/api/v1/flows/export.csv')
  config.fetchCategories.mockResolvedValue(categories)
  config.fetchRules.mockResolvedValue(rules)
  config.fetchExporters.mockResolvedValue([exporter, { ...exporter, id: 2, address: '10.0.0.1' }])
  config.fetchHosts.mockImplementation(() => res([localHost], { total: 1 }))
  config.fetchNetworks.mockResolvedValue([{ cidr: '192.168.88.0/24', label: 'LAN' }])
  config.fetchSettings.mockResolvedValue(settings)
  config.updateSettings.mockResolvedValue(settings)
  config.testRule.mockResolvedValue({ category_id: 7, rule_id: 10, confidence: 'inferred' })
  applications.fetchAppCatalog.mockResolvedValue(catalog)
  config.reapplyRules.mockResolvedValue({ job_id: 'job-1' })
  config.fetchJob.mockResolvedValue({ state: 'done', progress: 1 })
  config.exportSettings.mockResolvedValue({ categories: [] })
  system.fetchStatus.mockResolvedValue(status)
  system.fetchStatusWithMeta.mockImplementation(() => res(status))
}
