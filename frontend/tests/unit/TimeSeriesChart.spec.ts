import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import TimeSeriesChart from '@/components/charts/TimeSeriesChart.vue'
import {
  buildDonutOption,
  buildTimeseriesOption,
  buildWorldMapOption,
  colorForKey,
  toRatePoints,
} from '@/components/charts/options'
import type { TimeseriesSeries } from '@/api/types'

interface SeriesOpt {
  id: string
  name: string
  stack: string
  itemStyle: { color: string }
  data: [number, number][]
}

const series: TimeseriesSeries[] = [
  {
    key: 1,
    label: 'Streaming',
    points: [
      ['2024-01-01T00:00:00Z', 6000],
      ['2024-01-01T00:01:00Z', 12000],
    ],
  },
  {
    key: 2,
    label: 'Gaming',
    points: [
      ['2024-01-01T00:00:00Z', 600],
      ['2024-01-01T00:01:00Z', 0],
    ],
  },
]
const colors = { '1': '#ff0000', '2': '#00ff00' }

type ChartProps = InstanceType<typeof TimeSeriesChart>['$props']

function mountChart(props: ChartProps) {
  const pinia = createPinia()
  setActivePinia(pinia)
  return mount(TimeSeriesChart, { props, global: { plugins: [pinia] } })
}

function chartOption(wrapper: ReturnType<typeof mountChart>): { series: SeriesOpt[]; color: string[] } {
  return wrapper.findComponent({ name: 'VChartStub' }).props('option') as {
    series: SeriesOpt[]
    color: string[]
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('TimeSeriesChart', () => {
  it('renders one stacked series per API series with category colours', () => {
    const wrapper = mountChart({ series, step: 60, colors })
    const opt = chartOption(wrapper)
    expect(opt.series.map((s) => s.name)).toEqual(['Streaming', 'Gaming'])
    expect(opt.series.every((s) => s.stack === 'total')).toBe(true)
    expect(opt.series.map((s) => s.itemStyle.color)).toEqual(['#ff0000', '#00ff00'])
    expect(opt.color).toEqual(['#ff0000', '#00ff00'])
  })

  it('converts bytes per bucket into bytes per second using the step', () => {
    const wrapper = mountChart({ series, step: '1m', colors })
    const first = chartOption(wrapper).series[0]!
    expect(first.data).toEqual([
      [Date.UTC(2024, 0, 1, 0, 0), 100],
      [Date.UTC(2024, 0, 1, 0, 1), 200],
    ])
  })

  it('shows an empty state without points', () => {
    const wrapper = mountChart({ series: [{ key: 1, label: 'x', points: [] }], step: 60 })
    expect(wrapper.findComponent({ name: 'VChartStub' }).exists()).toBe(false)
    expect(wrapper.text()).toContain('No traffic in this range')
  })

  it('updates when the series prop changes', async () => {
    const wrapper = mountChart({ series, step: 60, colors })
    await wrapper.setProps({ series: [series[0]!] })
    expect(chartOption(wrapper).series).toHaveLength(1)
  })

  it('falls back to a 1 s step for invalid steps', () => {
    const wrapper = mountChart({ series, step: 'bogus' })
    expect(chartOption(wrapper).series[0]!.data[0]![1]).toBe(6000)
  })
})

describe('option builders', () => {
  it('uses the palette when no colour is mapped', () => {
    expect(colorForKey('x', 0)).toBe('#3b82f6')
    expect(colorForKey('x', 11)).toBe('#10b981')
    expect(toRatePoints([[1_700_000_000, 10]], 0)).toEqual([[1_700_000_000_000, 10]])
  })
  it('formats the axis tooltip', () => {
    const opt = buildTimeseriesOption({ series, stepSeconds: 60, colors }) as {
      tooltip: { formatter: (p: unknown) => string }
    }
    const html = opt.tooltip.formatter([
      { axisValueLabel: '00:00', marker: '', seriesName: 'Streaming', value: [0, 125] },
      { axisValueLabel: '00:00', marker: '', seriesName: 'Gaming', value: [0, 0] },
    ])
    expect(html).toBe('00:00<br/>Streaming: <b>1.0 Kbps</b>')
  })
  it('builds donut and map options', () => {
    const items = [{ key: 1, label: 'Streaming', bytes: 1000, packets: 1, flows: 1, share: 0.5 }]
    const donut = buildDonutOption(items, colors) as {
      series: { data: { itemStyle: { color: string } }[] }[]
      tooltip: { formatter: (p: unknown) => string }
    }
    expect(donut.series[0]!.data[0]!.itemStyle.color).toBe('#ff0000')
    expect(donut.tooltip.formatter({ name: 'Streaming', value: 1000, percent: 50 })).toContain('50.0%')
    const map = buildWorldMapOption([{ key: 'us', label: 'United States', bytes: 5000, flows: 3 }]) as {
      series: { data: { name: string }[] }[]
      tooltip: { formatter: (p: unknown) => string }
      visualMap: { max: number }
    }
    expect(map.series[0]!.data[0]!.name).toBe('US')
    expect(map.visualMap.max).toBe(5000)
    expect(map.tooltip.formatter({ name: 'US', value: 5000 })).toContain('United States')
    expect(map.tooltip.formatter({ name: 'FR', value: Number.NaN })).toBe('FR')
  })
})
