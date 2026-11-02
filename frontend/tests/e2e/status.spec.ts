// T090 — quickstart V10: Status page counters and the stall banner.
// The stall check needs the backend started with TMON_TEST_STALL_SECS (default assumed 10 s here).
import { expect, test } from '@playwright/test'
import { FIXTURES, login, replayFixture, requireReplay } from './helpers'
import { mockApi, statusFixture } from './mockApi'

const STALL_SECS = Number(process.env.TMON_E2E_STALL_SECS ?? '10')
/** Status is polled every 30 s by the banner; allow stall time + one poll + slack. */
const BANNER_TIMEOUT_MS = (STALL_SECS + 45) * 1000

test('status page shows exporter counters and collectors', async ({ page }) => {
  requireReplay()
  await login(page)
  replayFixture(FIXTURES.ipfix)
  await page.goto('/status')
  const exporters = page.getByTestId('exporters-table')
  await expect(exporters.locator('tbody tr').first()).toBeVisible({ timeout: 30_000 })
  await expect(exporters).toContainText('ipfix')
  await expect(page.getByTestId('ingest-fps')).toHaveText(/^\d+(\.\d)?$/)
  await expect(page.getByText('Collectors')).toBeVisible()
  // Feature 002: the capture card is always shown (disabled unless [capture] is configured).
  await expect(page.getByTestId('capture-card')).toBeVisible()
  await expect(page.getByTestId('capture-state')).toHaveText(/^(disabled|listening|not listening)$/)
})

test('stall banner appears when flows stop', async ({ page }) => {
  requireReplay()
  test.skip(process.env.TMON_E2E_SKIP_STALL === '1', 'stall check disabled')
  test.setTimeout(BANNER_TIMEOUT_MS + 120_000)
  await login(page)
  replayFixture(FIXTURES.ipfix)
  await page.goto('/')
  await expect(page.getByTestId('warning-banner')).toContainText(/No flows received from/, {
    timeout: BANNER_TIMEOUT_MS,
  })
})

// T064 — feature 003: the application-classification card (mocked API).
const dpiRunning = {
  enabled: true,
  state: 'running',
  engine: 'nDPI 4.10',
  impl_version: '0.3.0',
  proto_version: 1,
  uptime_secs: 3725,
  restarts_10m: 0,
  restarts_total: 1,
  error: null,
  queue: { sent: 10_000, dropped_queue: 900, dropped_down: 0, skipped_final: 0, drop_share_15m: 0.09 },
  classifier: {
    packets_in: 123_456,
    packets_invalid: 0,
    flows_active: 321,
    classified_confident: 900,
    classified_guess: 100,
    evicted_lru: 0,
    evicted_idle: 0,
    evicted_memory: 0,
    evicted: 0,
    mem_bytes: 1_048_576,
    mem_limit_bytes: 67_108_864,
  },
  ipc_invalid: 0,
  app_invalid: 0,
  top_apps: [{ app: 'BitTorrent', flows: 12 }],
}

test('status page shows the DPI card and the dropping warning', async ({ page }) => {
  await mockApi(page, {
    '/status': () => ({
      ...statusFixture,
      dpi: dpiRunning,
      warnings: [
        { code: 'dpi_dropping', message: 'The classifier cannot keep up; 9% of packets were skipped.' },
      ],
    }),
  })
  await page.goto('/status')
  const card = page.getByTestId('dpi-card')
  await expect(card).toBeVisible()
  await expect(page.getByTestId('dpi-state')).toHaveText('running')
  await expect(page.getByTestId('dpi-engine')).toContainText('nDPI 4.10')
  await expect(page.getByTestId('dpi-uptime')).toHaveText('1h 2m')
  await expect(page.getByTestId('dpi-packets')).toContainText('123,456')
  await expect(page.getByTestId('dpi-classified')).toContainText('90%')
  await expect(page.getByTestId('dpi-drops')).toHaveAttribute('data-warn', 'true')
  await expect(page.getByTestId('dpi-restarts')).toContainText('0 / 1')
  await expect(page.getByTestId('dpi-top-apps')).toContainText('BitTorrent')
  const banner = page.getByTestId('warning-banner')
  await expect(banner).toContainText('cannot keep up')
  await expect(banner.getByTestId('warning-guide')).toContainText('docs/dpi.md')
})

test('status page shows DPI as disabled', async ({ page }) => {
  await mockApi(page)
  await page.goto('/status')
  await expect(page.getByTestId('dpi-state')).toHaveText('disabled')
})
