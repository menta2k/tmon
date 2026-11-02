<script setup lang="ts">
import { computed, h, ref } from 'vue'
import { NAlert, NCard, NDataTable, NSelect, NTabPane, NTabs, type DataTableColumns } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { DASHBOARD_REFRESH_MS, TOP_N_DEFAULT, TOP_N_OPTIONS } from '@/constants'
import { fetchTopKeyed, fetchTopRemotes } from '@/api/analytics'
import { mergeParams } from '@/api/query'
import type { ApiResult, TopKeyedItem, TopKeyedKind, TopRemote } from '@/api/types'
import { useAsyncData } from '@/composables/useAsyncData'
import { useRangeFilters } from '@/composables/useRangeFilters'
import { useTimeRangeStore } from '@/stores/timeRange'
import { countryFlag, formatBytes, formatNumber, remoteAsnLabel, remoteCountryLabel } from '@/utils/format'
import FilterBar from '@/components/FilterBar.vue'
import SampledBadge from '@/components/SampledBadge.vue'
import HostSourceTag from '@/components/HostSourceTag.vue'
import WorldMapChart from '@/components/charts/WorldMapChart.vue'

type Tab = 'remotes' | TopKeyedKind
type TabRows = { tab: 'remotes'; rows: TopRemote[] } | { tab: TopKeyedKind; rows: TopKeyedItem[] }

const { params } = useRangeFilters()
const { isLive } = storeToRefs(useTimeRangeStore())
const tab = ref<Tab>('remotes')
const limit = ref<number>(TOP_N_DEFAULT)
const limitOptions = TOP_N_OPTIONS.map((n) => ({ label: `Top ${n}`, value: n }))
const query = computed(() => mergeParams(params.value, { limit: limit.value }))

async function load(): Promise<ApiResult<TabRows>> {
  const current = tab.value
  if (current === 'remotes') {
    const r = await fetchTopRemotes(query.value)
    return { data: { tab: current, rows: r.data }, meta: r.meta }
  }
  const r = await fetchTopKeyed(current, query.value)
  return { data: { tab: current, rows: r.data }, meta: r.meta }
}

const result = useAsyncData(load, {
  watch: [query, tab],
  refreshMs: DASHBOARD_REFRESH_MS,
  refreshEnabled: isLive,
})

const remotes = computed<TopRemote[]>(() =>
  result.data.value?.tab === 'remotes' ? result.data.value.rows : [],
)
const keyed = computed<TopKeyedItem[]>(() =>
  result.data.value && result.data.value.tab !== 'remotes' ? result.data.value.rows : [],
)

const remoteColumns: DataTableColumns<TopRemote> = [
  { title: 'Remote IP', key: 'ip', className: 'mono' },
  {
    title: 'Hostname',
    key: 'hostname',
    ellipsis: { tooltip: true },
    render: (r) => (r.hostname ? [r.hostname, h(HostSourceTag, { source: r.hostname_source })] : '—'),
  },
  { title: 'Country', key: 'country', render: (r) => remoteCountryLabel(r.ip, r.country) },
  {
    title: 'ASN / owner',
    key: 'asn',
    ellipsis: { tooltip: true },
    render: (r) => remoteAsnLabel(r.ip, r.asn, r.as_name),
  },
  {
    title: 'Bytes',
    key: 'bytes',
    align: 'right',
    sorter: (a, b) => a.bytes - b.bytes,
    render: (r) => formatBytes(r.bytes),
  },
  {
    title: 'Flows',
    key: 'flows',
    align: 'right',
    sorter: (a, b) => a.flows - b.flows,
    render: (r) => formatNumber(r.flows),
  },
]

function keyedColumns(kind: TopKeyedKind): DataTableColumns<TopKeyedItem> {
  const keyTitle = { asns: 'ASN', countries: 'Country', ports: 'Port' }[kind]
  const keyRender = (r: TopKeyedItem): string =>
    kind === 'countries'
      ? `${countryFlag(String(r.key))} ${r.key}`.trim()
      : kind === 'asns'
        ? `AS${r.key}`
        : String(r.key)
  return [
    { title: keyTitle, key: 'key', className: 'mono', render: keyRender },
    { title: 'Name', key: 'label', ellipsis: { tooltip: true } },
    {
      title: 'Bytes',
      key: 'bytes',
      align: 'right',
      sorter: (a, b) => a.bytes - b.bytes,
      render: (r) => formatBytes(r.bytes),
    },
    {
      title: 'Flows',
      key: 'flows',
      align: 'right',
      sorter: (a, b) => a.flows - b.flows,
      render: (r) => formatNumber(r.flows),
    },
  ]
}

const columnsByKind = {
  asns: keyedColumns('asns'),
  countries: keyedColumns('countries'),
  ports: keyedColumns('ports'),
} as const
</script>

<template>
  <div class="stack">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Remote endpoints</span>
      <SampledBadge :show="result.meta.value.sampled === true" />
      <NSelect v-model:value="limit" :options="limitOptions" size="small" style="width: 110px" />
    </div>
    <FilterBar />
    <NAlert v-if="result.error.value" type="error" :bordered="false">{{ result.error.value }}</NAlert>
    <NCard size="small">
      <NTabs v-model:value="tab" type="line" animated>
        <NTabPane name="remotes" tab="Remotes">
          <NDataTable
            size="small"
            :columns="remoteColumns"
            :data="remotes"
            :loading="result.loading.value"
            :row-key="(r: TopRemote) => r.ip"
            :scroll-x="760"
          />
        </NTabPane>
        <NTabPane name="asns" tab="ASNs">
          <NDataTable
            size="small"
            :columns="columnsByKind.asns"
            :data="keyed"
            :loading="result.loading.value"
          />
        </NTabPane>
        <NTabPane name="countries" tab="Countries">
          <div class="stack">
            <WorldMapChart :items="keyed" />
            <NDataTable
              size="small"
              :columns="columnsByKind.countries"
              :data="keyed"
              :loading="result.loading.value"
            />
          </div>
        </NTabPane>
        <NTabPane name="ports" tab="Ports">
          <NDataTable
            size="small"
            :columns="columnsByKind.ports"
            :data="keyed"
            :loading="result.loading.value"
          />
        </NTabPane>
      </NTabs>
    </NCard>
  </div>
</template>
