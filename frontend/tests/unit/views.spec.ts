import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { analytics, config, resetApiMocks, system } from './apiMocks'
import { mountWithApp } from './helpers'
import { MockEventSource } from './eventSourceMock'
import TopHostsView from '@/views/TopHostsView.vue'
import HostDetailView from '@/views/HostDetailView.vue'
import RemotesView from '@/views/RemotesView.vue'
import FlowsView from '@/views/FlowsView.vue'
import HostsView from '@/views/HostsView.vue'
import StatusView from '@/views/StatusView.vue'
import LoginView from '@/views/LoginView.vue'
import { useFiltersStore } from '@/stores/filters'
import { useWarningsStore } from '@/stores/warnings'
import { ApiError } from '@/api/client'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)
vi.mock('@/components/charts/worldMap', () => ({ ensureWorldMap: vi.fn().mockResolvedValue(undefined) }))

const body = (): string => document.body.textContent ?? ''

beforeEach(() => {
  resetApiMocks()
  vi.stubGlobal('EventSource', MockEventSource)
})
afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

describe('TopHostsView', () => {
  it('lists hosts, shows the sampled badge and navigates to the drill-down', async () => {
    const { wrapper, router } = await mountWithApp(TopHostsView)
    expect(analytics.fetchTopHosts).toHaveBeenCalledWith({ from: 'now-15m', to: 'now', limit: 20 })
    expect(wrapper.text()).toContain('laptop')
    expect(wrapper.text()).toContain('192.168.88.20')
    expect(wrapper.find('[data-testid="sampled-badge"]').exists()).toBe(true)
    const rows = wrapper.findAll('[data-testid="top-host-row"]')
    expect(rows.length).toBe(2)
    await rows[0]!.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('host-detail')
    wrapper.unmount()
  })

  it('refetches with filters', async () => {
    const { wrapper } = await mountWithApp(TopHostsView)
    useFiltersStore().setFilter('category', [1])
    await flushPromises()
    expect(analytics.fetchTopHosts).toHaveBeenLastCalledWith({
      from: 'now-15m',
      to: 'now',
      category: [1],
      limit: 20,
    })
    wrapper.unmount()
  })
})

describe('HostDetailView', () => {
  it('shows host info, remotes with country/ASN and Local/Private', async () => {
    const { wrapper } = await mountWithApp(HostDetailView, { ip: '192.168.88.10' })
    expect(analytics.fetchHostDetail).toHaveBeenCalledWith('192.168.88.10', { from: 'now-15m', to: 'now' })
    expect(analytics.fetchTimeseries).toHaveBeenCalledWith('category', {
      from: 'now-15m',
      to: 'now',
      host: ['192.168.88.10'],
    })
    const text = wrapper.text()
    expect(text).toContain('laptop')
    expect(text).toContain('johns-laptop')
    expect(text).toContain('dns.google')
    expect(wrapper.find('[data-testid="host-source"][data-source="router"]').exists()).toBe(true)
    expect(text).toContain('AS15169 GOOGLE')
    expect(text).toContain('🇺🇸 US')
    expect(text).toContain('Local/Private')
    wrapper.unmount()
  })
})

