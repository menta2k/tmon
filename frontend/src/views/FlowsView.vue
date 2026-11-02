<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NInput,
  type DataTableColumns,
  type PaginationProps,
} from 'naive-ui'
import { FLOWS_EXPORT_MAX_ROWS, FLOWS_PAGE_SIZE_DEFAULT, FLOWS_PAGE_SIZES } from '@/constants'
import { fetchFlows, flowsExportUrl } from '@/api/analytics'
import { mergeParams, type QueryParams } from '@/api/query'
import type { FlowRecord } from '@/api/types'
import { useAsyncData } from '@/composables/useAsyncData'
import { useRangeFilters } from '@/composables/useRangeFilters'
import { useNotify } from '@/composables/useNotify'
import { triggerDownload } from '@/utils/download'
import {
  directionLabel,
  formatBytes,
  formatEndpoint,
  formatNumber,
  protocolName,
  remoteCountryLabel,
} from '@/utils/format'
import { formatDateTime } from '@/utils/time'
import { validateIp, validatePort } from '@/utils/validators'
import {
  DEFAULT_FLOW_SORT,
  flowSortFromSorter,
  flowSortParams,
  sortOrderFor,
  type ColumnSortOrder,
  type FlowSort,
  type SorterState,
} from '@/utils/flowSort'
import CategoryChip from '@/components/CategoryChip.vue'
import HostSourceTag from '@/components/HostSourceTag.vue'
import AppTag from '@/components/AppTag.vue'
import AppSelect from '@/components/AppSelect.vue'
import FilterBar from '@/components/FilterBar.vue'
import SampledBadge from '@/components/SampledBadge.vue'

const { params } = useRangeFilters()
const notify = useNotify()
const page = ref(1)
const pageSize = ref<number>(FLOWS_PAGE_SIZE_DEFAULT)
const remoteIpInput = ref('')
const portInput = ref('')
/** Applied (validated) extra filters; replaced on Apply. */
const extra = ref<Readonly<{ remote_ip?: string; port?: number }>>({})
/** Application filter (feature 003); applied immediately, raw ranges only. */
const apps = ref<readonly string[]>([])
/** Server-side sort (Time / Bytes / Packets columns). */
const sort = ref<FlowSort>(DEFAULT_FLOW_SORT)

const remoteIpError = computed(() => (remoteIpInput.value.trim() ? validateIp(remoteIpInput.value) : null))
const portError = computed(() => (portInput.value.trim() ? validatePort(portInput.value) : null))

const filterQuery = computed<QueryParams>(() =>
  mergeParams(
    params.value,
    extra.value,
    apps.value.length > 0 ? { app: apps.value } : {},
    flowSortParams(sort.value),
  ),
)
const pageQuery = computed<QueryParams>(() =>
  mergeParams(filterQuery.value, { page: page.value, limit: pageSize.value }),
)

watch(params, () => {
  page.value = 1
})

const flows = useAsyncData(() => fetchFlows(pageQuery.value), { watch: [pageQuery] })
const total = computed(() => flows.meta.value.total ?? flows.data.value?.length ?? 0)

function apply(): void {
  if (remoteIpError.value || portError.value) {
    notify.error(new Error(remoteIpError.value ?? portError.value ?? 'Invalid filter'))
    return
  }
  const ip = remoteIpInput.value.trim()
  const port = portInput.value.trim()
  extra.value = { ...(ip ? { remote_ip: ip } : {}), ...(port ? { port: Number(port) } : {}) }
  page.value = 1
}

function setApps(next: readonly string[]): void {
  const same = next.length === apps.value.length && next.every((v, i) => v === apps.value[i])
  if (same) return
  apps.value = [...next]
  page.value = 1
}

function setSort(state: SorterState | null): void {
  sort.value = flowSortFromSorter(state)
  page.value = 1
}

function exportCsv(): void {
  triggerDownload(flowsExportUrl(filterQuery.value), 'flows.csv')
}

const pagination = computed<PaginationProps>(() => ({
  page: page.value,
  pageSize: pageSize.value,
  itemCount: total.value,
  pageSizes: [...FLOWS_PAGE_SIZES],
  showSizePicker: true,
  onUpdatePage: (p: number) => {
    page.value = p
  },
  onUpdatePageSize: (s: number) => {
    pageSize.value = s
    page.value = 1
  },
}))

/** Remote sorter props for a sortable column. */
const sortable = (key: string): { sorter: true; sortOrder: ColumnSortOrder } => ({
  sorter: true,
  sortOrder: sortOrderFor(sort.value, key),
})

