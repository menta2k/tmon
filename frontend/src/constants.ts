/** Application-wide constants. Keep magic values here, not in components. */

export const API_BASE = '/api/v1'
export const LIVE_PATH = `${API_BASE}/live`

/** Refresh / polling intervals (milliseconds). */
export const DASHBOARD_REFRESH_MS = 10_000
export const STATUS_POLL_MS = 30_000
export const STATUS_VIEW_REFRESH_MS = 10_000
export const JOB_POLL_MS = 1_000
export const SEARCH_DEBOUNCE_MS = 300

/** SSE reconnect backoff. */
export const LIVE_BACKOFF_BASE_MS = 1_000
export const LIVE_BACKOFF_MAX_MS = 30_000
/** Number of live ticks kept in memory (5 s each → 10 min). */
export const LIVE_HISTORY_SIZE = 120

/** Paging / list sizes. */
export const TOP_N_DEFAULT = 20
export const TOP_N_OPTIONS = [10, 20, 50, 100] as const
export const TOP_N_MAX = 1000
export const FLOWS_PAGE_SIZE_DEFAULT = 50
export const FLOWS_PAGE_SIZES = [25, 50, 100, 200, 500] as const
export const HOSTS_PAGE_SIZE = 50
export const HOST_DETAIL_TOP_REMOTES = 20
export const FLOWS_EXPORT_MAX_ROWS = 100_000

/** Validation limits (mirror data-model.md). */
export const CATEGORY_NAME_MAX = 40
export const CATEGORY_DESCRIPTION_MAX = 200
export const RULE_NAME_MAX = 80
export const NETWORK_LABEL_MAX = 80
export const HOST_NAME_MAX = 80
export const PORT_MIN = 0
export const PORT_MAX = 65_535
export const PROTO_MAX = 255
export const ASN_MAX = 4_294_967_295
export const IFINDEX_MAX = 4_294_967_295
export const RETENTION_DAYS_MAX = 3650
export const DISK_CAP_GB_MAX = 100_000
export const POLL_SECS_MIN = 5
export const POLL_SECS_MAX = 86_400
export const SAMPLING_MAX = 1_000_000

/** Reserved category id ("Other"). */
export const OTHER_CATEGORY_ID = 0
export const FALLBACK_COLOR = '#8a8f98'

/** Generic palette for non-category series (protocols, directions, countries...). */
export const SERIES_PALETTE = [
  '#3b82f6',
  '#10b981',
  '#f59e0b',
  '#ef4444',
  '#8b5cf6',
  '#06b6d4',
  '#ec4899',
  '#84cc16',
  '#f97316',
  '#64748b',
] as const

export const DIRECTION_COLORS: Readonly<Record<string, string>> = {
  down: '#3b82f6',
  up: '#10b981',
  internal: '#a855f7',
  transit: '#94a3b8',
}

export const PROTOCOL_NAMES: Readonly<Record<number, string>> = {
  1: 'ICMP',
  2: 'IGMP',
  6: 'TCP',
  17: 'UDP',
  41: 'IPv6',
  47: 'GRE',
  50: 'ESP',
  51: 'AH',
  58: 'ICMPv6',
  89: 'OSPF',
  132: 'SCTP',
}

export const DIRECTIONS = ['down', 'up', 'internal', 'transit'] as const
export const DIRECTION_LABELS: Readonly<Record<string, string>> = {
  down: 'Download',
  up: 'Upload',
  internal: 'Internal',
  transit: 'Transit',
}

export const COUNT_DIRECTIONS = ['ingress', 'egress', 'both'] as const

export const LOCAL_PRIVATE_LABEL = 'Local/Private'
export const UNKNOWN_LABEL = 'Unknown'
export const SAMPLED_BADGE_TEXT = 'estimated (sampled)'

/** Storage keys for per-browser conveniences. */
export const THEME_STORAGE_KEY = 'tmon.theme'

/** Responsive breakpoint (px) below which the side nav collapses. */
export const MOBILE_BREAKPOINT_PX = 768

/** Storage gauge colour thresholds (fraction of the disk cap). */
export const STORAGE_WARN_RATIO = 0.8
export const STORAGE_ERROR_RATIO = 0.95

/** Confidence of a categorization: tag text, Naive UI tag type and explanation. */
export const CONFIDENCE_INFO = {
  detected: { label: 'detected', type: 'success', hint: 'Matched on the remote host name' },
  inferred: {
    label: 'inferred',
    type: 'info',
    hint: 'Matched on the remote network (ASN, IP range, country) or device',
  },
  guessed: {
    label: 'guessed',
    type: 'warning',
    hint: 'Matched on port/protocol only — any service could use it',
  },
  none: { label: 'unmatched', type: 'default', hint: 'No rule matched' },
} as const

/** Applications listed on the dashboard. */
export const APPLICATIONS_MAX_ROWS = 10

