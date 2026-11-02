// T049 — the dashboard "By rule / By app" applications toggle (feature 003).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { h } from 'vue'
import { NConfigProvider } from 'naive-ui'
import { analytics, dpiRunning, resetApiMocks, status, system } from './apiMocks'
import { mountWithApp, ok } from './helpers'
import { MockEventSource } from './eventSourceMock'
import DashboardView from '@/views/DashboardView.vue'
import ApplicationsTable from '@/components/ApplicationsTable.vue'
import { ApiError } from '@/api/client'
import type { BreakdownBy, BreakdownItem } from '@/api/types'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)
vi.mock('@/api/applications', async () => (await import('./apiMocks')).applications)

const appRows: BreakdownItem[] = [
  { key: 'YouTube', label: 'YouTube', bytes: 8000, packets: 8, flows: 4, share: 0.8, confident_share: 1 },
  { key: '', label: 'Unclassified', bytes: 2000, packets: 2, flows: 2, share: 0.2, confident_share: 0 },
]

beforeEach(() => {
  resetApiMocks()
  vi.stubGlobal('EventSource', MockEventSource)
  analytics.fetchBreakdown.mockImplementation((by: BreakdownBy) =>
    Promise.resolve(ok(by === 'app' ? appRows : [])),
  )
})
afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

async function switchToApps(wrapper: VueWrapper): Promise<void> {
  const radios = wrapper.findAll('[data-testid="applications-mode"] input[type="radio"]')
  await radios[1]!.setValue(true)
  await flushPromises()
}

const appCalls = (): unknown[][] => analytics.fetchBreakdown.mock.calls.filter((c) => c[0] === 'app')

describe('Dashboard applications toggle', () => {
  it('starts in by-rule mode without requesting by=app', async () => {
    const { wrapper } = await mountWithApp(DashboardView)
    expect(appCalls()).toHaveLength(0)
    expect(wrapper.find('[data-testid="confidence-tag"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('calls by=app and shows the confident share', async () => {
    system.fetchStatus.mockResolvedValue({ ...status, dpi: dpiRunning })
    const { wrapper } = await mountWithApp(DashboardView)
    await switchToApps(wrapper)
    expect(appCalls()).toEqual([['app', { from: 'now-15m', to: 'now' }]])
    const table = wrapper.find('[data-testid="applications-table"]')
    expect(table.text()).toContain('YouTube')
    expect(table.text()).toContain('Unclassified')
    expect(table.text()).toContain('100%')
    expect(wrapper.find('[data-testid="dpi-disabled-hint"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('hints that classification is required when DPI is disabled', async () => {
    const { wrapper } = await mountWithApp(DashboardView)
    await switchToApps(wrapper)
    expect(wrapper.find('[data-testid="dpi-disabled-hint"]').text()).toContain(
      'requires application classification',
    )
    wrapper.unmount()
  })

  it('explains an unsupported range instead of failing', async () => {
    system.fetchStatus.mockResolvedValue({ ...status, dpi: dpiRunning })
    analytics.fetchBreakdown.mockImplementation((by: BreakdownBy) =>
      by === 'app'
        ? Promise.reject(new ApiError(422, { code: 'unsupported_filter_range', message: 'raw data only' }))
        : Promise.resolve(ok([])),
    )
    const { wrapper } = await mountWithApp(DashboardView)
    await switchToApps(wrapper)
    expect(wrapper.find('[data-testid="app-range-unsupported"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="dashboard-error"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('shows other by-app errors', async () => {
    analytics.fetchBreakdown.mockImplementation((by: BreakdownBy) =>
      by === 'app' ? Promise.reject(new Error('boom')) : Promise.resolve(ok([])),
    )
    const { wrapper } = await mountWithApp(DashboardView)
    await switchToApps(wrapper)
    expect(wrapper.find('[data-testid="dashboard-error"]').text()).toContain('boom')
    wrapper.unmount()
  })
})

describe('ApplicationsTable by-app mode', () => {
  it('renders the confident share instead of confidence tags', () => {
    const w = mount({
      render: () => h(NConfigProvider, null, () => h(ApplicationsTable, { items: appRows, mode: 'app' })),
    })
    expect(w.findAll('[data-testid="confidence-tag"]')).toHaveLength(0)
    expect(w.findAll('[data-testid="confident-share"]').map((c) => c.text())).toEqual(['100%', '0%'])
    expect(w.find('[data-testid="guessed-summary"]').exists()).toBe(false)
  })
})
