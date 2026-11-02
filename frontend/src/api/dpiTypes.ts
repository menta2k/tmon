/**
 * Feature 003 (nDPI application classification) API types, from
 * specs/003-ndpi-app-classification/contracts/http-api.md. Re-exported by `types.ts`.
 */

/** `GET /applications/catalog` row. */
export interface AppCatalogEntry {
  app: string
  category: string
}

/** `GET /hosts/{ip}` → `top_apps[]`. */
export interface HostTopApp {
  app: string
  bytes: number
  flows: number
}

export const DPI_STATES = [
  'disabled',
  'not_found',
  'starting',
  'running',
  'restarting',
  'failed',
  'incompatible',
] as const
export type DpiState = (typeof DPI_STATES)[number]

export interface DpiQueue {
  sent: number
  dropped_queue: number
  dropped_down: number
  skipped_final: number
  /** Share (0..1) of packets not delivered to the classifier over the last 15 min. */
  drop_share_15m: number
}

export interface DpiClassifier {
  packets_in: number
  packets_invalid: number
  flows_active: number
  classified_confident: number
  classified_guess: number
  evicted_lru: number
  evicted_idle: number
  evicted_memory: number
  evicted: number
  mem_bytes: number
  mem_limit_bytes: number
}

/** `/status.dpi` when application classification is configured. */
export interface DpiEnabled {
  enabled: true
  state: DpiState
  engine: string | null
  impl_version: string | null
  proto_version: number | null
  uptime_secs: number | null
  restarts_10m: number
  restarts_total: number
  error: string | null
  queue: DpiQueue
  classifier: DpiClassifier
  ipc_invalid: number
  app_invalid: number
  top_apps: { app: string; flows: number }[]
}

export type DpiStatus = DpiEnabled | { enabled: false }
