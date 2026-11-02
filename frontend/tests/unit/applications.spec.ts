import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { NConfigProvider } from 'naive-ui'
import { h } from 'vue'
import ApplicationsTable from '@/components/ApplicationsTable.vue'
import { guessedShare } from '@/utils/confidence'
import type { BreakdownItem } from '@/api/types'

const rows: BreakdownItem[] = [
  {
    key: '900',
    label: 'HTTPS / HTTP / QUIC',
    bytes: 9000,
    packets: 9,
    flows: 3,
    share: 0.75,
    confidence: 'guessed',
  },
  {
    key: '1',
    label: 'YouTube video',
    bytes: 2500,
    packets: 3,
    flows: 1,
    share: 0.2083,
    confidence: 'detected',
  },
  { key: '0', label: 'Unmatched', bytes: 500, packets: 1, flows: 1, share: 0.0417, confidence: 'none' },
]

function render(items: BreakdownItem[]) {
  return mount({
    render: () => h(NConfigProvider, null, () => h(ApplicationsTable, { items, loading: false })),
  })
}

describe('guessedShare', () => {
  it('sums the share of port-only and unmatched traffic', () => {
    expect(guessedShare(rows)).toBeCloseTo(0.7917, 3)
  })
  it('is 0 for no data', () => {
    expect(guessedShare([])).toBe(0)
  })
})

describe('ApplicationsTable', () => {
  it('lists applications with confidence tags', () => {
    const w = render(rows)
    const text = w.find('[data-testid="applications-table"]').text()
    expect(text).toContain('HTTPS / HTTP / QUIC')
    expect(text).toContain('YouTube video')
    const tags = w.findAll('[data-testid="confidence-tag"]').map((t) => t.text())
    expect(tags).toEqual(['guessed', 'detected', 'unmatched'])
  })

  it('summarizes how much traffic is only guessed', () => {
    const w = render(rows)
    expect(w.find('[data-testid="guessed-summary"]').text()).toContain('79%')
  })

  it('hides the summary without data', () => {
    expect(render([]).find('[data-testid="guessed-summary"]').exists()).toBe(false)
  })
})
