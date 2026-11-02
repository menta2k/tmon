import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { categories, config, resetApiMocks, rules } from './apiMocks'
import { mountWithApp } from './helpers'
import RulesView from '@/views/RulesView.vue'
import { ApiError } from '@/api/client'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)

const $ = <T extends Element = HTMLElement>(sel: string): T | null => document.body.querySelector<T>(sel)
const $$ = <T extends Element = HTMLElement>(sel: string): T[] => [...document.body.querySelectorAll<T>(sel)]
const body = (): string => document.body.textContent ?? ''

function typeInto(input: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  input.value = value
  input.dispatchEvent(new Event('input'))
}

async function clickButton(text: string, scope = ''): Promise<void> {
  const btn = $$<HTMLButtonElement>(`${scope} button`).find((b) => b.textContent?.trim() === text)
  if (!btn) throw new Error(`button "${text}" not found`)
  btn.click()
  await flushPromises()
}

async function confirmPopover(): Promise<void> {
  const btns = $$<HTMLButtonElement>('.n-popconfirm__action button')
  btns.at(-1)!.click()
  await flushPromises()
}

async function openTab(wrapper: VueWrapper, name: string): Promise<void> {
  await wrapper
    .findAll('.n-tabs-tab')
    .find((t) => t.text() === name)!
    .trigger('click')
  await flushPromises()
}

beforeEach(() => {
  resetApiMocks()
})
afterEach(() => {
  document.body.innerHTML = ''
  vi.useRealTimers()
})

