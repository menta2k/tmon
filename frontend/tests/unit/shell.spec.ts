import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { config, resetApiMocks, system } from './apiMocks'
import { mountWithApp } from './helpers'
import { MockEventSource } from './eventSourceMock'
import App from '@/App.vue'
import FilterBar from '@/components/FilterBar.vue'
import TimeRangePicker from '@/components/TimeRangePicker.vue'
import { createAppRouter, routes } from '@/router'
import { useFiltersStore } from '@/stores/filters'
import { useTimeRangeStore } from '@/stores/timeRange'
import { useThemeStore } from '@/stores/theme'
import * as client from '@/api/client'
import { ApiError } from '@/api/client'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)

const body = (): string => document.body.textContent ?? ''

beforeEach(() => {
  resetApiMocks()
  MockEventSource.reset()
  vi.stubGlobal('EventSource', MockEventSource)
})
afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('FilterBar', () => {
  it('loads categories/exporters and validates tag inputs', async () => {
    const { wrapper } = await mountWithApp(FilterBar)
    expect(config.fetchExporters).toHaveBeenCalled()
    const store = useFiltersStore()
    const hostSelect = wrapper.findComponent({ name: 'Select' })
    hostSelect.vm.$emit('update:value', ['192.168.1.1', 'bogus'])
    await flushPromises()
    expect(store.filters.host).toEqual(['192.168.1.1'])
    expect(body()).toContain('not a valid IP')
    const selects = wrapper.findAllComponents({ name: 'Select' })
    selects[1]!.vm.$emit('update:value', [1])
    selects[2]!.vm.$emit('update:value', ['6', 300])
    selects[3]!.vm.$emit('update:value', ['down'])
    selects[4]!.vm.$emit('update:value', ['de', 'xyz'])
    selects[5]!.vm.$emit('update:value', ['15169'])
    selects[6]!.vm.$emit('update:value', [2])
    await flushPromises()
    expect(store.params).toEqual({
      host: ['192.168.1.1'],
      category: [1],
      proto: [6],
      direction: ['down'],
      country: ['DE'],
      asn: [15169],
      exporter: [2],
    })
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Clear filters')!
      .trigger('click')
    expect(store.activeCount).toBe(0)
    wrapper.unmount()
  })

  it('reports exporter load failures', async () => {
    config.fetchExporters.mockRejectedValue(new Error('nope'))
    config.fetchCategories.mockRejectedValue(new Error('cats'))
    const { wrapper } = await mountWithApp(FilterBar)
    expect(body()).toContain('Loading exporters: nope')
    expect(body()).toContain('Loading categories: cats')
    wrapper.unmount()
  })
})

describe('TimeRangePicker', () => {
  it('switches presets and accepts a custom range', async () => {
    const { wrapper } = await mountWithApp(TimeRangePicker)
    const store = useTimeRangeStore()
    const select = wrapper.findComponent({ name: 'Select' })
    select.vm.$emit('update:value', '1h')
    await flushPromises()
    expect(store.params.from).toBe('now-1h')
    select.vm.$emit('update:value', 'custom')
    await flushPromises()
    const picker = wrapper.findComponent({ name: 'DatePicker' })
    expect(picker.exists()).toBe(true)
    picker.vm.$emit('update:value', [2000, 1000])
    await flushPromises()
    expect(body()).toContain('before its end')
    picker.vm.$emit('update:value', [1000, 2000])
    await flushPromises()
    expect(store.key).toBe('custom')
    picker.vm.$emit('update:value', null)
    wrapper.unmount()
  })
})

describe('App shell + router', () => {
  async function mountApp(path: string) {
    const pinia = createPinia()
    setActivePinia(pinia)
    const router = createAppRouter()
    await router.push(path)
    await router.isReady()
    const wrapper = mount(App, { global: { plugins: [pinia, router] }, attachTo: document.body })
    await flushPromises()
    return { wrapper, router }
  }

  it('declares every view route', () => {
    expect(routes.map((r) => r.path)).toEqual(
      expect.arrayContaining([
        '/login',
        '/',
        '/hosts/top',
        '/hosts/:ip',
        '/remotes',
        '/flows',
        '/rules',
        '/hosts',
        '/status',
        '/settings',
      ]),
    )
  })

  it('redirects unauthenticated users to /login', async () => {
    vi.spyOn(client, 'request').mockRejectedValue(new ApiError(401, { code: 'unauthorized', message: '' }))
    const { wrapper, router } = await mountApp('/flows')
    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/flows')
    expect(wrapper.find('[data-testid="login-password"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('renders the shell, live indicator, banner, theme toggle and logout', async () => {
    vi.spyOn(client, 'request').mockResolvedValue({ data: [], meta: {} })
    system.logout.mockResolvedValue(undefined)
    const { wrapper, router } = await mountApp('/status')
    expect(router.currentRoute.value.name).toBe('status')
    expect(document.title).toBe('Status · tmon')
    expect(wrapper.text()).toContain('Collection status')
    await flushPromises()
    expect(wrapper.find('[data-testid="warning-banner"]').text()).toContain('No flows received from 10.0.0.1')
    MockEventSource.latest().emit('open')
    MockEventSource.latest().emit('tick', { ts: 1, bytes_per_sec: { down: 125, up: 0 }, by_category: [] })
    await flushPromises()
    expect(wrapper.find('[data-testid="live-indicator"]').text()).toContain('1.0 Kbps')

    const theme = useThemeStore()
    const before = theme.mode
    await wrapper.find('button[aria-label="Toggle theme"]').trigger('click')
    expect(theme.mode).not.toBe(before)

    await wrapper.find('.n-alert__close').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="warning-banner"]').exists()).toBe(false)

    await wrapper.find('[data-testid="logout"]').trigger('click')
    await flushPromises()
    expect(system.logout).toHaveBeenCalled()
    expect(router.currentRoute.value.name).toBe('login')
    wrapper.unmount()
  })

  it('global 401 handler navigates to login', async () => {
    const spy = vi.spyOn(client, 'request').mockResolvedValue({ data: [], meta: {} })
    const { wrapper, router } = await mountApp('/status')
    spy.mockRestore()
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({ ok: false, data: null, error: { code: 'unauthorized', message: '' } }),
            { status: 401 },
          ),
      ),
    )
    await client.getData('/status').catch(() => undefined)
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('login')
    wrapper.unmount()
  })
})
