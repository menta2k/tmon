// T060 — quickstart V6: top talkers ranking and drill-down with remote enrichment.
import { expect, test } from '@playwright/test'
import { FIXTURES, login, replayFixture, requireReplay, selectTimeRange } from './helpers'

test('top hosts drill-down shows remotes with country/ASN and Local/Private', async ({ page }) => {
  requireReplay()
  await login(page)
  replayFixture(FIXTURES.mixed)
  await page.goto('/hosts/top')
  await selectTimeRange(page, 'Last 1 h')

  const firstRow = page.getByTestId('top-host-row').first()
  await expect(firstRow).toBeVisible({ timeout: 30_000 })
  await firstRow.click()
  await expect(page).toHaveURL(/\/hosts\/[^/]+$/)

  const remotes = page.getByTestId('host-remotes-table')
  await expect(remotes.locator('tbody tr').first()).toBeVisible({ timeout: 30_000 })
  await expect(remotes).toContainText(/AS\d+/)
  await expect(remotes).toContainText(/[\u{1F1E6}-\u{1F1FF}]{2} [A-Z]{2}/u)
  await expect(remotes).toContainText('Local/Private')
})
