/**
 * Mocked-API helpers for E2E specs that must not depend on a live collector (feature 003).
 * Every `/api/v1/**` request is answered in the browser, so these specs run against any
 * server that serves the SPA — the tmon backend or `pnpm dev` (set TMON_E2E_BASE_URL).
 */
import type { Page, Route } from '@playwright/test'

/** A non-JSON answer (CSV, SSE ...). */
export interface RawAnswer {
  raw: { status?: number; body: string; headers?: Record<string, string> }
}
/** A handler returns the envelope `data`, or a `RawAnswer`. */
export type MockHandler = (url: URL) => unknown
export type MockRoutes = Readonly<Record<string, MockHandler>>

const API_PREFIX = '/api/v1'

export const statusFixture = {
  exporters: [],
  collectors: [{ proto: 'netflow', port: 2055, listening: true }],
  ingest: { flows_per_sec: 1, dropped_total: {} },
  storage: { disk_bytes: 1e9, cap_bytes: 1e10, oldest_raw: null },
  enrichment: { geoip_db_date: null, router_integration: { enabled: false, last_ok: null } },
  capture: { enabled: false },
  dpi: { enabled: false },
  warnings: [],
}

const DEFAULT_ROUTES: MockRoutes = {
  '/categories': () => [{ id: 0, name: 'Other', color: '#888888', description: '', builtin: true }],
  '/exporters': () => [],
  '/status': () => statusFixture,
  '/applications/catalog': () => [],
  '/live': () => ({ raw: { body: '', headers: { 'content-type': 'text/event-stream' } } }),
}

function isRaw(v: unknown): v is RawAnswer {
  return typeof v === 'object' && v !== null && 'raw' in v
}

async function answer(route: Route, routes: MockRoutes): Promise<void> {
  const url = new URL(route.request().url())
  const path = url.pathname.slice(API_PREFIX.length)
  const handler = routes[path]
  const result = handler ? handler(url) : []
  if (isRaw(result)) {
    await route.fulfill({
      status: result.raw.status ?? 200,
      body: result.raw.body,
      headers: result.raw.headers,
    })
    return
  }
  await route.fulfill({ json: { ok: true, data: result, error: null, meta: {} } })
}

/** Routes all API calls to `routes` (merged over sensible defaults). Unknown paths get `[]`. */
export async function mockApi(page: Page, routes: MockRoutes = {}): Promise<void> {
  const merged = { ...DEFAULT_ROUTES, ...routes }
  await page.route(`**${API_PREFIX}/**`, (route) => answer(route, merged))
}
