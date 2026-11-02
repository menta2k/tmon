<script setup lang="ts">
import { computed, h, ref } from 'vue'
import { useRouter } from 'vue-router'
import { NAlert, NCard, NDataTable, NSelect, type DataTableColumns } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { DASHBOARD_REFRESH_MS, TOP_N_DEFAULT, TOP_N_OPTIONS } from '@/constants'
import { fetchTopHosts } from '@/api/analytics'
import { mergeParams } from '@/api/query'
import type { TopHost } from '@/api/types'
import { useAsyncData } from '@/composables/useAsyncData'
import { useRangeFilters } from '@/composables/useRangeFilters'
import { useTimeRangeStore } from '@/stores/timeRange'
import { formatBytes, formatNumber } from '@/utils/format'
import CategoryChip from '@/components/CategoryChip.vue'
import FilterBar from '@/components/FilterBar.vue'
import SampledBadge from '@/components/SampledBadge.vue'

const router = useRouter()
const { params } = useRangeFilters()
const { isLive } = storeToRefs(useTimeRangeStore())
const limit = ref<number>(TOP_N_DEFAULT)
const limitOptions = TOP_N_OPTIONS.map((n) => ({ label: `Top ${n}`, value: n }))

const query = computed(() => mergeParams(params.value, { limit: limit.value }))
const hosts = useAsyncData(() => fetchTopHosts(query.value), {
  watch: [query],
  refreshMs: DASHBOARD_REFRESH_MS,
  refreshEnabled: isLive,
})

const rows = computed<TopHost[]>(() => [...(hosts.data.value ?? [])])
const total = (r: TopHost): number => r.bytes_down + r.bytes_up

const columns: DataTableColumns<TopHost> = [
  {
    title: 'Name',
    key: 'name',
    ellipsis: { tooltip: true },
    sorter: (a, b) => (a.name ?? a.ip).localeCompare(b.name ?? b.ip),
    render: (r) => r.name ?? h('span', { class: 'muted' }, '—'),
  },
  { title: 'IP', key: 'ip', className: 'mono', sorter: (a, b) => a.ip.localeCompare(b.ip) },
  {
    title: 'Download',
    key: 'bytes_down',
    align: 'right',
    sorter: (a, b) => a.bytes_down - b.bytes_down,
    render: (r) => formatBytes(r.bytes_down),
  },
  {
    title: 'Upload',
    key: 'bytes_up',
    align: 'right',
    sorter: (a, b) => a.bytes_up - b.bytes_up,
    render: (r) => formatBytes(r.bytes_up),
  },
  {
    title: 'Total',
    key: 'total',
    align: 'right',
    defaultSortOrder: 'descend',
    sorter: (a, b) => total(a) - total(b),
    render: (r) => formatBytes(total(r)),
  },
  {
    title: 'Flows',
    key: 'flows',
    align: 'right',
    sorter: (a, b) => a.flows - b.flows,
    render: (r) => formatNumber(r.flows),
  },
  {
    title: 'Top category',
    key: 'top_category',
    render: (r) => (r.top_category === null ? '—' : h(CategoryChip, { id: r.top_category })),
  },
]

function rowProps(row: TopHost): Record<string, unknown> {
  return {
    class: 'clickable-row',
    'data-testid': 'top-host-row',
    onClick: () => void router.push({ name: 'host-detail', params: { ip: row.ip } }),
  }
}
</script>

<template>
  <div class="stack">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Top hosts</span>
      <SampledBadge :show="hosts.meta.value.sampled === true" />
      <NSelect v-model:value="limit" :options="limitOptions" size="small" style="width: 110px" />
    </div>
    <FilterBar />
    <NAlert v-if="hosts.error.value" type="error" :bordered="false">{{ hosts.error.value }}</NAlert>
    <NCard size="small">
      <NDataTable
        size="small"
        :columns="columns"
        :data="rows"
        :loading="hosts.loading.value"
        :row-key="(r: TopHost) => r.ip"
        :row-props="rowProps"
        :scroll-x="760"
        data-testid="top-hosts-table"
      />
    </NCard>
  </div>
</template>