const columns = computed<DataTableColumns<PagedFlow>>(() => [
  { title: 'Time', key: 'ts', width: 160, ...sortable('ts'), render: (r) => formatDateTime(r.ts) },
  { title: 'Dir', key: 'direction', width: 80, render: (r) => directionLabel(r.direction) },
  {
    title: 'Local',
    key: 'local_ip',
    className: 'mono',
    render: (r) => formatEndpoint(r.local_ip, r.local_port),
  },
  {
    title: 'Remote',
    key: 'remote_ip',
    className: 'mono',
    render: (r) => [
      formatEndpoint(r.remote_ip, r.remote_port),
      r.remote_host
        ? h('div', { class: 'muted' }, [r.remote_host, h(HostSourceTag, { source: r.remote_host_source })])
        : null,
    ],
  },
  { title: 'Proto', key: 'proto', width: 70, render: (r) => protocolName(r.proto) },
  {
    title: 'App',
    key: 'app',
    width: 120,
    render: (r) => h(AppTag, { app: r.app, confident: r.app_confident }),
  },
  { title: 'Bytes', key: 'bytes', align: 'right', ...sortable('bytes'), render: (r) => formatBytes(r.bytes) },
  {
    title: 'Packets',
    key: 'packets',
    align: 'right',
    ...sortable('packets'),
    render: (r) => formatNumber(r.packets),
  },
  { title: 'Category', key: 'category_id', render: (r) => h(CategoryChip, { id: r.category_id }) },
  { title: 'Country', key: 'country', render: (r) => remoteCountryLabel(r.remote_ip, r.country) },
  {
    title: 'ASN',
    key: 'asn',
    ellipsis: { tooltip: true },
    render: (r) => (r.asn ? `AS${r.asn} ${r.as_name ?? ''}` : '—'),
  },
])

/** A flow with its position in the page: flow records are not unique (two
 * records can share time, endpoints and protocol), so the row key is the
 * server-ordered position — duplicate keys would leave stale rows on re-sort. */
type PagedFlow = FlowRecord & { readonly rowIndex: number }

const rows = computed<PagedFlow[]>(() => (flows.data.value ?? []).map((r, rowIndex) => ({ ...r, rowIndex })))
const rowKey = (r: PagedFlow): number => r.rowIndex
</script>

<template>
  <div class="stack">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Raw flows</span>
      <SampledBadge :show="flows.meta.value.sampled === true" />
      <NButton size="small" secondary data-testid="flows-export" @click="exportCsv">Export CSV</NButton>
    </div>
    <FilterBar />
    <div class="flows-extra">
      <NInput
        v-model:value="remoteIpInput"
        size="small"
        placeholder="Remote IP"
        clearable
        :status="remoteIpError ? 'error' : undefined"
        data-testid="flows-remote-ip"
        @keyup.enter="apply"
      />
      <NInput
        v-model:value="portInput"
        size="small"
        placeholder="Port"
        clearable
        :status="portError ? 'error' : undefined"
        data-testid="flows-port"
        @keyup.enter="apply"
      />
      <NButton size="small" type="primary" data-testid="flows-apply" @click="apply">Apply</NButton>
      <AppSelect
        :value="apps"
        placeholder="Application"
        allow-unclassified
        data-testid="filter-app"
        @update:value="setApps"
      />
    </div>
    <NAlert v-if="remoteIpError || portError" type="warning" :bordered="false">{{
      remoteIpError ?? portError
    }}</NAlert>
    <NAlert v-if="flows.error.value" type="error" :bordered="false">{{ flows.error.value }}</NAlert>
    <NCard size="small">
      <NDataTable
        remote
        size="small"
        :columns="columns"
        :data="rows"
        :loading="flows.loading.value"
        :pagination="pagination"
        :row-key="rowKey"
        :scroll-x="1220"
        data-testid="flows-table"
        @update:sorter="setSort"
      />
      <div class="muted flows-note">
        CSV export is capped at {{ formatNumber(FLOWS_EXPORT_MAX_ROWS) }} rows.
      </div>
    </NCard>
  </div>
</template>

<style scoped>
.flows-extra {
  display: grid;
  grid-template-columns: minmax(140px, 220px) minmax(90px, 120px) auto minmax(160px, 280px);
  gap: 6px;
}
.flows-note {
  font-size: 12px;
  margin-top: 6px;
}
</style>
