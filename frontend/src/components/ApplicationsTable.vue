<script setup lang="ts">
import { computed, h } from 'vue'
import { NDataTable, NTag, NTooltip, type DataTableColumns } from 'naive-ui'
import type { BreakdownItem } from '@/api/types'
import { APPLICATIONS_MAX_ROWS, CONFIDENCE_INFO } from '@/constants'
import { guessedShare, type ApplicationsMode } from '@/utils/confidence'
import { formatBytes, formatPercent } from '@/utils/format'

interface Props {
  items: readonly BreakdownItem[]
  loading?: boolean
  mode?: ApplicationsMode
}

const props = withDefaults(defineProps<Props>(), { loading: false, mode: 'rule' })

const rows = computed(() => props.items.slice(0, APPLICATIONS_MAX_ROWS))
const guessed = computed(() => guessedShare(props.items))

function confidenceTag(item: BreakdownItem) {
  const info = CONFIDENCE_INFO[item.confidence ?? 'none']
  return h(NTooltip, null, {
    trigger: () =>
      h(
        NTag,
        { size: 'small', type: info.type, bordered: false, 'data-testid': 'confidence-tag' },
        () => info.label,
      ),
    default: () => info.hint,
  })
}

function confidentShare(item: BreakdownItem) {
  const text = item.confident_share === undefined ? '—' : formatPercent(item.confident_share, 0)
  return h('span', { 'data-testid': 'confident-share' }, text)
}

const nameColumn = { title: 'Application', key: 'label', ellipsis: { tooltip: true } } as const
const tailColumns: DataTableColumns<BreakdownItem> = [
  { title: 'Bytes', key: 'bytes', width: 100, align: 'right', render: (r) => formatBytes(r.bytes) },
  { title: 'Share', key: 'share', width: 80, align: 'right', render: (r) => formatPercent(r.share, 0) },
]
const RULE_COLUMNS: DataTableColumns<BreakdownItem> = [
  nameColumn,
  { title: 'Confidence', key: 'confidence', width: 110, render: confidenceTag },
  ...tailColumns,
]
const APP_COLUMNS: DataTableColumns<BreakdownItem> = [
  nameColumn,
  {
    title: 'Confident',
    key: 'confident_share',
    width: 100,
    align: 'right',
    render: confidentShare,
  },
  ...tailColumns,
]

const columns = computed(() => (props.mode === 'app' ? APP_COLUMNS : RULE_COLUMNS))
</script>

<template>
  <div class="stack" data-testid="applications">
    <div v-if="mode === 'rule' && items.length > 0" class="muted" data-testid="guessed-summary">
      {{ formatPercent(guessed, 0) }} of traffic is categorized by port only or not at all.
    </div>
    <NDataTable
      size="small"
      :columns="columns"
      :data="rows"
      :loading="loading"
      :row-key="(r: BreakdownItem) => String(r.key)"
      data-testid="applications-table"
    />
  </div>
</template>
