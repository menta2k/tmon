<script setup lang="ts">
import { computed, h } from 'vue'
import { NDataTable, NEmpty, NSpin, type DataTableColumns } from 'naive-ui'
import type { BreakdownItem } from '@/api/types'
import { useThemeStore } from '@/stores/theme'
import { formatBytes, formatNumber, formatPercent } from '@/utils/format'
import { VChart } from './echarts'
import { buildDonutOption, colorForKey, type ColorMap } from './options'

interface Props {
  title: string
  items: readonly BreakdownItem[]
  colors?: ColorMap
  loading?: boolean
  /** Max rows shown in the table. */
  maxRows?: number
}

const props = withDefaults(defineProps<Props>(), { colors: () => ({}), loading: false, maxRows: 8 })
const theme = useThemeStore()

const option = computed(() =>
  buildDonutOption(props.items, props.colors, theme.mode === 'dark' ? '#cbd5e1' : undefined),
)

interface Row {
  key: string
  label: string
  color: string
  bytes: number
  flows: number
  share: number
}

const rows = computed<Row[]>(() =>
  props.items.slice(0, props.maxRows).map((it, i) => ({
    key: String(it.key),
    label: it.label,
    color: colorForKey(it.key, i, props.colors),
    bytes: it.bytes,
    flows: it.flows,
    share: it.share,
  })),
)

const columns: DataTableColumns<Row> = [
  {
    title: 'Name',
    key: 'label',
    ellipsis: { tooltip: true },
    render: (r) => [h('span', { class: 'swatch', style: { background: r.color } }), r.label],
  },
  { title: 'Bytes', key: 'bytes', align: 'right', render: (r) => formatBytes(r.bytes) },
  { title: 'Flows', key: 'flows', align: 'right', render: (r) => formatNumber(r.flows) },
  { title: 'Share', key: 'share', align: 'right', render: (r) => formatPercent(r.share) },
]
</script>

<template>
  <div class="breakdown" :data-testid="`breakdown-${title.toLowerCase()}`">
    <div class="breakdown__title">{{ title }}</div>
    <NSpin :show="loading">
      <template v-if="items.length > 0">
        <div class="breakdown__donut">
          <VChart :option="option" :theme="theme.mode === 'dark' ? 'dark' : ''" autoresize />
        </div>
        <NDataTable
          size="small"
          :columns="columns"
          :data="rows"
          :row-key="(r: Row) => r.key"
          :bordered="false"
        />
      </template>
      <NEmpty v-else description="No data" />
    </NSpin>
  </div>
</template>

<style scoped>
.breakdown__title {
  font-weight: 600;
  margin-bottom: 4px;
}
.breakdown__donut {
  height: 180px;
}
.breakdown :deep(.swatch) {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 2px;
  margin-right: 6px;
  vertical-align: middle;
}
</style>
