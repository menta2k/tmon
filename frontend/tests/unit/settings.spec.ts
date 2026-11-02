import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { config, resetApiMocks, settings } from './apiMocks'
import { mountWithApp } from './helpers'
import { NSelect } from 'naive-ui'
import { ROUTER_KINDS } from '@/constants'
import SettingsView from '@/views/SettingsView.vue'
import { ApiError } from '@/api/client'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)

const body = (): string => document.body.textContent ?? ''

beforeEach(() => {
  resetApiMocks()
})
afterEach(() => {
  document.body.innerHTML = ''
})

describe('SettingsView', () => {
  it('edits and saves local networks with CIDR validation', async () => {
    config.saveNetworks.mockImplementation(async (rows: unknown) => rows)
    const { wrapper } = await mountWithApp(SettingsView)
    const section = (): ReturnType<typeof wrapper.find> => wrapper.find('[data-testid="networks-section"]')
    expect((section().find('input').element as HTMLInputElement).value).toBe('192.168.88.0/24')
    const addBtn = section()
      .findAll('button')
      .find((b) => b.text() === 'Add network')!
    await addBtn.trigger('click')
    const cidrInputs = section()
      .findAll('.net-row')
      .map((r) => r.findAll('input')[0]!)
    await cidrInputs[1]!.setValue('10.0.0.0/40')
    expect(section().text()).toContain('not a valid CIDR')
    const saveBtn = (): ReturnType<typeof wrapper.find> =>
      section()
        .findAll('button')
        .find((b) => b.text() === 'Save networks')!
    expect(saveBtn().attributes('disabled')).toBeDefined()
    await cidrInputs[1]!.setValue('192.168.88.0/24')
    expect(section().text()).toContain('Duplicate network')
    await cidrInputs[1]!.setValue('10.0.0.0/8')
    await section().findAll('.net-row')[1]!.findAll('input')[1]!.setValue('VPN')
    await saveBtn().trigger('click')
    await flushPromises()
    expect(config.saveNetworks).toHaveBeenCalledWith([
      { cidr: '192.168.88.0/24', label: 'LAN' },
      { cidr: '10.0.0.0/8', label: 'VPN' },
    ])
    const removeBtns = section()
      .findAll('button')
      .filter((b) => b.text() === '✕')
    await removeBtns[1]!.trigger('click')
    expect(section().findAll('.net-row')).toHaveLength(1)
    wrapper.unmount()
  })

  it('saves retention and disk cap', async () => {
    const { wrapper } = await mountWithApp(SettingsView)
    const form = wrapper.find('[data-testid="retention-section"]')
    const inputs = form.findAll('input')
    expect(inputs.map((i) => (i.element as HTMLInputElement).value)).toEqual(['7', '30', '365', '20'])
    await inputs[0]!.setValue('3')
    await inputs[0]!.trigger('blur')
    await form.trigger('submit')
    await flushPromises()
    expect(config.updateSettings).toHaveBeenCalledWith({
      retention: { raw_days: 3, rollup_1m_days: 30, rollup_1h_days: 365 },
      storage: { disk_cap_gb: 20 },
    })
    wrapper.unmount()
  })

  it('switches the router type to OpenWrt and saves it as kind', async () => {
    const { wrapper } = await mountWithApp(SettingsView)
    const form = wrapper.find('[data-testid="router-section"]')
    const select = wrapper
      .findAllComponents(NSelect)
      .find((c) => c.attributes('data-testid') === 'router-kind')!
    expect(select.props('value')).toBe('routeros')
    expect((select.props('options') as { label: string; value: string }[]).map((o) => o.value)).toEqual(
      ROUTER_KINDS.map((k) => k.value),
    )
    expect(form.text()).toContain('DNS cache poll')
    select.vm.$emit('update:value', 'openwrt')
    await flushPromises()
    expect(form.text()).not.toContain('DNS cache poll')
    expect(form.text()).toContain('getDHCPLeases')
    expect(form.find('input[placeholder="http://192.168.1.1"]').exists()).toBe(true)
    await form.trigger('submit')
    await flushPromises()
    const patch = config.updateSettings.mock.calls[0]![0] as { router: Record<string, unknown> }
    expect(patch.router).toMatchObject({ kind: 'openwrt' })
    wrapper.unmount()
  })

  it('saves router integration without echoing the password, and tests the connection', async () => {
    config.testRouterConnection.mockResolvedValueOnce({ reachable: false, error: 'TLS handshake failed' })
    config.testRouterConnection.mockResolvedValueOnce({ reachable: true })
    const { wrapper } = await mountWithApp(SettingsView)
    const form = wrapper.find('[data-testid="router-section"]')
    const pw = form.find('input[type="password"]')
    expect(pw.attributes('placeholder')).toContain('unchanged')
    await form.trigger('submit')
    await flushPromises()
    const patch = config.updateSettings.mock.calls[0]![0] as { router: Record<string, unknown> }
    expect(patch.router).not.toHaveProperty('password')
    expect(patch.router).toMatchObject({ url: 'https://192.168.88.1', username: 'tmon-ro', enabled: false })

    await pw.setValue('s3cret')
    await form.trigger('submit')
    await flushPromises()
    expect(
      (config.updateSettings.mock.calls[1]![0] as { router: { password: string } }).router.password,
    ).toBe('s3cret')

    const testBtn = form.findAll('button').find((b) => b.text() === 'Test connection')!
    await testBtn.trigger('click')
    await flushPromises()
    expect(form.text()).toContain('TLS handshake failed')
    await testBtn.trigger('click')
    await flushPromises()
    expect(form.text()).toContain('Router reachable')

    config.testRouterConnection.mockRejectedValueOnce(new Error('offline'))
    await testBtn.trigger('click')
    await flushPromises()
    expect(form.text()).toContain('Connection test: offline')

    await form.findAll('input')[0]!.setValue('not a url')
    expect(form.text()).toContain('URL must start with http')
    wrapper.unmount()
  })

  it('exports and imports the settings bundle with per-field errors', async () => {
    URL.createObjectURL = vi.fn(() => 'blob:x')
    URL.revokeObjectURL = vi.fn()
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    config.importSettings.mockRejectedValueOnce(
      new ApiError(400, {
        code: 'validation_failed',
        message: 'rules[0].match.remote_cidr[0]: invalid CIDR',
        fields: { 'rules[0].match.remote_cidr[0]': 'invalid CIDR' },
      }),
    )
    config.importSettings.mockResolvedValueOnce({})
    const { wrapper } = await mountWithApp(SettingsView)
    await wrapper.find('[data-testid="settings-export"]').trigger('click')
    await flushPromises()
    expect(config.exportSettings).toHaveBeenCalled()
    expect(click).toHaveBeenCalled()

    const fileInput = wrapper.find('[data-testid="settings-import-file"]')
    const upload = async (content: string): Promise<void> => {
      const file = new File([content], 'bundle.json', { type: 'application/json' })
      Object.defineProperty(fileInput.element, 'files', { value: [file], configurable: true })
      await fileInput.trigger('change')
      await vi.waitFor(() => expect(wrapper.vm).toBeTruthy())
      await new Promise((r) => setTimeout(r, 20))
      await flushPromises()
    }
    await upload('not json')
    expect(wrapper.find('[data-testid="import-error"]').text()).toContain('not valid JSON')
    await upload('[1,2]')
    expect(wrapper.find('[data-testid="import-error"]').text()).toContain('does not contain')
    await upload('{"rules":[]}')
    expect(wrapper.find('[data-testid="import-error"]').text()).toContain('invalid CIDR')
    expect(wrapper.text()).toContain('rules[0].match.remote_cidr[0]')
    await upload('{"rules":[]}')
    expect(config.importSettings).toHaveBeenLastCalledWith({ rules: [] })
    expect(wrapper.find('[data-testid="import-error"]').exists()).toBe(false)
    click.mockRestore()
    wrapper.unmount()
  })

  it('shows load errors', async () => {
    config.fetchSettings.mockRejectedValue(new Error('boom'))
    config.fetchNetworks.mockRejectedValue(new Error('nets'))
    const { wrapper } = await mountWithApp(SettingsView)
    expect(wrapper.text()).toContain('Loading settings: boom')
    expect(wrapper.text()).toContain('Loading local networks: nets')
    wrapper.unmount()
  })

  it('reports save failures with a toast', async () => {
    config.updateSettings.mockRejectedValue(new ApiError(409, { code: 'conflict', message: '' }))
    const { wrapper } = await mountWithApp(SettingsView)
    await wrapper.find('[data-testid="retention-section"]').trigger('submit')
    await flushPromises()
    expect(body()).toContain('conflicts with the current state')
    expect(settings.retention.raw_days).toBe(7)
    wrapper.unmount()
  })
})
