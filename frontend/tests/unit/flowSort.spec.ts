// Server-side sorting of the Flows table (time / bytes / packets).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NDataTable } from 'naive-ui'
import { analytics, flow, resetApiMocks } from './apiMocks'
import { mountWithApp, ok } from './helpers'
import { MockEventSource } from './eventSourceMock'
import FlowsView from '@/views/FlowsView.vue'
import { DEFAULT_FLOW_SORT, flowSortFromSorter, flowSortParams, sortOrderFor } from '@/utils/flowSort'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)
vi.mock('@/api/applications', async () => (await import('./apiMocks')).applications)

describe('flowSort utils', () => {
  it('maps table sorter state to a sort, falling back to newest first', () => {
    expect(flowSortFromSorter({ columnKey: 'bytes', order: 'descend' })).toEqual({ key: 'bytes', desc: true })
    expect(flowSortFromSorter({ columnKey: 'packets', order: 'ascend' })).toEqual({ key: 'packets', desc: false })
    expect(flowSortFromSorter({ columnKey: 'bytes', order: false })).toEqual(DEFAULT_FLOW_SORT)
    expect(flowSortFromSorter({ columnKey: 'remote_ip', order: 'ascend' })).toEqual(DEFAULT_FLOW_SORT)
    expect(flowSortFromSorter(null)).toEqual(DEFAULT_FLOW_SORT)
  })

  it('sends no params for the default and sort/order otherwise', () => {
    expect(flowSortParams(DEFAULT_FLOW_SORT)).toEqual({})
    expect(flowSortParams({ key: 'bytes', desc: true })).toEqual({ sort: 'bytes', order: 'desc' })
    expect(flowSortParams({ key: 'time', desc: false })).toEqual({ sort: 'time', order: 'asc' })
  })

  it('reports the column sort order for the table', () => {
    const s = { key: 'bytes', desc: false } as const
    expect(sortOrderFor(s, 'bytes')).toBe('ascend')
    expect(sortOrderFor(s, 'ts')).toBe(false)
    expect(sortOrderFor(DEFAULT_FLOW_SORT, 'ts')).toBe('descend')
  })
})

describe('FlowsView sorting', () => {
  beforeEach(() => {
    resetApiMocks()
    vi.stubGlobal('EventSource', MockEventSource)
  })
  afterEach(() => {
    document.body.innerHTML = ''
    vi.unstubAllGlobals()
  })

  it('marks Time, Bytes and Packets as remotely sortable', async () => {
    const { wrapper } = await mountWithApp(FlowsView)
    const cols = wrapper.findComponent(NDataTable).props('columns') as { key: string; sorter?: unknown; sortOrder?: unknown }[]
    const sortable = cols.filter((c) => c.sorter === true).map((c) => c.key)
    expect(sortable).toEqual(['ts', 'bytes', 'packets'])
    expect(cols.find((c) => c.key === 'ts')?.sortOrder).toBe('descend')
    wrapper.unmount()
  })

  it('re-queries page 1 sorted by bytes and passes it to the CSV export', async () => {
    const { wrapper } = await mountWithApp(FlowsView)
    const table = wrapper.findComponent(NDataTable)
    table.vm.$emit('update:sorter', { columnKey: 'bytes', order: 'descend' })
    await flushPromises()
    expect(analytics.fetchFlows).toHaveBeenLastCalledWith(
      expect.objectContaining({ sort: 'bytes', order: 'desc', page: 1 }),
    )
    const cols = table.props('columns') as { key: string; sortOrder?: unknown }[]
    expect(cols.find((c) => c.key === 'bytes')?.sortOrder).toBe('descend')
    expect(cols.find((c) => c.key === 'ts')?.sortOrder).toBe(false)
    await wrapper.find('[data-testid="flows-export"]').trigger('click')
    expect(analytics.flowsExportUrl).toHaveBeenLastCalledWith(
      expect.objectContaining({ sort: 'bytes', order: 'desc' }),
    )
    table.vm.$emit('update:sorter', { columnKey: 'bytes', order: false })
    await flushPromises()
    const last = analytics.fetchFlows.mock.lastCall?.[0] as Record<string, unknown>
    expect(last.sort).toBeUndefined()
    wrapper.unmount()
  })
})

describe('FlowsView row identity', () => {
  beforeEach(() => {
    resetApiMocks()
    vi.stubGlobal('EventSource', MockEventSource)
  })
  afterEach(() => {
    document.body.innerHTML = ''
    vi.unstubAllGlobals()
  })

  it('replaces every row when identical-looking flows are re-sorted', async () => {
    const twin = { ...flow, bytes: 4_800_000 }
    analytics.fetchFlows.mockImplementationOnce(() =>
      Promise.resolve(ok([twin, twin, { ...flow, remote_port: 53, bytes: 78 }], { total: 3 })),
    )
    const { wrapper } = await mountWithApp(FlowsView)
    expect(wrapper.findAll('tbody tr')).toHaveLength(3)
    analytics.fetchFlows.mockImplementationOnce(() =>
      Promise.resolve(ok([{ ...flow, remote_port: 1, bytes: 9e7 }, twin], { total: 2 })),
    )
    wrapper.findComponent(NDataTable).vm.$emit('update:sorter', { columnKey: 'bytes', order: 'descend' })
    await flushPromises()
    expect(wrapper.findAll('tbody tr')).toHaveLength(2)
    wrapper.unmount()
  })
})
