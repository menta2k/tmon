// T106 — quickstart V11: raw flow search (filter, paging) and CSV export.
import { readFileSync } from 'node:fs'
import { expect, test } from '@playwright/test'
import {
  FIXTURES,
  addSelectTag,
  apiData,
  login,
  replayFixture,
  requireReplay,
  selectTimeRange,
} from './helpers'
import { mockApi } from './mockApi'

interface Flow {
  local_ip: string
  remote_port: number
}

test('filters raw flows, pages and exports CSV', async ({ page }) => {
  requireReplay()
  await login(page)
  replayFixture(FIXTURES.mixed)

  const sample = await apiData<Flow[]>(page.request.get('/api/v1/flows?from=now-1h&to=now&limit=1'))
  expect(sample.length).toBeGreaterThan(0)
  const { local_ip: host, remote_port: port } = sample[0]!

  await page.goto('/flows')
  await selectTimeRange(page, 'Last 1 h')
  const table = page.getByTestId('flows-table')
  await expect(table.locator('tbody tr').first()).toBeVisible({ timeout: 30_000 })

  await addSelectTag(page, 'filter-host', host)
  await page.getByTestId('flows-port').locator('input').fill(String(port))
  const filtered = page.waitForResponse(
    (r) => r.url().includes('/api/v1/flows?') && r.url().includes(`port=${port}`),
  )
  await page.getByTestId('flows-apply').click()
  await filtered
  const rows = table.locator('tbody tr')
  await expect(rows.first()).toBeVisible()
  const count = await rows.count()
  for (let i = 0; i < count; i += 1) await expect(rows.nth(i)).toContainText(host)

  const next = page.locator('.n-pagination-item--button').last()
  // Naive UI disables pagination buttons via a CSS class, not the `disabled` attribute.
  const nextDisabled = ((await next.getAttribute('class')) ?? '').includes('--disabled')
  if (!nextDisabled) {
    const paged = page.waitForResponse(
      (r) => r.url().includes('/api/v1/flows?') && r.url().includes('page=2'),
    )
    await next.click()
    await paged
  }

  const downloadPromise = page.waitForEvent('download')
  await page.getByTestId('flows-export').click()
  const download = await downloadPromise
  expect(download.url()).toContain('/api/v1/flows/export.csv')
  expect(download.url()).toContain(`port=${port}`)
  const file = await download.path()
  const lines = readFileSync(file, 'utf8').trim().split('\n')
  expect(lines[0]).toMatch(/[a-z_]+(,[a-z_]+)+/)
  expect(lines.length).toBeGreaterThan(1)
})

// T050 — feature 003: filter by classifier application and export the app columns (mocked API).
const appFlow = (port: number, app: string | null, confident: boolean | null) => ({
  ts: '2026-11-01T10:00:05Z',
  ts_start: '2026-11-01T10:00:00Z',
  exporter_id: 1,
  in_if: 1,
  out_if: 2,
  ip_ver: 4,
  local_ip: '192.168.88.10',
  remote_ip: '203.0.113.7',
  local_port: 50000,
  remote_port: port,
  proto: 6,
  tcp_flags: 0,
  bytes: 1500,
  packets: 2,
  sampled: false,
  direction: 'down',
  nat_ip: null,
  nat_port: null,
  category_id: 0,
  rule_id: null,
  country: null,
  asn: null,
  as_name: null,
  remote_host: null,
  remote_host_source: null,
  app,
  app_confident: confident,
})

const APP_FLOWS = [
  appFlow(443, 'YouTube', true),
  appFlow(6881, 'BitTorrent', false),
  appFlow(6882, 'BitTorrent', true),
  appFlow(53, null, null),
]
const CSV_HEADER =
  'ts,local_ip,local_port,remote_ip,remote_port,proto,bytes,packets,remote_host,remote_host_source,app,app_confident'

test('filters flows by application and exports the app columns', async ({ page }) => {
  await mockApi(page, {
    '/applications/catalog': () => [
      { app: 'BitTorrent', category: 'Download' },
      { app: 'YouTube', category: 'Media' },
    ],
    '/flows': (url) => {
      const apps = url.searchParams.getAll('app').map((a) => a.toLowerCase())
      return apps.length === 0
        ? APP_FLOWS
        : APP_FLOWS.filter((f) => f.app && apps.includes(f.app.toLowerCase()))
    },
    '/flows/export.csv': () => ({
      raw: {
        body: `${CSV_HEADER}\n2026-11-01T10:00:05Z,192.168.88.10,50000,203.0.113.7,6881,6,1500,2,,,BitTorrent,false\n`,
        headers: { 'content-type': 'text/csv', 'content-disposition': 'attachment; filename="flows.csv"' },
      },
    }),
  })
  await page.goto('/flows')
  const rows = page.getByTestId('flows-table').locator('tbody tr')
  await expect(rows).toHaveCount(APP_FLOWS.length)
  await expect(page.getByTestId('app-tag')).toHaveCount(3)

  const filtered = page.waitForResponse(
    (r) => r.url().includes('/api/v1/flows?') && r.url().includes('app=BitTorrent'),
  )
  await addSelectTag(page, 'filter-app', 'BitTorrent')
  await filtered
  await expect(rows).toHaveCount(2)
  const tags = page.getByTestId('app-tag')
  await expect(tags).toHaveText(['BitTorrent', 'BitTorrent'])
  await expect(tags.first()).toHaveAttribute('data-confident', 'false')

  // Chromium does not pass `<a download>` navigations through page.route, so record the
  // export URL in the page and fetch it there (a routed request) to check the CSV header.
  await page.evaluate(() => {
    const original = HTMLAnchorElement.prototype.click
    HTMLAnchorElement.prototype.click = function (this: HTMLAnchorElement) {
      if (this.download) {
        ;(window as unknown as { __exportUrl: string }).__exportUrl = this.href
        return
      }
      original.call(this)
    }
  })
  await page.getByTestId('flows-export').click()
  const exportUrl = await page.evaluate(() => (window as unknown as { __exportUrl: string }).__exportUrl)
  expect(exportUrl).toContain('/api/v1/flows/export.csv')
  expect(exportUrl).toContain('app=BitTorrent')
  const csv = await page.evaluate(async (url) => (await fetch(url)).text(), exportUrl)
  const header = csv.split('\n')[0]!.split(',')
  expect(header).toContain('app')
  expect(header.slice(-3)).toEqual(['remote_host_source', 'app', 'app_confident'])
})