describe('RemotesView', () => {
  it('shows remotes and switches to countries with the world map', async () => {
    const { wrapper } = await mountWithApp(RemotesView)
    expect(analytics.fetchTopRemotes).toHaveBeenCalled()
    expect(wrapper.text()).toContain('dns.google')
    expect(wrapper.find('[data-testid="host-source"][data-source="router"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Local/Private')
    const tabs = wrapper.findAll('.n-tabs-tab')
    await tabs.find((t) => t.text() === 'Countries')!.trigger('click')
    await flushPromises()
    expect(analytics.fetchTopKeyed).toHaveBeenCalledWith('countries', {
      from: 'now-15m',
      to: 'now',
      limit: 20,
    })
    expect(wrapper.text()).toContain('United States')
    expect(wrapper.find('[data-testid="world-map"]').exists()).toBe(true)
    for (const name of ['ASNs', 'Ports']) {
      await wrapper
        .findAll('.n-tabs-tab')
        .find((t) => t.text() === name)!
        .trigger('click')
      await flushPromises()
    }
    expect(analytics.fetchTopKeyed).toHaveBeenCalledWith('ports', expect.anything())
    wrapper.unmount()
  })

  it('shows API errors', async () => {
    analytics.fetchTopRemotes.mockRejectedValue(
      new ApiError(422, { code: 'unsupported_filter_range', message: 'range exceeds raw retention' }),
    )
    const { wrapper } = await mountWithApp(RemotesView)
    expect(wrapper.text()).toContain('range exceeds raw retention')
    wrapper.unmount()
  })
})

describe('FlowsView', () => {
  it('lists flows, validates and applies remote IP / port, and exports', async () => {
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    const { wrapper } = await mountWithApp(FlowsView)
    expect(analytics.fetchFlows).toHaveBeenCalledWith({ from: 'now-15m', to: 'now', page: 1, limit: 50 })
    expect(wrapper.text()).toContain('8.8.8.8:443')
    expect(wrapper.text()).toContain('dns.google')
    expect(wrapper.find('[data-testid="host-source"][data-source="tls"]').exists()).toBe(true)

    await wrapper.find('[data-testid="flows-remote-ip"] input').setValue('999.1.1.1')
    expect(wrapper.text()).toContain('not a valid IP')
    await wrapper.find('[data-testid="flows-apply"]').trigger('click')
    await flushPromises()
    expect(analytics.fetchFlows).toHaveBeenCalledTimes(1)

    await wrapper.find('[data-testid="flows-remote-ip"] input').setValue('8.8.8.8')
    await wrapper.find('[data-testid="flows-port"] input').setValue('443')
    await wrapper.find('[data-testid="flows-apply"]').trigger('click')
    await flushPromises()
    expect(analytics.fetchFlows).toHaveBeenLastCalledWith({
      from: 'now-15m',
      to: 'now',
      remote_ip: '8.8.8.8',
      port: 443,
      page: 1,
      limit: 50,
    })

    await wrapper.find('[data-testid="flows-export"]').trigger('click')
    expect(analytics.flowsExportUrl).toHaveBeenCalledWith({
      from: 'now-15m',
      to: 'now',
      remote_ip: '8.8.8.8',
      port: 443,
    })
    expect(click).toHaveBeenCalled()

    const page2 = wrapper.findAll('.n-pagination-item').find((el) => el.text() === '2')
    await page2!.trigger('click')
    await flushPromises()
    expect(analytics.fetchFlows).toHaveBeenLastCalledWith(expect.objectContaining({ page: 2 }))
    click.mockRestore()
    wrapper.unmount()
  })
})

describe('HostsView', () => {
  it('edits a friendly name inline with validation', async () => {
    config.updateHost.mockResolvedValue({
      ...(await config.fetchHosts()).data[0],
      friendly_name: 'NAS',
      mac: '11:22:33:44:55:66',
    })
    const { wrapper } = await mountWithApp(HostsView)
    expect(wrapper.text()).toContain('johns-laptop')
    await wrapper.find('[data-testid="host-edit"]').trigger('click')
    await wrapper.find('[data-testid="host-edit-mac"] input').setValue('zz')
    expect(wrapper.text()).toContain('not a valid MAC')
    await wrapper.find('[data-testid="host-save"]').trigger('click')
    expect(config.updateHost).not.toHaveBeenCalled()
    await wrapper.find('[data-testid="host-edit-mac"] input').setValue('11:22:33:44:55:66')
    await wrapper.find('[data-testid="host-edit-friendly_name"] input').setValue('NAS')
    await wrapper.find('[data-testid="host-save"]').trigger('click')
    await flushPromises()
    expect(config.updateHost).toHaveBeenCalledWith('192.168.88.10', {
      friendly_name: 'NAS',
      mac: '11:22:33:44:55:66',
    })
    expect(wrapper.text()).toContain('NAS')
    wrapper.unmount()
  })

  it('searches with debounce', async () => {
    vi.useFakeTimers()
    const { wrapper } = await mountWithApp(HostsView)
    await wrapper.find('[data-testid="hosts-search"] input').setValue('nas')
    await vi.advanceTimersByTimeAsync(400)
    expect(config.fetchHosts).toHaveBeenLastCalledWith({ page: 1, limit: 50, q: 'nas' })
    vi.useRealTimers()
    wrapper.unmount()
  })

  it('reports save errors', async () => {
    config.updateHost.mockRejectedValue(new ApiError(404, { code: 'not_found', message: 'x' }))
    const { wrapper } = await mountWithApp(HostsView)
    await wrapper.find('[data-testid="host-edit"]').trigger('click')
    await wrapper.find('[data-testid="host-save"]').trigger('click')
    await flushPromises()
    expect(body()).toContain('no longer exists')
    wrapper.unmount()
  })
})

describe('StatusView', () => {
  it('renders exporters, drops, storage and pushes warnings to the banner store', async () => {
    const { wrapper } = await mountWithApp(StatusView)
    const text = wrapper.text()
    expect(text).toContain('192.168.88.1')
    expect(text).toContain('stalled')
    expect(text).toContain('1:512 (override)')
    expect(wrapper.find('[data-testid="drop-not_allowed"]').text()).toBe('2')
    expect(wrapper.find('[data-testid="ingest-fps"]').text()).toBe('12.5')
    expect(text).toContain('timeout')
    expect(wrapper.find('[data-testid="capture-card"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="capture-state"]').text()).toBe('disabled')
    expect(useWarningsStore().all.map((w) => w.code)).toEqual(['stalled'])
    wrapper.unmount()
  })

  it('edits an exporter', async () => {
    config.updateExporter.mockResolvedValue({})
    const { wrapper } = await mountWithApp(StatusView)
    const edit = wrapper.findAll('button').find((b) => b.text() === 'Edit')!
    await edit.trigger('click')
    await flushPromises()
    const input = document.body.querySelector<HTMLInputElement>('.n-modal input')!
    input.value = 'core router'
    input.dispatchEvent(new Event('input'))
    await flushPromises()
    const save = [...document.body.querySelectorAll<HTMLButtonElement>('.n-modal button')].find((b) =>
      b.textContent?.includes('Save'),
    )!
    save.click()
    await flushPromises()
    expect(config.updateExporter).toHaveBeenCalledWith(1, {
      name: 'core router',
      sampling_override: null,
      count_direction: 'both',
    })
    wrapper.unmount()
  })
})

describe('LoginView', () => {
  it('shows wrong password, then logs in and redirects', async () => {
    system.login.mockRejectedValueOnce(new ApiError(401, { code: 'unauthorized', message: 'nope' }))
    system.login.mockResolvedValueOnce(undefined)
    const { wrapper, router } = await mountWithApp(LoginView)
    await router.push('/login?redirect=/hosts')
    await wrapper.find('[data-testid="login-submit"]').trigger('submit')
    await flushPromises()
    expect(wrapper.find('[data-testid="login-error"]').text()).toBe('Enter the password')
    await wrapper.find('[data-testid="login-password"] input').setValue('bad')
    await wrapper.find('form').trigger('submit')
    await flushPromises()
    expect(wrapper.find('[data-testid="login-error"]').text()).toBe('Wrong password')
    await wrapper.find('[data-testid="login-password"] input').setValue('changeme')
    await wrapper.find('form').trigger('submit')
    await flushPromises()
    expect(system.login).toHaveBeenLastCalledWith('changeme')
    expect(router.currentRoute.value.fullPath).toBe('/hosts')
    wrapper.unmount()
  })

  it('reports rate limiting', async () => {
    system.login.mockRejectedValue(new ApiError(429, { code: 'rate_limited', message: '' }))
    const { wrapper } = await mountWithApp(LoginView)
    await wrapper.find('[data-testid="login-password"] input').setValue('x')
    await wrapper.find('form').trigger('submit')
    await flushPromises()
    expect(wrapper.find('[data-testid="login-error"]').text()).toContain('Too many attempts')
    wrapper.unmount()
  })
})
