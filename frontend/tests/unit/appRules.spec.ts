// T049 — rule editor app / app_category fields and rule-test confidence (feature 003).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import { catalog, config, resetApiMocks, rules } from './apiMocks'
import { mountWithApp } from './helpers'
import RulesView from '@/views/RulesView.vue'
import { CONFIDENCE_INFO } from '@/constants'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)
vi.mock('@/api/applications', async () => (await import('./apiMocks')).applications)

const $ = <T extends Element = HTMLElement>(sel: string): T | null => document.body.querySelector<T>(sel)
const body = (): string => document.body.textContent ?? ''

function typeInto(input: HTMLInputElement, value: string): void {
  input.value = value
  input.dispatchEvent(new Event('input'))
}

function matchSelect(wrapper: VueWrapper, field: string) {
  const select = wrapper
    .findAllComponents(NSelect)
    .find((c) => c.attributes('data-testid') === `rule-match-${field}`)
  if (!select) throw new Error(`select ${field} not found`)
  return select
}

async function openEditor(wrapper: VueWrapper): Promise<void> {
  await wrapper.find('[data-testid="rule-new"]').trigger('click')
  await flushPromises()
}

beforeEach(() => resetApiMocks())
afterEach(() => {
  document.body.innerHTML = ''
})

describe('RuleEditor application fields', () => {
  it('offers the catalog with search and free text', async () => {
    const { wrapper } = await mountWithApp(RulesView)
    await openEditor(wrapper)
    const app = matchSelect(wrapper, 'app')
    expect((app.props('options') as { value: string }[]).map((o) => o.value)).toEqual(
      catalog.map((c) => c.app),
    )
    expect(app.props('filterable')).toBe(true)
    expect(app.props('tag')).toBe(true)
    const cat = matchSelect(wrapper, 'app_category')
    expect((cat.props('options') as { value: string }[]).map((o) => o.value)).toEqual([
      'Download',
      'Game',
      'Media',
    ])
    wrapper.unmount()
  })

  it('validates 1–48 characters and saves the app match', async () => {
    config.createRule.mockResolvedValue({ ...rules[1], id: 99 })
    const { wrapper } = await mountWithApp(RulesView)
    await openEditor(wrapper)
    typeInto($<HTMLInputElement>('[data-testid="rule-name"] input')!, 'Torrents')
    const category = wrapper
      .findAllComponents(NSelect)
      .find((c) => c.attributes('data-testid') === 'rule-category')!
    category.vm.$emit('update:value', 7)

    matchSelect(wrapper, 'app').vm.$emit('update:value', ['x'.repeat(49)])
    await flushPromises()
    expect(body()).toContain('1–48')

    matchSelect(wrapper, 'app').vm.$emit('update:value', ['BitTorrent', 'My App'])
    await flushPromises()
    $<HTMLButtonElement>('[data-testid="rule-save"]')!.click()
    await flushPromises()
    expect(config.createRule).toHaveBeenCalledWith({
      name: 'Torrents',
      category_id: 7,
      enabled: true,
      match: { app: ['BitTorrent', 'My App'] },
    })
    wrapper.unmount()
  })
})

describe('RuleTestPanel application fields', () => {
  async function openTestTab(wrapper: VueWrapper): Promise<void> {
    await wrapper
      .findAll('.n-tabs-tab')
      .find((t) => t.text() === 'Test')!
      .trigger('click')
    await flushPromises()
  }

  it('sends app fields and shows the match confidence', async () => {
    const { wrapper } = await mountWithApp(RulesView)
    await openTestTab(wrapper)
    await wrapper.find('[data-testid="rule-test-app"] input').setValue('BitTorrent')
    await wrapper.find('[data-testid="rule-test-app-category"] input').setValue('Download')
    await wrapper.find('[data-testid="rule-test-panel"] form').trigger('submit')
    await flushPromises()
    expect(config.testRule).toHaveBeenCalledWith({
      app: 'BitTorrent',
      app_category: 'Download',
      app_confident: true,
    })
    expect(wrapper.find('[data-testid="rule-test-confidence"]').text()).toBe(CONFIDENCE_INFO.inferred.label)
    wrapper.unmount()
  })

  it('sends app_confident=false when the guessed box is ticked', async () => {
    const { wrapper } = await mountWithApp(RulesView)
    await openTestTab(wrapper)
    await wrapper.find('[data-testid="rule-test-app"] input').setValue('BitTorrent')
    await wrapper.find('[data-testid="rule-test-app-guessed"]').trigger('click')
    await wrapper.find('[data-testid="rule-test-panel"] form').trigger('submit')
    await flushPromises()
    expect(config.testRule).toHaveBeenCalledWith({ app: 'BitTorrent', app_confident: false })
    wrapper.unmount()
  })

  it('rejects an invalid app name', async () => {
    const { wrapper } = await mountWithApp(RulesView)
    await openTestTab(wrapper)
    await wrapper.find('[data-testid="rule-test-app"] input').setValue('x'.repeat(49))
    await wrapper.find('[data-testid="rule-test-panel"] form').trigger('submit')
    await flushPromises()
    expect(config.testRule).not.toHaveBeenCalled()
    expect(body()).toContain('1–48')
    wrapper.unmount()
  })
})
