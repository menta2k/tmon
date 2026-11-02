// T046 — quickstart V2: ingest IPFIX, dashboard shows the category breakdown within 30 s, range switching works.
import { expect, test } from '@playwright/test'
import { FIXTURES, login, replayFixture, requireReplay, selectTimeRange } from './helpers'

test.describe('dashboard', () => {
  test('shows categorized traffic after replay and switches time range', async ({ page }) => {
    requireReplay()
    await login(page)
    replayFixture(FIXTURES.ipfix)
    await page.goto('/')

    const categoryBreakdown = page.getByTestId('breakdown-category')
    await expect(categoryBreakdown.locator('tbody tr').first()).toBeVisible({ timeout: 30_000 })
    await expect(page.getByTestId('timeseries-chart').locator('canvas')).toBeVisible()

    const request = page.waitForRequest(
      (r) => r.url().includes('/api/v1/traffic/timeseries') && r.url().includes('from=now-1h'),
    )
    await selectTimeRange(page, 'Last 1 h')
    await request
    await expect(categoryBreakdown.locator('tbody tr').first()).toBeVisible()

    const dayRequest = page.waitForRequest(
      (r) => r.url().includes('/api/v1/traffic/breakdown') && r.url().includes('from=now-24h'),
    )
    await selectTimeRange(page, 'Last 24 h')
    await dayRequest
  })

  test('shows the sampled badge for sFlow 1:512 data', async ({ page }) => {
    requireReplay()
    await login(page)
    replayFixture(FIXTURES.sflow)
    await page.goto('/')
    await expect(page.getByTestId('sampled-badge').first()).toHaveText('estimated (sampled)', {
      timeout: 30_000,
    })
  })
})
