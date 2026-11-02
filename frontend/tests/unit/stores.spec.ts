import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useTimeRangeStore } from '@/stores/timeRange'
import { EMPTY_FILTERS, useFiltersStore } from '@/stores/filters'
import { useCategoriesStore } from '@/stores/categories'
import { useThemeStore } from '@/stores/theme'
import { useWarningsStore, warningKey } from '@/stores/warnings'
import { useAuthStore } from '@/stores/auth'
import { useLiveStore } from '@/stores/live'
import { ApiError } from '@/api/client'
import * as configApi from '@/api/config'
import * as systemApi from '@/api/system'
import * as clientApi from '@/api/client'
import { MockEventSource } from './eventSourceMock'

const categories = [
  { id: 0, name: 'Other', color: '#888888', description: '', builtin: true },
  { id: 1, name: 'Streaming', color: '#ff0000', description: '', builtin: true },
]

beforeEach(() => {
  setActivePinia(createPinia())
  vi.stubGlobal('EventSource', MockEventSource)
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('timeRange store', () => {
  it('defaults to 15m relative range', () => {
    const s = useTimeRangeStore()
    expect(s.params).toEqual({ from: 'now-15m', to: 'now' })
    expect(s.isLive).toBe(true)
    expect(s.label).toBe('Last 15 min')
  })
  it('switches presets', () => {
    const s = useTimeRangeStore()
    s.setPreset('7d')
    expect(s.params).toEqual({ from: 'now-7d', to: 'now' })
  })
  it('accepts a valid custom range as RFC3339', () => {
    const s = useTimeRangeStore()
    s.setCustom(Date.UTC(2024, 0, 1), Date.UTC(2024, 0, 2))
    expect(s.params).toEqual({ from: '2024-01-01T00:00:00.000Z', to: '2024-01-02T00:00:00.000Z' })
    expect(s.isLive).toBe(false)
    expect(s.label).toBe('Custom range')
  })
  it('rejects an inverted custom range', () => {
    const s = useTimeRangeStore()
    expect(() => s.setCustom(10, 5)).toThrow(/before its end/)
    expect(s.key).toBe('15m')
  })
})

describe('filters store', () => {
  it('replaces filter objects immutably', () => {
    const s = useFiltersStore()
    const before = s.filters
    s.setFilter('host', ['1.1.1.1', '1.1.1.1', '2.2.2.2'])
    expect(s.filters).not.toBe(before)
    expect(before).toBe(EMPTY_FILTERS)
    expect(s.filters.host).toEqual(['1.1.1.1', '2.2.2.2'])
    const prevHosts = s.filters.host
    s.addValue('host', '3.3.3.3')
    s.addValue('host', '3.3.3.3')
    expect(prevHosts).toEqual(['1.1.1.1', '2.2.2.2'])
    expect(s.filters.host).toEqual(['1.1.1.1', '2.2.2.2', '3.3.3.3'])
    s.removeValue('host', '1.1.1.1')
    expect(s.filters.host).toEqual(['2.2.2.2', '3.3.3.3'])
  })
  it('builds params from non-empty filters only', () => {
    const s = useFiltersStore()
    s.setFilter('category', [1])
    s.setFilter('direction', ['down'])
    expect(s.params).toEqual({ category: [1], direction: ['down'] })
    expect(s.activeCount).toBe(2)
    s.clear()
    expect(s.params).toEqual({})
  })
})

describe('categories store', () => {
  it('loads once, resolves colours and names', async () => {
    const spy = vi.spyOn(configApi, 'fetchCategories').mockResolvedValue(categories)
    const s = useCategoriesStore()
    await Promise.all([s.load(), s.load()])
    await s.load()
    expect(spy).toHaveBeenCalledTimes(1)
    expect(s.colorFor(1)).toBe('#ff0000')
    expect(s.colorFor('1')).toBe('#ff0000')
    expect(s.colorFor(99)).toBe('#8a8f98')
    expect(s.colorFor(null)).toBe('#8a8f98')
    expect(s.nameFor(0)).toBe('Other')
    expect(s.nameFor(42)).toBe('#42')
    expect(s.nameFor(undefined)).toBe('—')
    await s.load(true)
    expect(spy).toHaveBeenCalledTimes(2)
  })
  it('propagates load errors and allows retry', async () => {
    vi.spyOn(configApi, 'fetchCategories')
      .mockRejectedValueOnce(new Error('down'))
      .mockResolvedValueOnce(categories)
    const s = useCategoriesStore()
    await expect(s.load()).rejects.toThrow('down')
    expect(s.loaded).toBe(false)
    await s.load()
    expect(s.items).toHaveLength(2)
  })
})

describe('theme store', () => {
  it('toggles and persists', async () => {
    const s = useThemeStore()
    const start = s.mode
    s.toggle()
    await nextTick()
    expect(s.mode).not.toBe(start)
    expect(localStorage.getItem('tmon.theme')).toBe(s.mode)
  })
  it('reads a stored preference', () => {
    localStorage.setItem('tmon.theme', 'dark')
    expect(useThemeStore().mode).toBe('dark')
  })
})

describe('warnings store', () => {
  const status = (
    warnings: { code: string; message: string }[],
  ): Awaited<ReturnType<typeof systemApi.fetchStatus>> =>
    ({ warnings }) as unknown as Awaited<ReturnType<typeof systemApi.fetchStatus>>

  it('merges polled and live warnings, and dismisses', async () => {
    vi.useFakeTimers()
    const spy = vi
      .spyOn(systemApi, 'fetchStatus')
      .mockResolvedValue(status([{ code: 'stalled', message: 'No flows from 1.2.3.4' }]))
    const s = useWarningsStore()
    s.start()
    s.start()
    await vi.waitFor(() => expect(s.all).toHaveLength(1))
    useLiveStore().start()
    MockEventSource.latest().emit('warning', { code: 'router', message: 'Router unreachable' })
    expect(s.visible.map((w) => w.code)).toEqual(['stalled', 'router'])
    s.dismiss({ code: 'stalled', message: 'No flows from 1.2.3.4' })
    expect(s.visible.map((w) => w.code)).toEqual(['router'])

    vi.setSystemTime(Date.now() + 60_000)
    spy.mockResolvedValue(status([]))
    await vi.advanceTimersByTimeAsync(30_000)
    expect(spy).toHaveBeenCalledTimes(2)
    expect(s.all).toHaveLength(0)
    s.stop()
    useLiveStore().stop()
  })

  it('records poll errors and accepts pushed status', async () => {
    vi.spyOn(systemApi, 'fetchStatus').mockRejectedValue(new Error('offline'))
    const s = useWarningsStore()
    await s.poll()
    expect(s.lastError).toBe('offline')
    s.setPolled([{ code: 'x', message: 'y' }])
    expect(s.all).toHaveLength(1)
    expect(warningKey({ code: 'x', message: 'y' })).toBe('x|y')
  })
})

describe('auth store', () => {
  it('probes the session and caches categories', async () => {
    vi.spyOn(clientApi, 'request').mockResolvedValue({ data: categories, meta: {} })
    const s = useAuthStore()
    expect(await s.ensure()).toBe(true)
    expect(useCategoriesStore().items).toHaveLength(2)
    expect(await s.ensure()).toBe(true)
  })
  it('reports unauthenticated on 401 but not on other failures', async () => {
    const spy = vi
      .spyOn(clientApi, 'request')
      .mockRejectedValue(new ApiError(401, { code: 'unauthorized', message: '' }))
    const s = useAuthStore()
    expect(await s.ensure()).toBe(false)
    spy.mockRejectedValue(new ApiError(0, { code: 'network_error', message: '' }))
    expect(await s.ensure()).toBe(true)
  })
  it('logs in and out', async () => {
    vi.spyOn(systemApi, 'login').mockResolvedValue()
    vi.spyOn(systemApi, 'logout').mockRejectedValue(new Error('x'))
    const s = useAuthStore()
    await s.login('pw')
    expect(s.authenticated).toBe(true)
    await expect(s.logout()).rejects.toThrow('x')
    expect(s.authenticated).toBe(false)
    s.markLoggedOut()
    expect(s.authenticated).toBe(false)
  })
})