/** Remote host name sources (feature 002): short tag text and explanation. */
export const NAME_SOURCE_INFO = {
  tls: { label: 'TLS', hint: 'Server name from the TLS ClientHello captured on the router' },
  quic: { label: 'QUIC', hint: 'Server name from the QUIC Initial packet captured on the router' },
  http: { label: 'HTTP', hint: 'Host header of a plain-HTTP request captured on the router' },
  dns: { label: 'DNS', hint: "Name from a DNS answer the device received (captured on the router)" },
  router: { label: 'DNS cache', hint: "Name from the router's DNS cache (may be shared by several sites)" },
  dpi: { label: 'DPI', hint: 'Name reported by the application classifier (nDPI)' },
} as const

/** Optional base URL of the rendered docs (e.g. the repository's docs/ folder). */
const DOCS_BASE_URL: string = import.meta.env.VITE_DOCS_BASE_URL ?? ''

export interface GuideRef {
  label: string
  /** Null when no docs URL is configured: the label is then shown as text. */
  href: string | null
}

const guide = (label: string, path: string): GuideRef => ({
  label,
  href: DOCS_BASE_URL ? `${DOCS_BASE_URL.replace(/\/$/, '')}/${path}` : null,
})

/** Router packet-capture setup (feature 002). */
export const CAPTURE_SETUP_GUIDE = guide(
  'Setup guide: docs/routeros.md → Packet capture',
  'routeros.md#packet-capture-connection-naming',
)

/** Application classification troubleshooting (feature 003). */
export const DPI_TROUBLESHOOTING_GUIDE = guide(
  'Troubleshooting: docs/dpi.md → Troubleshooting',
  'dpi.md#troubleshooting',
)

/** Warning codes that have a setup-guide section. */
export const WARNING_GUIDES: Record<string, GuideRef> = {
  capture_silent: CAPTURE_SETUP_GUIDE,
  capture_large_packets_lost: guide(
    'Setup guide: docs/routeros.md → WSL2 limitation',
    'routeros.md#wsl2-and-large-capture-packets',
  ),
  capture_fastpath_suspected: guide(
    'Setup guide: docs/routeros.md → FastTrack',
    'routeros.md#fasttrack-exemption',
  ),
  dpi_not_found: DPI_TROUBLESHOOTING_GUIDE,
  dpi_incompatible: DPI_TROUBLESHOOTING_GUIDE,
  dpi_failed: DPI_TROUBLESHOOTING_GUIDE,
  dpi_restarting: DPI_TROUBLESHOOTING_GUIDE,
  dpi_dropping: DPI_TROUBLESHOOTING_GUIDE,
}

export const warningGuide = (code: string): GuideRef | null => WARNING_GUIDES[code] ?? null

/** Named-share threshold below which the capture card highlights the value. */
export const CAPTURE_NAMED_SHARE_WARN = 0.5

/** Feature 003 — application classification (nDPI). */
/** Drop share (0..1, last 15 min) above which the DPI card highlights the value. */
export const DPI_DROP_SHARE_WARN = 0.05
/** Accent colour of a status tile whose value crosses a warning threshold. */
export const WARN_ACCENT_COLOR = 'var(--warning-color, #f0a020)'
/** Application / app-category names: 1–48 printable ASCII characters (mirrors the backend). */
export const APP_NAME_MAX = 48
/** Label of the "no application" choice; sent to the API as an empty `app=`. */
export const APP_UNCLASSIFIED_LABEL = 'Unclassified'
/** Maximum entries in a rule's `app` / `app_category` list. */
export const RULE_APP_VALUES_MAX = 64
/** Title of an application tag when the classifier was not confident. */
export const APP_GUESSED_HINT = 'guessed by classifier'
/** Error code returned when an app filter / breakdown hits a non-raw range. */
export const UNSUPPORTED_FILTER_RANGE = 'unsupported_filter_range'

/** DPI supervisor state: tag text and Naive UI tag type. */
export const DPI_STATE_INFO = {
  disabled: { label: 'disabled', type: 'default' },
  not_found: { label: 'not found', type: 'error' },
  starting: { label: 'starting', type: 'info' },
  running: { label: 'running', type: 'success' },
  restarting: { label: 'restarting', type: 'warning' },
  failed: { label: 'failed', type: 'error' },
  incompatible: { label: 'incompatible', type: 'error' },
} as const

/** Router integration types (`router.kind`). */
export type RouterKind = 'routeros' | 'openwrt'
export const ROUTER_KINDS: readonly { label: string; value: RouterKind }[] = [
  { label: 'MikroTik RouterOS (REST)', value: 'routeros' },
  { label: 'OpenWrt (rpcd / ubus)', value: 'openwrt' },
]
/** URL and user placeholders per router type. */
export const ROUTER_PLACEHOLDERS: Readonly<Record<RouterKind, { url: string; user: string }>> = {
  routeros: { url: 'https://192.168.88.1', user: 'tmon-ro' },
  openwrt: { url: 'http://192.168.1.1', user: 'tmon' },
}
