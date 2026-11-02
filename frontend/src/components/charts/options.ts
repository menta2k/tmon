/** Pure ECharts option builders (unit-tested; no DOM access). */
import type { EChartsOption } from 'echarts'
import type { BreakdownItem, TimeseriesSeries, TopKeyedItem } from '@/api/types'
import { FALLBACK_COLOR, SERIES_PALETTE } from '@/constants'
import { formatBps, formatBytes, formatPercent } from '@/utils/format'
import { toMillis } from '@/utils/time'

export type ColorMap = Readonly<Record<string, string>>

export function paletteColor(index: number): string {
  return SERIES_PALETTE[index % SERIES_PALETTE.length] ?? FALLBACK_COLOR
}

export function colorForKey(key: string | number, index: number, colors?: ColorMap): string {
  return colors?.[String(key)] ?? paletteColor(index)
}

export interface TimeseriesOptionInput {
  series: readonly TimeseriesSeries[]
  stepSeconds: number
  colors?: ColorMap
  textColor?: string
}

/** Converts bytes-per-bucket points into [ms, bytes/sec] pairs. */
export function toRatePoints(points: TimeseriesSeries['points'], stepSeconds: number): [number, number][] {
  const step = stepSeconds > 0 ? stepSeconds : 1
  return points.map(([ts, bytes]) => [toMillis(ts), bytes / step])
}

interface AxisTooltipParam {
  marker?: string
  seriesName?: string
  value?: [number, number]
  axisValueLabel?: string
}

function timeseriesTooltip(params: unknown): string {
  const list = (Array.isArray(params) ? params : [params]) as AxisTooltipParam[]
  const header = list[0]?.axisValueLabel ?? ''
  const rows = list
    .filter((p) => (p.value?.[1] ?? 0) > 0)
    .map((p) => `${p.marker ?? ''}${p.seriesName ?? ''}: <b>${formatBps(p.value?.[1] ?? 0)}</b>`)
  return [header, ...rows].join('<br/>')
}

export function buildTimeseriesOption(input: TimeseriesOptionInput): EChartsOption {
  const { series, stepSeconds, colors, textColor } = input
  return {
    backgroundColor: 'transparent',
    color: series.map((s, i) => colorForKey(s.key, i, colors)),
    tooltip: { trigger: 'axis', formatter: timeseriesTooltip },
    legend: { type: 'scroll', top: 0, textStyle: { color: textColor } },
    grid: { left: 8, right: 16, top: 36, bottom: 8, containLabel: true },
    xAxis: { type: 'time', boundaryGap: [0, 0] },
    yAxis: { type: 'value', axisLabel: { formatter: (v: number) => formatBps(v, 0) } },
    series: series.map((s, i) => ({
      id: String(s.key),
      name: s.label,
      type: 'line',
      stack: 'total',
      showSymbol: false,
      smooth: false,
      lineStyle: { width: 1 },
      areaStyle: { opacity: 0.55 },
      itemStyle: { color: colorForKey(s.key, i, colors) },
      emphasis: { focus: 'series' },
      data: toRatePoints(s.points, stepSeconds),
    })),
  }
}

export function buildDonutOption(
  items: readonly BreakdownItem[],
  colors?: ColorMap,
  textColor?: string,
): EChartsOption {
  return {
    backgroundColor: 'transparent',
    tooltip: {
      trigger: 'item',
      formatter: (p: unknown) => {
        const { name, value, percent } = p as { name: string; value: number; percent: number }
        return `${name}<br/><b>${formatBytes(value)}</b> (${percent.toFixed(1)}%)`
      },
    },
    series: [
      {
        type: 'pie',
        radius: ['52%', '80%'],
        avoidLabelOverlap: true,
        label: { show: false },
        labelLine: { show: false },
        itemStyle: { borderWidth: 1, borderColor: textColor ? 'rgba(0,0,0,0.25)' : '#fff' },
        data: items.map((it, i) => ({
          name: it.label,
          value: it.bytes,
          itemStyle: { color: colorForKey(it.key, i, colors) },
        })),
      },
    ],
  }
}

export function shareLabel(item: BreakdownItem): string {
  return formatPercent(item.share)
}

export const WORLD_MAP_NAME = 'tmon-world'

export function buildWorldMapOption(items: readonly TopKeyedItem[], textColor?: string): EChartsOption {
  const labels = new Map(items.map((it) => [String(it.key).toUpperCase(), it.label]))
  const data = items.map((it) => ({ name: String(it.key).toUpperCase(), value: it.bytes }))
  const max = data.reduce((m, d) => Math.max(m, d.value), 0)
  return {
    backgroundColor: 'transparent',
    tooltip: {
      trigger: 'item',
      formatter: (p: unknown) => {
        const { name, value } = p as { name: string; value: number }
        return Number.isFinite(value) ? `${labels.get(name) ?? name}<br/><b>${formatBytes(value)}</b>` : name
      },
    },
    visualMap: {
      min: 0,
      max: max || 1,
      left: 8,
      bottom: 8,
      calculable: false,
      text: ['more', 'less'],
      textStyle: { color: textColor },
      inRange: { color: ['#cfe8ff', '#2563eb', '#1e3a8a'] },
      formatter: (v: unknown) => formatBytes(Number(v), 0),
    },
    series: [
      {
        type: 'map',
        map: WORLD_MAP_NAME,
        roam: true,
        emphasis: { label: { show: false } },
        itemStyle: { areaColor: 'rgba(148,163,184,0.25)', borderColor: 'rgba(100,116,139,0.6)' },
        data,
      },
    ],
  }
}
