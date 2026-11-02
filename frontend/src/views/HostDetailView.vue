<script setup lang="ts">
import { computed, h } from 'vue'
import { NAlert, NCard, NDataTable, NDescriptions, NDescriptionsItem, type DataTableColumns } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { DASHBOARD_REFRESH_MS } from '@/constants'
import { fetchHostDetail, fetchTimeseries } from '@/api/analytics'
import { mergeParams } from '@/api/query'
import type { HostTopApp, TopRemote } from '@/api/types'
import { useAsyncData } from '@/composables/useAsyncData'
import { useRangeFilters } from '@/composables/useRangeFilters'
import { useTimeRangeStore } from '@/stores/timeRange'
import { useCategoriesStore } from '@/stores/categories'
import {
  formatBytes,
  formatNumber,
  hostDisplayName,
  remoteAsnLabel,
  remoteCountryLabel,
} from '@/utils/format'
import { formatDateTime } from '@/utils/time'
import SampledBadge from '@/components/SampledBadge.vue'
import HostSourceTag from '@/components/HostSourceTag.vue'
import TimeSeriesChart from '@/components/charts/TimeSeriesChart.vue'
import BreakdownChart from '@/components/charts/BreakdownChart.vue'

const props = defineProps<{ ip: string }>()

const { params, rangeOnly } = useRangeFilters()
const { isLive } = storeToRefs(useTimeRangeStore())
const categories = useCategoriesStore()
const opts = { refreshMs: DASHBOARD_REFRESH_MS, refreshEnabled: isLive }

const tsQuery = computed(() => mergeParams(params.value, { host: [props.ip] }))
const detail = useAsyncData(() => fetchHostDetail(props.ip, rangeOnly.value), {
  ...opts,
  watch: [rangeOnly, () => props.ip],
})
const timeseries = useAsyncData(() => fetchTimeseries('category', tsQuery.value), {
  ...opts,
  watch: [tsQuery],
})

const categoryColors = computed<Record<string, string>>(() =>
  Object.fromEntries(categories.items.map((c) => [String(c.id), c.color])),
)
const host = computed(() => detail.data.value?.host ?? null)
const title = computed(() => (host.value ? hostDisplayName(host.value) : props.ip))
const sampled = computed(() => detail.meta.value.sampled === true || timeseries.meta.value.sampled === true)

const topApps = computed<readonly HostTopApp[]>(() => detail.data.value?.top_apps ?? [])
const appColumns: DataTableColumns<HostTopApp> = [
  { title: 'Application', key: 'app', ellipsis: { tooltip: true } },
  { title: 'Bytes', key: 'bytes', align: 'right', render: (r) => formatBytes(r.bytes) },
  { title: 'Flows', key: 'flows', align: 'right', render: (r) => formatNumber(r.flows) },
]

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
  { title: 'Bytes', key: 'bytes', align: 'right', render: (r) => formatBytes(r.bytes) },
  { title: 'Flows', key: 'flows', align: 'right', render: (r) => formatNumber(r.flows) },
]
</script>

<template>
  <div class="stack" data-testid="host-detail">
    <div class="page-toolbar">
      <span class="page-toolbar__title">{{ title }}</span>
      <span class="mono muted">{{ ip }}</span>
      <SampledBadge :show="sampled" />
    </div>
    <NAlert v-if="detail.error.value" type="error" :bordered="false">{{ detail.error.value }}</NAlert>

    <NCard v-if="detail.data.value" size="small">
      <NDescriptions :column="4" size="small" label-placement="top">
        <NDescriptionsItem label="Download">{{
          formatBytes(detail.data.value.totals.bytes_down)
        }}</NDescriptionsItem>
        <NDescriptionsItem label="Upload">{{
          formatBytes(detail.data.value.totals.bytes_up)
        }}</NDescriptionsItem>
        <NDescriptionsItem label="Flows">{{
          formatNumber(detail.data.value.totals.flows)
        }}</NDescriptionsItem>
        <NDescriptionsItem label="MAC">{{ host?.mac ?? '—' }}</NDescriptionsItem>
        <NDescriptionsItem label="Friendly name">{{ host?.friendly_name ?? '—' }}</NDescriptionsItem>
        <NDescriptionsItem label="DHCP hostname">{{ host?.dhcp_hostname ?? '—' }}</NDescriptionsItem>
        <NDescriptionsItem label="First seen">{{ formatDateTime(host?.first_seen) }}</NDescriptionsItem>
        <NDescriptionsItem label="Last seen">{{ formatDateTime(host?.last_seen) }}</NDescriptionsItem>
      </NDescriptions>
    </NCard>

    <NCard size="small" title="Traffic by category">
      <TimeSeriesChart
        :series="timeseries.data.value?.series ?? []"
        :step="timeseries.data.value?.step ?? 1"
        :colors="categoryColors"
        :loading="timeseries.loading.value"
      />
    </NCard>

    <div class="grid-2">
      <NCard size="small">
        <BreakdownChart
          title="Categories"
          :items="detail.data.value?.categories ?? []"
          :colors="categoryColors"
          :loading="detail.loading.value"
        />
      </NCard>
      <NCard size="small">
        <BreakdownChart
          title="Protocols"
          :items="detail.data.value?.protocols ?? []"
          :loading="detail.loading.value"
        />
      </NCard>
    </div>

    <NCard v-if="topApps.length > 0" size="small" title="Top applications" data-testid="host-top-apps">
      <NDataTable
        size="small"
        :columns="appColumns"
        :data="[...topApps]"
        :row-key="(r: HostTopApp) => r.app"
      />
    </NCard>

    <NCard size="small" title="Top remote endpoints">
      <NDataTable
        size="small"
        :columns="remoteColumns"
        :data="detail.data.value?.top_remotes ?? []"
        :loading="detail.loading.value"
        :row-key="(r: TopRemote) => r.ip"
        :scroll-x="760"
        data-testid="host-remotes-table"
      />
    </NCard>
  </div>
</template>
