import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import DashboardView from '@/views/DashboardView.vue'
import { useTimeRangeStore } from '@/stores/timeRange'
import { useCategoriesStore } from '@/stores/categories'
import { useLiveStore } from '@/stores/live'
import type { ApiMeta, BreakdownItem, TimeseriesResponse } from '@/api/types'
import { mountWithApp, ok } from './helpers'
import { MockEventSource } from './eventSourceMock'

const analytics = vi.hoisted(() => ({
  fetchTimeseries: vi.fn(),
  fetchBreakdown: vi.fn(),
}))
vi.mock('@/api/analytics', () => analytics)
vi.mock('@/api/config', () => ({
  fetchExporters: vi.fn().mockResolvedValue([]),
  fetchCategories: vi.fn().mockResolvedValue([
    { id: 1, name: 'Streaming', color: '#ff0000', description: '', builtin: true },
    { id: 2, name: 'Gaming', color: '#00ff00', description: '', builtin: true },
  ]),
}))

const timeseries: TimeseriesResponse = {
  step: 60,
  series: [
    { key: 1, label: 'Streaming', points: [['2024-01-01T00:00:00Z', 6000]] },
    { key: 2, label: 'Gaming', points: [['2024-01-01T00:00:00Z', 600]] },
  ],
}
const breakdown: BreakdownItem[] = [
  { key: 1, label: 'Streaming', bytes: 6000, packets: 10, flows: 2, share: 0.9 },
  { key: 2, label: 'Gaming', bytes: 600, packets: 4, flows: 1, share: 0.1 },
]

function respond(meta: ApiMeta = {}): void {
  analytics.fetchTimeseries.mockResolvedValue(ok(timeseries, meta))
  analytics.fetchBreakdown.mockResolvedValue(ok(breakdown, meta))
}

beforeEach(() => {
  vi.useRealTimers()
  analytics.fetchTimeseries.mockReset()
  analytics.fetchBreakdown.mockReset()
  MockEventSource.reset()
  vi.stubGlobal('EventSource', MockEventSource)
})
afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

describe('DashboardView', () => {
  it('renders the category series and breakdowns from the API', async () => {
    respond()
    const { wrapper } = await mountWithApp(DashboardView)
    expect(analytics.fetchTimeseries).toHaveBeenCalledWith('category', { from: 'now-15m', to: 'now' })
    expect(analytics.fetchBreakdown.mock.calls.map((c) => c[0]).sort()).toEqual([
      'category',
      'direction',
      'proto',
      'rule',
    ])
    expect(wrapper.find('[data-testid="applications-table"]').exists()).toBe(true)
    const chart = wrapper.find('[data-testid="timeseries-chart"] .vchart-stub')
    expect(chart.exists()).toBe(true)
    const names = (JSON.parse(chart.attributes('data-series') ?? '[]') as { name: string }[]).map(
      (s) => s.name,
    )
    expect(names).toEqual(['Streaming', 'Gaming'])
    expect(wrapper.find('[data-testid="breakdown-category"]').text()).toContain('Streaming')
    expect(wrapper.text()).toContain('6.6 KB')
    wrapper.unmount()
  })

  it('refetches when the time range changes', async () => {
    respond()
    const { wrapper } = await mountWithApp(DashboardView)
    analytics.fetchTimeseries.mockClear()
    analytics.fetchBreakdown.mockClear()
    useTimeRangeStore().setPreset('24h')
    await flushPromises()
    expect(analytics.fetchTimeseries).toHaveBeenCalledWith('category', { from: 'now-24h', to: 'now' })
    expect(analytics.fetchBreakdown).toHaveBeenCalledWith('category', { from: 'now-24h', to: 'now' })
    wrapper.unmount()
  })

  it('applies the direction toggle to the timeseries only', async () => {
    respond()
    const { wrapper } = await mountWithApp(DashboardView)
    analytics.fetchTimeseries.mockClear()
    const radios = wrapper.findAll('[data-testid="direction-toggle"] input[type="radio"]')
    await radios[1]!.setValue(true)
    await flushPromises()
    expect(analytics.fetchTimeseries).toHaveBeenCalledWith('category', {
      from: 'now-15m',
      to: 'now',
      direction: 'down',
    })
    wrapper.unmount()
  })

  it('shows the "estimated (sampled)" badge when meta.sampled is true', async () => {
    respond({ sampled: true, source: 'rollup_1m' })
    const { wrapper } = await mountWithApp(DashboardView)
    const badge = wrapper.find('[data-testid="sampled-badge"]')
    expect(badge.exists()).toBe(true)
    expect(badge.text()).toBe('estimated (sampled)')
    wrapper.unmount()
  })

  it('hides the badge for unsampled data', async () => {
    respond({ sampled: false })
    const { wrapper } = await mountWithApp(DashboardView)
    expect(wrapper.find('[data-testid="sampled-badge"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('uses category colours from the categories store', async () => {
    respond()
    const { wrapper } = await mountWithApp(DashboardView)
    await useCategoriesStore().load()
    await flushPromises()
    const chart = wrapper.find('[data-testid="timeseries-chart"] .vchart-stub')
    const colors = (
      JSON.parse(chart.attributes('data-series') ?? '[]') as { itemStyle: { color: string } }[]
    ).map((s) => s.itemStyle.color)
    expect(colors).toEqual(['#ff0000', '#00ff00'])
    wrapper.unmount()
  })

  it('shows live throughput tiles from the SSE feed', async () => {
    respond()
    const { wrapper } = await mountWithApp(DashboardView)
    const live = useLiveStore()
    live.start()
    MockEventSource.latest().emit('tick', {
      ts: 1,
      bytes_per_sec: { down: 1_250_000, up: 125_000 },
      by_category: [],
    })
    await flushPromises()
    expect(wrapper.find('[data-testid="tile-down"]').text()).toContain('10.0 Mbps')
    expect(wrapper.find('[data-testid="tile-up"]').text()).toContain('1.0 Mbps')
    live.stop()
    wrapper.unmount()
  })

  it('shows a friendly error when the API fails', async () => {
    analytics.fetchTimeseries.mockRejectedValue(new Error('boom'))
    analytics.fetchBreakdown.mockResolvedValue(ok(breakdown))
    const { wrapper } = await mountWithApp(DashboardView)
    expect(wrapper.find('[data-testid="dashboard-error"]').text()).toContain('boom')
    wrapper.unmount()
  })

  it('auto-refreshes every 10 s for relative ranges', async () => {
    vi.useFakeTimers()
    respond()
    const { wrapper } = await mountWithApp(DashboardView)
    const before = analytics.fetchTimeseries.mock.calls.length
    await vi.advanceTimersByTimeAsync(10_000)
    expect(analytics.fetchTimeseries.mock.calls.length).toBe(before + 1)
    wrapper.unmount()
    vi.useRealTimers()
  })
})
