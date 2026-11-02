// T074 — quickstart V7 + V8: custom Gaming port-range rule, re-apply to history, host rename.
import { expect, test, type Page } from '@playwright/test'
import {
  FIXTURES,
  addSelectTag,
  apiData,
  login,
  replayFixture,
  requireReplay,
  selectTimeRange,
} from './helpers'

interface Category {
  id: number
  name: string
}
interface Rule {
  id: number
  name: string
  hits_24h: number
}

const RULE_NAME = `e2e Steam ${Date.now()}`

async function ensureGamingCategory(page: Page): Promise<Category> {
  const categories = await apiData<Category[]>(page.request.get('/api/v1/categories'))
  const existing = categories.find((c) => c.name === 'Gaming')
  if (existing) return existing
  return apiData<Category>(
    page.request.post('/api/v1/categories', {
      data: { name: 'Gaming', color: '#22c55e', description: 'e2e' },
    }),
  )
}

test.describe.serial('rules and host names', () => {
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage()
    await login(page)
    const rules = await apiData<Rule[]>(page.request.get('/api/v1/rules'))
    for (const r of rules.filter((x) => x.name === RULE_NAME))
      await page.request.delete(`/api/v1/rules/${r.id}`)
    await page.close()
  })

  test('creates the Gaming rule, replays steam.pcap and re-applies to history', async ({ page }) => {
    requireReplay()
    await login(page)
    await ensureGamingCategory(page)

    await page.goto('/rules')
    await page.getByTestId('rule-new').click()
    await page.getByTestId('rule-name').locator('input').fill(RULE_NAME)
    await page.getByTestId('rule-category').click()
    await page
      .locator('.n-base-select-option', { hasText: /^Gaming$/ })
      .first()
      .click()
    await addSelectTag(page, 'rule-match-proto', '17')
    await addSelectTag(page, 'rule-match-remote_port', '27015-27030')
    await page.getByTestId('rule-save').click()
    await expect(page.getByTestId('user-rules-table')).toContainText(RULE_NAME)

    replayFixture(FIXTURES.steam)
    await page.goto('/')
    await selectTimeRange(page, 'Last 1 h')
    await expect(page.getByTestId('breakdown-category')).toContainText('Gaming', { timeout: 30_000 })

    // hits_24h is cached server-side for 15 s; poll until the replayed flows are counted.
    await expect
      .poll(
        async () => {
          const rules = await apiData<Rule[]>(page.request.get('/api/v1/rules'))
          return rules.find((r) => r.name === RULE_NAME)?.hits_24h ?? 0
        },
        { timeout: 45_000, intervals: [2_000] },
      )
      .toBeGreaterThan(0)

    await page.goto('/rules')
    await page.getByTestId('rules-reapply').click()
    await page.locator('.n-popconfirm__action button').last().click()
    await expect(page.getByTestId('reapply-state')).toHaveText('done', { timeout: 120_000 })
  })

  test('renames a host and shows the name in Top Hosts', async ({ page }) => {
    requireReplay()
    await login(page)
    const hosts = await apiData<{ ip: string }[]>(
      page.request.get('/api/v1/top/hosts?from=now-1h&to=now&limit=1'),
    )
    test.skip(hosts.length === 0, 'no hosts with traffic in the last hour')
    const ip = hosts[0]!.ip
    const name = `e2e-host-${Date.now() % 100000}`

    await page.goto('/hosts')
    await page.getByTestId('hosts-search').locator('input').fill(ip)
    const row = page.getByTestId('hosts-table').locator('tbody tr', { hasText: ip }).first()
    await expect(row).toBeVisible()
    await row.getByTestId('host-edit').click()
    await row.getByTestId('host-edit-friendly_name').locator('input').fill(name)
    await row.getByTestId('host-save').click()
    await expect(row).toContainText(name)

    await page.goto('/hosts/top')
    await selectTimeRange(page, 'Last 1 h')
    await expect(page.getByTestId('top-hosts-table')).toContainText(name, { timeout: 30_000 })
  })
})