describe('RulesView', () => {
  it('lists user rules first by priority with hits and categories', async () => {
    const { wrapper } = await mountWithApp(RulesView)
    const userRows = wrapper.findAll('[data-testid="user-rule-row"]')
    expect(userRows.map((r) => r.text().includes('Steam'))).toEqual([true, false])
    expect(wrapper.text()).toContain('proto 17 · remote_port 27015-27030')
    expect(wrapper.text()).toContain('Netflix')
    expect(wrapper.text()).toContain('Gaming')
    wrapper.unmount()
  })

  it('toggles, reorders and deletes rules', async () => {
    config.updateRule.mockResolvedValue({})
    config.reorderRules.mockResolvedValue(undefined)
    config.deleteRule.mockResolvedValue(undefined)
    const { wrapper } = await mountWithApp(RulesView)

    await wrapper.findAll('[data-testid="user-rules-table"] .n-switch')[1]!.trigger('click')
    await flushPromises()
    expect(config.updateRule).toHaveBeenCalledWith(11, { enabled: true })

    await clickButton('↓', '[data-testid="user-rules-table"]')
    expect(config.reorderRules).toHaveBeenCalledWith([11, 10])

    const rows = wrapper.findAll('[data-testid="user-rule-row"]')
    await rows[1]!.trigger('dragstart', { dataTransfer: { setData: vi.fn() } })
    await rows[0]!.trigger('dragover')
    await rows[0]!.trigger('drop')
    await flushPromises()
    expect(config.reorderRules).toHaveBeenLastCalledWith([11, 10])

    await clickButton('Delete', '[data-testid="user-rules-table"]')
    await confirmPopover()
    expect(config.deleteRule).toHaveBeenCalledWith(10)
    expect(wrapper.findAll('[data-testid="user-rule-row"]')).toHaveLength(1)
    wrapper.unmount()
  })

  it('rolls back a failed toggle', async () => {
    config.updateRule.mockRejectedValue(new Error('nope'))
    const { wrapper } = await mountWithApp(RulesView)
    const sw = wrapper.findAll('[data-testid="user-rules-table"] .n-switch')[0]!
    await sw.trigger('click')
    await flushPromises()
    expect(sw.classes()).toContain('n-switch--active')
    expect(body()).toContain('nope')
    wrapper.unmount()
  })

  it('creates a rule after inline validation', async () => {
    config.createRule.mockResolvedValue({ ...rules[1], id: 99 })
    const { wrapper } = await mountWithApp(RulesView)
    await wrapper.find('[data-testid="rule-new"]').trigger('click')
    await flushPromises()
    await clickButton('Save', '.n-modal')
    expect(config.createRule).not.toHaveBeenCalled()
    expect(body()).toContain('Set at least one match criterion')
    expect(body()).toContain('Choose a category')

    typeInto($<HTMLInputElement>('[data-testid="rule-name"] input')!, 'Steam games')
    const portInput = $<HTMLInputElement>('[data-testid="rule-match-remote_port"] input')!
    portInput.focus()
    typeInto(portInput, '27030-27015')
    portInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await flushPromises()
    expect(body()).toContain('must have start ≤ end')
    wrapper.unmount()
  })

  it('edits an existing rule and saves the parsed match', async () => {
    config.updateRule.mockResolvedValue(rules[1])
    const { wrapper } = await mountWithApp(RulesView)
    await clickButton('Edit', '[data-testid="user-rules-table"]')
    expect($<HTMLInputElement>('[data-testid="rule-name"] input')!.value).toBe('Steam')
    await clickButton('Save', '.n-modal')
    expect(config.updateRule).toHaveBeenCalledWith(10, {
      name: 'Steam',
      category_id: 7,
      enabled: true,
      match: { proto: [17], remote_port: ['27015-27030'] },
    })
    wrapper.unmount()
  })

  it('shows server-side validation errors', async () => {
    config.updateRule.mockRejectedValue(
      new ApiError(400, {
        code: 'validation_failed',
        message: 'name: duplicate',
        fields: { name: 'duplicate' },
      }),
    )
    const { wrapper } = await mountWithApp(RulesView)
    await clickButton('Edit', '[data-testid="user-rules-table"]')
    await clickButton('Save', '.n-modal')
    expect(body()).toContain('name: duplicate')
    wrapper.unmount()
  })

  it('re-applies rules to history and polls the job', async () => {
    vi.useFakeTimers()
    config.fetchJob
      .mockResolvedValueOnce({ state: 'running', progress: 0.5 })
      .mockResolvedValueOnce({ state: 'done', progress: 1 })
    const { wrapper } = await mountWithApp(RulesView)
    await wrapper.find('[data-testid="rules-reapply"]').trigger('click')
    await flushPromises()
    await confirmPopover()
    expect(config.reapplyRules).toHaveBeenCalled()
    expect(wrapper.find('[data-testid="reapply-progress"]').exists()).toBe(true)
    await vi.advanceTimersByTimeAsync(1000)
    expect(config.fetchJob).toHaveBeenCalledTimes(2)
    expect(wrapper.find('[data-testid="reapply-state"]').text()).toBe('done')
    wrapper.unmount()
  })

  it('reports a failed job', async () => {
    config.fetchJob.mockResolvedValue({ state: 'failed', progress: 0.2, error: 'disk full' })
    const { wrapper } = await mountWithApp(RulesView)
    await wrapper.find('[data-testid="rules-reapply"]').trigger('click')
    await flushPromises()
    await confirmPopover()
    expect(body()).toContain('disk full')
    wrapper.unmount()
  })

  it('manages categories: create with validation, delete disabled for built-ins', async () => {
    config.createCategory.mockResolvedValue({})
    config.deleteCategory.mockResolvedValue(undefined)
    const { wrapper } = await mountWithApp(RulesView)
    await openTab(wrapper, 'Categories')
    await wrapper.find('[data-testid="category-new"]').trigger('click')
    await flushPromises()
    await clickButton('Save', '.n-modal')
    expect(config.createCategory).not.toHaveBeenCalled()
    expect(body()).toContain('Name must be 1–40 characters')
    typeInto($<HTMLInputElement>('[data-testid="category-name"] input')!, 'IoT')
    await flushPromises()
    await clickButton('Save', '.n-modal')
    expect(config.createCategory).toHaveBeenCalledWith({ name: 'IoT', color: '#8a8f98', description: '' })

    const editButtons = wrapper.findAll('button').filter((b) => b.text() === 'Edit')
    await editButtons[editButtons.length - 3]!.trigger('click') // "Other" (built-in)
    await flushPromises()
    expect($<HTMLButtonElement>('[data-testid="category-delete"]')!.disabled).toBe(true)
    await clickButton('Cancel', '.n-modal')

    await wrapper
      .findAll('button')
      .filter((b) => b.text() === 'Edit')
      .at(-1)!
      .trigger('click') // Gaming (user)
    await flushPromises()
    expect($<HTMLButtonElement>('[data-testid="category-delete"]')!.disabled).toBe(false)
    $<HTMLButtonElement>('[data-testid="category-delete"]')!.click()
    await flushPromises()
    await confirmPopover()
    expect(config.deleteCategory).toHaveBeenCalledWith(categories[2]!.id)
    wrapper.unmount()
  })

  it('dry-runs a flow in the test panel', async () => {
    const { wrapper } = await mountWithApp(RulesView)
    await openTab(wrapper, 'Test')
    const inputs = wrapper.findAll('[data-testid="rule-test-panel"] input')
    await inputs[0]!.setValue('17')
    await inputs[4]!.setValue('27020')
    await wrapper.find('[data-testid="rule-test-panel"] form').trigger('submit')
    await flushPromises()
    expect(config.testRule).toHaveBeenCalledWith({ proto: 17, remote_port: 27020 })
    expect(wrapper.find('[data-testid="rule-test-result"]').text()).toContain('Steam')
    await inputs[3]!.setValue('bad-ip')
    await wrapper.find('[data-testid="rule-test-panel"] form').trigger('submit')
    await flushPromises()
    expect(config.testRule).toHaveBeenCalledTimes(1)
    expect(body()).toContain('not a valid IP')
    wrapper.unmount()
  })

  it('shows a load error', async () => {
    config.fetchRules.mockRejectedValue(new Error('db locked'))
    const { wrapper } = await mountWithApp(RulesView)
    expect(wrapper.text()).toContain('Loading rules: db locked')
    wrapper.unmount()
  })
})
