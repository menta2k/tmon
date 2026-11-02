// T049 — feature 003 (application classification): AppTag, validation, catalog, Flows and Host views.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { NSelect } from 'naive-ui'
import { analytics, applications, catalog, flow, hostDetail, resetApiMocks } from './apiMocks'
import { mountWithApp, ok } from './helpers'
import { MockEventSource } from './eventSourceMock'
import AppTag from '@/components/AppTag.vue'
import AppSelect from '@/components/AppSelect.vue'
import FlowsView from '@/views/FlowsView.vue'
import HostDetailView from '@/views/HostDetailView.vue'
import { APP_GUESSED_HINT, APP_NAME_MAX, RULE_APP_VALUES_MAX, APP_UNCLASSIFIED_LABEL } from '@/constants'
import { validateAppName } from '@/utils/validators'
import {
  emptyRuleMatchForm,
  formToRuleMatch,
  RULE_MATCH_FIELDS,
  validateRuleMatchField,
} from '@/utils/ruleMatch'
import { useAppCatalogStore } from '@/stores/appCatalog'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)
vi.mock('@/api/applications', async () => (await import('./apiMocks')).applications)

const body = (): string => document.body.textContent ?? ''

beforeEach(() => {
  resetApiMocks()
  vi.stubGlobal('EventSource', MockEventSource)
})
afterEach(() => {
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

describe('AppTag', () => {
  it('renders a confident application name', () => {
    const tag = mount(AppTag, { props: { app: 'YouTube', confident: true } }).find('[data-testid="app-tag"]')
    expect(tag.text()).toBe('YouTube')
    expect(tag.attributes('data-confident')).toBe('true')
    expect(tag.classes()).not.toContain('app-tag--dashed')
    expect(tag.attributes('title')).toBeUndefined()
  })

  it('uses a dashed style and an explanation when guessed', () => {
    const tag = mount(AppTag, { props: { app: 'BitTorrent', confident: false } }).find(
      '[data-testid="app-tag"]',
    )
    expect(tag.text()).toBe('BitTorrent')
    expect(tag.attributes('data-confident')).toBe('false')
    expect(tag.classes()).toContain('app-tag--dashed')
    expect(tag.attributes('title')).toBe(APP_GUESSED_HINT)
  })

  it('renders nothing for an unclassified flow', () => {
    expect(
      mount(AppTag, { props: { app: null, confident: null } })
        .find('[data-testid="app-tag"]')
        .exists(),
    ).toBe(false)
    expect(mount(AppTag, { props: {} }).find('[data-testid="app-tag"]').exists()).toBe(false)
  })
})

describe('validateAppName', () => {
  it('accepts 1–48 printable ASCII characters', () => {
    expect(validateAppName('BitTorrent')).toBeNull()
    expect(validateAppName('Microsoft 365')).toBeNull()
    expect(validateAppName('x'.repeat(APP_NAME_MAX))).toBeNull()
  })

  it('rejects empty, too long and non-ASCII names', () => {
    expect(validateAppName('')).toContain('1–48')
    expect(validateAppName('   ')).toContain('1–48')
    expect(validateAppName('x'.repeat(APP_NAME_MAX + 1))).toContain('1–48')
    expect(validateAppName('Café')).toContain('printable ASCII')
    expect(validateAppName('a\tb')).toContain('printable ASCII')
  })
})

describe('rule match app fields', () => {
  it('includes app and app_category in the editor form', () => {
    expect(RULE_MATCH_FIELDS).toContain('app')
    expect(RULE_MATCH_FIELDS).toContain('app_category')
  })

  it('validates names and the entry count', () => {
    expect(validateRuleMatchField('app', ['BitTorrent'])).toBeNull()
    expect(validateRuleMatchField('app', ['x'.repeat(49)])).toContain('1–48')
    const many = Array.from({ length: RULE_APP_VALUES_MAX + 1 }, (_, i) => `App${i}`)
    expect(validateRuleMatchField('app_category', many)).toContain(String(RULE_APP_VALUES_MAX))
  })

  it('keeps the case of application names when building the match', () => {
    const form = { ...emptyRuleMatchForm(), app: [' BitTorrent '], app_category: ['VPN'] }
    expect(formToRuleMatch(form)).toEqual({ app: ['BitTorrent'], app_category: ['VPN'] })
  })
})

describe('app catalog store', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('loads once and derives sorted app and category lists', async () => {
    const store = useAppCatalogStore()
    await Promise.all([store.load(), store.load()])
    expect(applications.fetchAppCatalog).toHaveBeenCalledTimes(1)
    expect(store.apps).toEqual(['BitTorrent', 'Steam', 'YouTube'])
    expect(store.categories).toEqual(['Download', 'Game', 'Media'])
  })

  it('records a load failure instead of throwing', async () => {
    applications.fetchAppCatalog.mockRejectedValue(new Error('down'))
    const store = useAppCatalogStore()
    await store.load()
    expect(store.error).toContain('down')
    expect(store.apps).toEqual([])
  })
})

describe('FlowsView application column and filter', () => {
  it('shows an app tag per classified flow', async () => {
    analytics.fetchFlows.mockImplementation(() =>
      Promise.resolve(
        ok(
          [
            flow,
            { ...flow, remote_port: 6881, app: 'BitTorrent', app_confident: false },
            { ...flow, remote_port: 53, app: null, app_confident: null },
          ],
          { total: 3 },
        ),
      ),
    )
    const { wrapper } = await mountWithApp(FlowsView)
    const tags = wrapper.findAll('[data-testid="app-tag"]')
    expect(tags.map((t) => t.text())).toEqual(['YouTube', 'BitTorrent'])
    expect(tags.map((t) => t.attributes('data-confident'))).toEqual(['true', 'false'])
    wrapper.unmount()
  })

  it('offers the catalog and adds app= to the flows query and CSV export', async () => {
    const { wrapper } = await mountWithApp(FlowsView)
    const select = wrapper.findComponent(AppSelect).findComponent(NSelect)
    const labels = (select.props('options') as { label: string }[]).map((o) => o.label)
    expect(labels).toEqual([APP_UNCLASSIFIED_LABEL, ...catalog.map((c) => c.app)])
    expect(select.props('tag')).toBe(true)
    expect(select.props('filterable')).toBe(true)

    select.vm.$emit('update:value', ['BitTorrent', 'Custom App'])
    await flushPromises()
    expect(analytics.fetchFlows).toHaveBeenLastCalledWith(
      expect.objectContaining({ app: ['BitTorrent', 'Custom App'], page: 1 }),
    )
    await wrapper.find('[data-testid="flows-export"]').trigger('click')
    expect(analytics.flowsExportUrl).toHaveBeenLastCalledWith(
      expect.objectContaining({ app: ['BitTorrent', 'Custom App'] }),
    )
    wrapper.unmount()
  })

  it('offers Unclassified first and sends it as an empty app=', async () => {
    const { wrapper } = await mountWithApp(FlowsView)
    const select = wrapper.findComponent(AppSelect).findComponent(NSelect)
    const options = select.props('options') as { label: string; value: string }[]
    expect(options[0]).toEqual({ label: APP_UNCLASSIFIED_LABEL, value: '' })
    select.vm.$emit('update:value', ['', 'BitTorrent'])
    await flushPromises()
    expect(analytics.fetchFlows).toHaveBeenLastCalledWith(
      expect.objectContaining({ app: ['', 'BitTorrent'], page: 1 }),
    )
    wrapper.unmount()
  })

  it('drops invalid app names with an error', async () => {
    const { wrapper } = await mountWithApp(FlowsView)
    analytics.fetchFlows.mockClear()
    const select = wrapper.findComponent(AppSelect).findComponent(NSelect)
    select.vm.$emit('update:value', ['x'.repeat(APP_NAME_MAX + 1)])
    await flushPromises()
    expect(body()).toContain('1–48')
    expect(analytics.fetchFlows).not.toHaveBeenCalled()
    wrapper.unmount()
  })
})

describe('HostDetailView top applications', () => {
  it('lists the host top applications', async () => {
    const { wrapper } = await mountWithApp(HostDetailView, { ip: '192.168.88.10' })
    const card = wrapper.find('[data-testid="host-top-apps"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('YouTube')
    expect(card.text()).toContain('BitTorrent')
    expect(card.text()).toContain('4.0 KB')
    wrapper.unmount()
  })

  it('hides the list when there are no applications', async () => {
    analytics.fetchHostDetail.mockImplementation(() => Promise.resolve(ok({ ...hostDetail, top_apps: [] })))
    const { wrapper } = await mountWithApp(HostDetailView, { ip: '192.168.88.10' })
    expect(wrapper.find('[data-testid="host-top-apps"]').exists()).toBe(false)
    wrapper.unmount()
  })
})
